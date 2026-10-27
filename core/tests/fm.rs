#[path = "support/fm.rs"]
mod support;
use cumulus_core::{Error, account::resources::MutationPhase, fm::*};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use support::*;

const GET: &str = "/api/v1/radio/get";
const SKIP: &str = "/api/v1/radio/skip";
const ADD: &str = "/api/radio/trash/add";
const DEL: &str = "/api/radio/trash/del/batch";
const TRASH: &str = "/api/v3/radio/trash/get";

#[tokio::test]
async fn preload_cache_rights_clock_and_alternate_mode_preserve_history() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    let q = f.preload(&[1, 2, 3]).await;
    assert_eq!(q.revision, 1);
    let same =
        f.fm.preload_for(&token, FmSource::Personal, FmRights::default())
            .await
            .unwrap();
    assert_eq!(same.revision, q.revision);
    assert_eq!(f.api.count(GET), 1);
    let rights = FmRights {
        vinyl: true,
        plus: false,
    };
    f.api.batch(&[3, 4, 5]);
    let changed =
        f.fm.preload_for(&token, FmSource::Personal, rights)
            .await
            .unwrap();
    assert_eq!(
        changed
            .tracks
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3", "4", "5"]
    );
    assert_eq!(
        changed.current().unwrap().entry_id,
        q.current().unwrap().entry_id
    );
    assert!(changed.fresh_at(rights, changed.fetched_at + 7_197_000));
    assert!(!changed.fresh_at(rights, changed.fetched_at + 7_197_001));
    assert!(!changed.fresh_at(rights, changed.fetched_at - 1));
    f.clock.0.store(1, Ordering::SeqCst);
    f.api.batch(&[6, 7, 8]);
    let rollback =
        f.fm.preload_for(&token, FmSource::Personal, rights)
            .await
            .unwrap();
    assert_eq!(rollback.tracks.len(), 8);
    let familiar = FmSource::Familiar {
        source_id: "9007199254740993123".into(),
    };
    f.api.batch(&[21, 22, 23]);
    let alt =
        f.fm.preload_for(&token, familiar.clone(), rights)
            .await
            .unwrap();
    f.api.batch(&[23, 24, 25]);
    let alt2 =
        f.fm.preload_for(&token, familiar.clone(), rights)
            .await
            .unwrap();
    assert_eq!(alt2.tracks.len(), 5);
    assert!(alt2.revision > alt.revision);
    let calls = f.api.calls.lock().unwrap();
    assert_eq!(
        calls.last().unwrap().1,
        json!({"imageFm":1,"mode":"FAMILIAR","sourceIds":"9007199254740993123"})
    );
    assert_eq!(
        f.fm.queue_for(&token, &FmSource::Personal)
            .unwrap()
            .unwrap()
            .tracks
            .len(),
        8
    );
}

#[tokio::test]
async fn short_batches_retry_once_deduplicate_and_fail_without_partial_commit() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.api.batch(&[1, 2]);
    f.api.batch(&[2, 3]);
    let q =
        f.fm.preload_for(&token, FmSource::Personal, FmRights::default())
            .await
            .unwrap();
    assert_eq!(q.tracks.len(), 3);
    assert_eq!(f.api.count(GET), 2);
    // Cancellation during a replacement preload must leave the old durable
    // cache untouched, including its cursor and revision.
    f.clock.0.store(q.fetched_at + 7_197_001, Ordering::SeqCst);
    *f.api.hold.lock().unwrap() = Some(GET.into());
    f.api.batch(&[4]);
    let fm = f.fm.clone();
    let t = token.clone();
    let task = tokio::spawn(async move {
        fm.preload_for(&t, FmSource::Personal, FmRights::default())
            .await
    });
    f.api.started.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(
        f.fm.queue_for(&token, &FmSource::Personal)
            .unwrap()
            .unwrap()
            .revision,
        q.revision
    );
    assert!(f.fm.skip_operations_for(&token).unwrap().is_empty());
}

#[tokio::test]
async fn manual_skip_uses_native_clock_and_natural_repeat_does_not_report() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    let q = f.preload(&[1, 2, 3, 4]).await;
    let obs = FmObservation {
        entry_id: q.current().unwrap().entry_id.clone(),
        position_seconds: 12.95,
    };
    let next =
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            q.revision,
            FmStep::ManualNext,
            Some(&obs),
        )
        .await
        .unwrap();
    assert_eq!(next.queue.current().unwrap().id, "2");
    assert_eq!(f.api.count(SKIP), 0);
    let id = next.skip_report.unwrap();
    let prepared = f.fm.skip_operation_for(&token, &id).unwrap().unwrap();
    assert_eq!(prepared.phase, SkipPhase::Prepared);
    assert_eq!(prepared.seconds, 12);
    assert_eq!(prepared.algorithm, "alg-1");
    let sent = f.fm.dispatch_skip_for(&token, &id).await.unwrap();
    assert_eq!(sent.phase, SkipPhase::Confirmed);
    f.fm.dispatch_skip_for(&token, &id).await.unwrap();
    assert_eq!(f.api.count(SKIP), 1);
    assert!(
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            next.queue.revision,
            FmStep::Ended,
            None
        )
        .await
        .is_err()
    );
    f.tick();
    let repeated =
        f.fm.repeat_for(
            &token,
            &FmSource::Personal,
            next.queue.revision,
            FmRepeat::One,
        )
        .unwrap();
    let natural =
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            repeated.revision,
            FmStep::Ended,
            None,
        )
        .await
        .unwrap();
    assert_eq!(natural.queue.current().unwrap().id, "2");
    assert!(natural.skip_report.is_none());
    assert_eq!(f.api.count(GET), 1);
    f.tick();
    let back =
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            natural.queue.revision,
            FmStep::Previous,
            None,
        )
        .await
        .unwrap();
    assert_eq!(back.queue.current().unwrap().id, "1");
    assert!(back.skip_report.is_none());
    f.tick();
    let mismatch = FmObservation {
        entry_id: "different".into(),
        position_seconds: 99.5,
    };
    let next =
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            back.queue.revision,
            FmStep::ManualNext,
            Some(&mismatch),
        )
        .await
        .unwrap();
    assert_eq!(
        f.fm.skip_operation_for(&token, next.skip_report.as_ref().unwrap())
            .unwrap()
            .unwrap()
            .seconds,
        0
    );
    assert!(matches!(
        f.fm.step_for(&token, &FmSource::Personal, q.revision, FmStep::Ended, None)
            .await,
        Err(Error::QueueChanged)
    ));
}

#[tokio::test]
async fn history_is_bounded_refill_failure_keeps_tail_and_exhaustion_does_not_wrap() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    let mut q = f.preload(&(1..=14).collect::<Vec<_>>()).await;
    for _ in 0..12 {
        f.tick();
        q =
            f.fm.step_for(&token, &FmSource::Personal, q.revision, FmStep::Ended, None)
                .await
                .unwrap()
                .queue;
    }
    assert_eq!(q.current().unwrap().id, "13");
    assert_eq!(q.index, 10);
    assert_eq!(q.tracks[0].id, "3");
    f.api.fault(GET, Fault::Before);
    f.tick();
    let tail =
        f.fm.step_for(&token, &FmSource::Personal, q.revision, FmStep::Ended, None)
            .await
            .unwrap();
    assert!(tail.refill_failed);
    assert_eq!(tail.queue.current().unwrap().id, "14");
    f.tick();
    let end =
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            tail.queue.revision,
            FmStep::Ended,
            None,
        )
        .await
        .unwrap();
    assert!(end.exhausted);
    assert!(!end.play);
    assert!(end.queue.current().is_none());
    assert_eq!(end.queue.tracks.len(), 10);
    f.api.batch(&[20, 21, 22]);
    f.tick();
    let resume =
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            end.queue.revision,
            FmStep::Ended,
            None,
        )
        .await
        .unwrap();
    assert_eq!(resume.queue.current().unwrap().id, "20");
    assert!(resume.skip_report.is_none());
    assert!(f.fm.skip_operations_for(&token).unwrap().is_empty());
}

#[tokio::test]
async fn blacklist_filters_songs_artists_and_exhausted_preload_fetches_again() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    {
        let mut b = f.api.blacklist.lock().unwrap();
        b.songs.insert("1".into());
        b.artists.insert("2".into());
    }
    f.fm.refresh_blacklist_for(&token).await.unwrap();
    let q = f.preload(&[1, 2, 3]).await;
    assert_eq!(q.current().unwrap().id, "3");
    f.tick();
    let end =
        f.fm.step_for(&token, &FmSource::Personal, q.revision, FmStep::Ended, None)
            .await
            .unwrap();
    assert!(end.queue.current().is_none());
    f.api.batch(&[4, 5, 6]);
    let q =
        f.fm.preload_for(&token, FmSource::Personal, FmRights::default())
            .await
            .unwrap();
    assert_eq!(q.current().unwrap().id, "4");
    f.adopt(10);
    let other = f.session.token().unwrap();
    assert!(f.fm.blacklist_for(&other).unwrap().songs.is_empty());
    assert!(
        f.fm.queue_for(&other, &FmSource::Personal)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        f.fm.queue_for(&token, &FmSource::Personal),
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn skip_unknown_rejected_and_interrupted_reports_never_replay() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    let mut q = f.preload(&[1, 2, 3, 4, 5, 6]).await;
    for (fault, phase) in [
        (Fault::Lost, SkipPhase::Uncertain),
        (Fault::Reject(403), SkipPhase::Rejected),
    ] {
        f.tick();
        let next =
            f.fm.step_for(
                &token,
                &FmSource::Personal,
                q.revision,
                FmStep::ManualNext,
                None,
            )
            .await
            .unwrap();
        q = next.queue;
        let id = next.skip_report.unwrap();
        f.api.fault(SKIP, fault);
        assert!(f.fm.dispatch_skip_for(&token, &id).await.is_err());
        let reopened = FmService::new(f.session.clone()).unwrap();
        assert_eq!(
            reopened.dispatch_skip_for(&token, &id).await.unwrap().phase,
            phase
        );
    }
    assert_eq!(f.api.count(SKIP), 2);
    f.tick();
    let next =
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            q.revision,
            FmStep::ManualNext,
            None,
        )
        .await
        .unwrap();
    let id = next.skip_report.unwrap();
    *f.api.hold.lock().unwrap() = Some(SKIP.into());
    let fm = f.fm.clone();
    let t = token.clone();
    let op = id.clone();
    let task = tokio::spawn(async move { fm.dispatch_skip_for(&t, &op).await });
    f.api.started.notified().await;
    assert!(matches!(
        f.fm.dispatch_skip_for(&token, &id).await,
        Err(Error::MutationBusy)
    ));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let reopened = FmService::new(f.session.clone()).unwrap();
    assert_eq!(
        reopened.dispatch_skip_for(&token, &id).await.unwrap().phase,
        SkipPhase::Uncertain
    );
    assert_eq!(f.api.count(SKIP), 3);
    assert!(reopened.abandon_skip_for(&token, &id).is_err());
    f.tick();
    let next =
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            next.queue.revision,
            FmStep::ManualNext,
            None,
        )
        .await
        .unwrap();
    let id = next.skip_report.unwrap();
    assert_eq!(
        f.fm.abandon_skip_for(&token, &id).unwrap().phase,
        SkipPhase::Abandoned
    );
    assert_eq!(
        f.fm.dispatch_skip_for(&token, &id).await.unwrap().phase,
        SkipPhase::Abandoned
    );
    assert_eq!(f.api.count(SKIP), 3);
}

#[tokio::test]
async fn trash_acknowledged_then_applied_and_restore_only_changes_selected_membership() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    let q = f.preload(&[1, 2, 3, 4]).await;
    f.api.trash.lock().unwrap().insert("88".into());
    let obs = FmObservation {
        entry_id: q.current().unwrap().entry_id.clone(),
        position_seconds: 7.8,
    };
    let added =
        f.fm.trash_current_for(&token, &FmSource::Personal, q.revision, Some(&obs))
            .await
            .unwrap();
    assert_eq!(added.phase, MutationPhase::Confirmed);
    assert_eq!(added.before, [false]);
    assert_eq!(added.observed, Some(vec![true]));
    assert_eq!(
        f.fm.queue_for(&token, &FmSource::Personal)
            .unwrap()
            .unwrap()
            .current()
            .unwrap()
            .id,
        "1"
    );
    let next =
        f.fm.apply_trash_for(&token, &FmSource::Personal, q.revision, &added.id)
            .await
            .unwrap();
    assert_eq!(next.queue.current().unwrap().id, "2");
    assert!(next.play);
    assert!(next.skip_report.is_none());
    assert!(!next.queue.tracks.iter().any(|t| t.id == "1"));
    assert_eq!(f.api.count(SKIP), 0);
    let restored =
        f.fm.restore_trash_for(&token, vec!["1".into(), "999".into()])
            .await
            .unwrap();
    assert_eq!(restored.phase, MutationPhase::Confirmed);
    assert_eq!(restored.before, [true, false]);
    assert_eq!(*f.api.trash.lock().unwrap(), ["88".into()].into());
    assert!(!f.fm.blacklist_for(&token).unwrap().songs.contains("1"));
    assert_eq!(
        f.fm.restore_trash_for(&token, vec!["1".into()])
            .await
            .unwrap()
            .phase,
        MutationPhase::Unchanged
    );
    assert_eq!(f.api.count(ADD), 1);
    assert_eq!(f.api.count(DEL), 1);
    let calls = f.api.calls.lock().unwrap();
    assert_eq!(
        calls.iter().find(|(p, _)| p == ADD).unwrap().1,
        json!({"songId":"1","alg":"alg-1","time":7})
    );
    assert_eq!(
        calls.iter().find(|(p, _)| p == DEL).unwrap().1,
        json!({"songIds":["1"]})
    );
}

#[tokio::test]
async fn lost_trash_reply_blocks_overlapping_writes_until_read_only_reconciliation() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    let q = f.preload(&[1, 2, 3, 4]).await;
    f.api.fault(ADD, Fault::Lost);
    assert!(
        f.fm.trash_current_for(&token, &FmSource::Personal, q.revision, None)
            .await
            .is_err()
    );
    let record = f.fm.trash_operations_for(&token).unwrap().pop().unwrap();
    assert_eq!(record.phase, MutationPhase::Uncertain);
    assert!(matches!(
        f.fm.apply_trash_for(&token, &FmSource::Personal, q.revision, &record.id)
            .await,
        Err(Error::MutationUncertain)
    ));
    assert!(matches!(
        f.fm.restore_trash_for(&token, vec!["1".into()]).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(f.api.count(DEL), 0);
    let reopened = FmService::new(f.session.clone()).unwrap();
    let confirmed = reopened
        .reconcile_trash_for(&token, &record.id)
        .await
        .unwrap();
    assert_eq!(confirmed.phase, MutationPhase::Confirmed);
    assert_eq!(f.api.count(ADD), 1);
    let next = reopened
        .apply_trash_for(&token, &FmSource::Personal, q.revision, &record.id)
        .await
        .unwrap();
    assert_eq!(next.queue.current().unwrap().id, "2");
    f.api.fault(DEL, Fault::Lost);
    assert!(
        reopened
            .restore_trash_for(&token, vec!["1".into()])
            .await
            .is_err()
    );
    let pending = reopened
        .trash_operations_for(&token)
        .unwrap()
        .into_iter()
        .find(|r| r.phase.pending())
        .unwrap();
    assert_eq!(
        reopened
            .reconcile_trash_for(&token, &pending.id)
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(f.api.count(DEL), 1);
    assert!(f.api.trash.lock().unwrap().is_empty());
}

#[tokio::test]
async fn interrupted_write_and_account_switch_preserve_original_owner_responsibility() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    let q = f.preload(&[1, 2, 3]).await;
    *f.api.hold.lock().unwrap() = Some(ADD.into());
    let fm = f.fm.clone();
    let t = token.clone();
    let revision = q.revision;
    let task = tokio::spawn(async move {
        fm.trash_current_for(&t, &FmSource::Personal, revision, None)
            .await
    });
    f.api.started.notified().await;
    f.adopt(10);
    f.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    let other = f.session.token().unwrap();
    assert!(f.fm.trash_operations_for(&other).unwrap().is_empty());
    assert!(f.fm.blacklist_for(&other).unwrap().songs.is_empty());
    f.adopt(9);
    let token = f.session.token().unwrap();
    let op = f.fm.trash_operations_for(&token).unwrap().pop().unwrap();
    assert_eq!(op.phase, MutationPhase::Uncertain);
    assert_eq!(
        f.fm.reconcile_trash_for(&token, &op.id)
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(f.api.count(ADD), 1);
    // A process/Future can also disappear after the server accepted the restore.
    *f.api.hold.lock().unwrap() = Some(DEL.into());
    let fm = f.fm.clone();
    let t = token.clone();
    let task = tokio::spawn(async move { fm.restore_trash_for(&t, vec!["1".into()]).await });
    f.api.started.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let pending =
        f.fm.trash_operations_for(&token)
            .unwrap()
            .into_iter()
            .find(|r| r.phase.pending())
            .unwrap();
    assert_eq!(pending.phase, MutationPhase::Sending);
    assert_eq!(
        f.fm.reconcile_trash_for(&token, &pending.id)
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(f.api.count(DEL), 1);
}

#[tokio::test]
async fn queue_and_prepared_feedback_commit_atomically_and_reject_invalid_native_clock() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    let q = f.preload(&[1, 2, 3, 4]).await;
    let db = rusqlite::Connection::open(f.dir.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_skip BEFORE INSERT ON state WHEN NEW.kind='fm-skip-v1' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(matches!(
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            q.revision,
            FmStep::ManualNext,
            None
        )
        .await,
        Err(Error::Storage(_))
    ));
    let retained =
        f.fm.queue_for(&token, &FmSource::Personal)
            .unwrap()
            .unwrap();
    assert_eq!(retained.revision, q.revision);
    assert_eq!(retained.index, 0);
    assert!(f.fm.skip_operations_for(&token).unwrap().is_empty());
    assert_eq!(f.api.count(SKIP), 0);
    db.execute_batch("DROP TRIGGER fail_skip;").unwrap();
    for seconds in [f64::NAN, -1.0, f64::INFINITY] {
        let obs = FmObservation {
            entry_id: q.current().unwrap().entry_id.clone(),
            position_seconds: seconds,
        };
        assert!(matches!(
            f.fm.step_for(
                &token,
                &FmSource::Personal,
                q.revision,
                FmStep::ManualNext,
                Some(&obs)
            )
            .await,
            Err(Error::InvalidInput(_))
        ));
    }
    let next =
        f.fm.step_for(
            &token,
            &FmSource::Personal,
            q.revision,
            FmStep::ManualNext,
            None,
        )
        .await
        .unwrap();
    let id = next.skip_report.unwrap();
    let mut corrupt =
        serde_json::to_value(f.fm.skip_operation_for(&token, &id).unwrap().unwrap()).unwrap();
    corrupt["id"] = json!("wrong");
    f.store.put("9", "fm-skip-v1", &id, &corrupt).unwrap();
    assert!(f.fm.dispatch_skip_for(&token, &id).await.is_err());
    assert_eq!(f.api.count(SKIP), 0);
}

#[tokio::test]
async fn trash_pagination_uses_page_indices_and_carries_server_marker() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.api
        .trash
        .lock()
        .unwrap()
        .extend((1..=205).map(|n| n.to_string()));
    let snap = f.fm.refresh_trash_for(&token).await.unwrap();
    assert_eq!(snap.total, 205);
    assert_eq!(snap.ids.len(), 205);
    {
        let calls = f.api.calls.lock().unwrap();
        let rows: Vec<_> = calls
            .iter()
            .filter(|(p, _)| p == TRASH)
            .map(|(_, v)| v)
            .collect();
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[0],
            &json!({"pagesize":100,"from":0,"to":1,"addTime":""})
        );
        assert_eq!(
            rows[1],
            &json!({"pagesize":100,"from":1,"to":2,"addTime":1000})
        );
        assert_eq!(
            rows[2],
            &json!({"pagesize":100,"from":2,"to":3,"addTime":1001})
        );
        assert_eq!(rows[3], rows[0]);
    }
    assert!(
        f.fm.trash_page_for(&token, 0, 100, Value::Null)
            .await
            .is_err()
    );
    assert!(f.fm.restore_trash_for(&token, vec![]).await.is_err());
    assert!(
        f.fm.restore_trash_for(&token, vec!["1".into(), "1".into()])
            .await
            .is_err()
    );
    f.fm.reasons_for(&token, "123456789").await.unwrap();
    let calls = f.api.calls.lock().unwrap();
    let params = &calls.last().unwrap().1;
    assert_eq!(params["page"], "userfm");
    let resource: Value = serde_json::from_str(params["resources"][0].as_str().unwrap()).unwrap();
    assert_eq!(
        resource,
        json!({"resourceId":"123456789","resourceType":4,"targetUrl":"","encId":"123456789"})
    );
}

#[tokio::test]
async fn zero_server_count_requires_an_empty_page_and_does_not_erase_existing_trash() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.api
        .trash
        .lock()
        .unwrap()
        .extend((1..=23).map(|n| n.to_string()));
    f.api.bad_trash.store(4, Ordering::SeqCst);
    let snapshot = f.fm.refresh_trash_for(&token).await.unwrap();
    assert_eq!(snapshot.total, 23);
    assert_eq!(snapshot.reported_total, 0);
    assert_eq!(f.api.count(TRASH), 3);
    assert!(f.fm.blacklist_for(&token).unwrap().songs.contains("23"));
    let unchanged =
        f.fm.restore_trash_for(&token, vec!["999".into()])
            .await
            .unwrap();
    assert_eq!(unchanged.phase, MutationPhase::Unchanged);
    assert_eq!(f.api.count(DEL), 0);
    let restored =
        f.fm.restore_trash_for(&token, vec!["23".into()])
            .await
            .unwrap();
    assert_eq!(restored.phase, MutationPhase::Confirmed);
    assert_eq!(f.api.trash.lock().unwrap().len(), 22);
}

#[tokio::test]
async fn incomplete_duplicate_and_changing_lists_never_replace_complete_cache() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.api
        .trash
        .lock()
        .unwrap()
        .extend((1..=205).map(|n| n.to_string()));
    f.fm.refresh_trash_for(&token).await.unwrap();
    let original = f.store.get("9", "fm-trash-snapshot-v1", "current").unwrap();
    for bad in [1, 2, 3] {
        f.api.bad_trash.store(bad, Ordering::SeqCst);
        assert!(f.fm.refresh_trash_for(&token).await.is_err());
        assert_eq!(
            f.store.get("9", "fm-trash-snapshot-v1", "current").unwrap(),
            original
        );
        assert!(
            f.fm.restore_trash_for(&token, vec!["1".into()])
                .await
                .is_err()
        );
        assert_eq!(f.api.count(DEL), 0);
    }
}

#[tokio::test]
async fn server_success_followed_by_failed_verification_remains_recoverable() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    let q = f.preload(&[1, 2, 3]).await;
    *f.api.hold.lock().unwrap() = Some(ADD.into());
    let fm = f.fm.clone();
    let t = token.clone();
    let task = tokio::spawn(async move {
        fm.trash_current_for(&t, &FmSource::Personal, q.revision, None)
            .await
    });
    f.api.started.notified().await;
    f.api.fault(TRASH, Fault::Before);
    f.api.release.notify_one();
    assert!(task.await.unwrap().is_err());
    let op = f.fm.trash_operations_for(&token).unwrap().pop().unwrap();
    assert_eq!(op.phase, MutationPhase::Uncertain);
    assert_eq!(
        f.fm.reconcile_trash_for(&token, &op.id)
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(f.api.count(ADD), 1);
    // Fail a durable transition after a successful restore: the original
    // Sending row remains, and a new service can settle by reading only.
    *f.api.hold.lock().unwrap() = Some(DEL.into());
    let fm = f.fm.clone();
    let t = token.clone();
    let task = tokio::spawn(async move { fm.restore_trash_for(&t, vec!["1".into()]).await });
    f.api.started.notified().await;
    let db = rusqlite::Connection::open(f.dir.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_terminal BEFORE UPDATE ON state WHEN NEW.kind='fm-trash-mutations-v1' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    f.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::Storage(_))));
    let op =
        f.fm.trash_operations_for(&token)
            .unwrap()
            .into_iter()
            .find(|r| r.phase.pending())
            .unwrap();
    assert_eq!(op.phase, MutationPhase::Sending);
    db.execute_batch("DROP TRIGGER fail_terminal;").unwrap();
    let reopened = FmService::new(f.session.clone()).unwrap();
    assert_eq!(
        reopened
            .reconcile_trash_for(&token, &op.id)
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(f.api.count(DEL), 1);
    assert!(f.api.trash.lock().unwrap().is_empty());
}
