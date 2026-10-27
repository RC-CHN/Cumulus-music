#[allow(dead_code)]
#[path = "support/fm.rs"]
mod support;
use cumulus_core::{Error, account::resources::MutationPhase, fm::*};
use support::{Fault, Fixture};

#[tokio::test]
async fn fixed_reasons_do_not_invent_payload_fields_or_block_artists() {
    let f = Fixture::new();
    let q = f.preload(&[1, 2, 3]).await;
    let token = f.session.token().unwrap();
    let track = q.current().unwrap();
    let choices =
        f.fm.reason_choices_for(&token, &q.source, &track.entry_id)
            .await
            .unwrap();
    assert_eq!(choices.options.len(), 3);
    assert_eq!(
        f.fm.artist_name_for(&token, "1").unwrap().as_deref(),
        Some("fixture artist")
    );
    let op =
        f.fm.trash_reason_for(
            &token,
            &q.source,
            q.revision,
            Some(&FmObservation {
                entry_id: track.entry_id.clone(),
                position_seconds: 12.9,
            }),
            Some(&choices.options[0].key),
        )
        .await
        .unwrap();
    assert_eq!(op.phase, MutationPhase::Confirmed);
    assert!(op.artist_id.is_none());
    f.fm.finish_trash_artist_for(&token, &op).await.unwrap();
    let calls = f.api.calls.lock().unwrap();
    let args = &calls
        .iter()
        .find(|(p, _)| p == "/api/radio/trash/add")
        .unwrap()
        .1;
    assert_eq!(
        args,
        &serde_json::json!({"songId":"1","alg":"alg-1","time":12})
    );
    assert!(!calls.iter().any(|(p, _)| p == "/api/music-blacklist/add"));
}

#[tokio::test]
async fn durable_artist_intent_recovers_after_song_commit_without_duplicate_writes() {
    let f = Fixture::new();
    let q = f.preload(&[1, 2, 3]).await;
    let token = f.session.token().unwrap();
    let choices =
        f.fm.reason_choices_for(&token, &q.source, &q.current().unwrap().entry_id)
            .await
            .unwrap();
    let key = &choices
        .options
        .iter()
        .find(|r| r.artist_id.is_some())
        .unwrap()
        .key;
    let op =
        f.fm.trash_reason_for(&token, &q.source, q.revision, None, Some(key))
            .await
            .unwrap();
    assert_eq!(op.artist_id.as_deref(), Some("1"));
    assert_eq!(f.api.count("/api/music-blacklist/add"), 0);
    let reopened = FmService::new(f.session.clone()).unwrap();
    let saved = reopened
        .trash_operation_for(&token, &op.id)
        .unwrap()
        .unwrap();
    f.api.fault("/api/music-blacklist/add", Fault::Lost);
    assert!(
        reopened
            .finish_trash_artist_for(&token, &saved)
            .await
            .is_err()
    );
    assert_eq!(
        reopened
            .artist_operation_for(&token, &op.id)
            .unwrap()
            .unwrap()
            .phase,
        MutationPhase::Uncertain
    );
    assert!(matches!(
        reopened.change_artist_for(&token, "1", false).await,
        Err(Error::MutationUncertain)
    ));
    reopened
        .finish_trash_artist_for(&token, &saved)
        .await
        .unwrap();
    reopened
        .finish_trash_artist_for(&token, &saved)
        .await
        .unwrap();
    assert_eq!(f.api.count("/api/music-blacklist/add"), 1);
    assert!(
        reopened
            .blacklist_for(&token)
            .unwrap()
            .artists
            .contains("1")
    );
    let restored = reopened
        .change_artist_for(&token, "1", false)
        .await
        .unwrap();
    assert_eq!(restored.phase, MutationPhase::Confirmed);
    assert!(
        !reopened
            .blacklist_for(&token)
            .unwrap()
            .artists
            .contains("1")
    );
    assert_eq!(f.api.count("/api/music-blacklist/delete"), 1);
}

#[tokio::test]
async fn artist_rejection_is_terminal_and_explicit_new_attempt_keeps_old_journal() {
    let f = Fixture::new();
    let token = f.session.token().unwrap();
    f.api.fault("/api/music-blacklist/add", Fault::Reject(403));
    assert!(f.fm.change_artist_for(&token, "1", true).await.is_err());
    let failed = f.fm.artist_operations_for(&token).unwrap().pop().unwrap();
    assert_eq!(failed.phase, MutationPhase::Rejected);
    f.fm.reconcile_artist_for(&token, &failed.id).await.unwrap();
    assert_eq!(f.api.count("/api/music-blacklist/add"), 1);
    let new = f.fm.change_artist_for(&token, "1", true).await.unwrap();
    assert_ne!(new.id, failed.id);
    assert_eq!(
        f.fm.artist_latest_for(&token, "1").unwrap().as_deref(),
        Some(new.id.as_str())
    );
    assert_eq!(
        f.fm.artist_operation_for(&token, &failed.id)
            .unwrap()
            .unwrap()
            .phase,
        MutationPhase::Rejected
    );
}

#[tokio::test]
async fn late_reasons_and_artist_reply_do_not_publish_to_new_account() {
    let f = Fixture::new();
    let q = f.preload(&[1, 2, 3]).await;
    let token = f.session.token().unwrap();
    *f.api.hold.lock().unwrap() = Some("/api/personalized/dislike/reason".into());
    let fm = f.fm.clone();
    let old = token.clone();
    let entry = q.current().unwrap().entry_id.clone();
    let task = tokio::spawn(async move {
        fm.reason_choices_for(&old, &FmSource::Personal, &entry)
            .await
    });
    f.api.started.notified().await;
    f.adopt(10);
    f.api.release.notify_one();
    assert!(task.await.unwrap().is_err());
    assert!(f.store.values("10", "fm-reasons-v1").unwrap().is_empty());
    f.adopt(9);
    let owner = f.session.token().unwrap();
    *f.api.hold.lock().unwrap() = Some("/api/music-blacklist/add".into());
    let fm = f.fm.clone();
    let task = tokio::spawn(async move { fm.change_artist_for(&owner, "1", true).await });
    f.api.started.notified().await;
    f.adopt(10);
    f.api.release.notify_one();
    assert!(task.await.unwrap().is_err());
    assert!(
        f.fm.artist_operations_for(&f.session.token().unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(
        !f.store
            .values("9", "fm-artist-mutations-v1")
            .unwrap()
            .is_empty()
    );
}
