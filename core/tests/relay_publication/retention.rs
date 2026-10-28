use super::*;

const JOURNAL: &str = "relay-publication-intents-v1";
const RECEIPTS: &str = "relay-publication-receipts-v1";
const MIGRATION: &str = "relay-publication-retention-v1";
fn device_key() -> String {
    hex::encode(Sha256::digest(b"SYNTHETIC-native-device"))
}
fn order() -> String {
    format!("relay-publication-order-{}", device_key())
}
fn receipt(w: &World, id: &str) -> PublicationReceipt {
    match w
        .publisher
        .recovery_receipt_for(&w.token(), id)
        .unwrap()
        .unwrap()
    {
        RecoveryReceipt::Recorded(row) => row,
        RecoveryReceipt::Cancelled { .. } => panic!("expected recorded outcome"),
    }
}
// Populate the old on-disk schema, without the new order index. Metadata and
// upload shapes come from an actual recorded native request, not placeholders.
fn legacy(w: &World, count: u64, upload: Option<RelayUpload>) -> Vec<String> {
    let template = w.rows().pop().unwrap();
    w.store
        .transaction(|tx| {
            let mut state = tx.get("9", "relay-publication-v1", &device_key())?.unwrap();
            let start = state["sequence"].as_u64().unwrap();
            let mut ids = Vec::new();
            for n in 1..=count {
                let mut row = template.clone();
                row.sequence = start + n;
                row.id = uuid::Uuid::from_u128(row.sequence as u128).to_string();
                row.phase = PublicationPhase::Confirmed;
                row.recovery_of = None;
                if let Some(upload) = &upload {
                    row.upload = upload.clone();
                }
                tx.put("9", JOURNAL, &row.id, &serde_json::to_value(&row)?)?;
                ids.push(row.id);
            }
            state["sequence"] = json!(start + count);
            state["latest"] = json!(ids.last());
            tx.put("9", "relay-publication-v1", &device_key(), &state)?;
            tx.delete("9", MIGRATION, &device_key())?;
            for key in tx.values("9", &order())?.keys() {
                tx.delete("9", &order(), key)?;
            }
            Ok(ids)
        })
        .unwrap()
}
fn finish_migration(w: &World) {
    for _ in 0..40 {
        w.publisher.compact_for(&w.token()).unwrap();
        if w.store.get("9", MIGRATION, &device_key()).unwrap().unwrap()["complete"] == true {
            return;
        }
    }
    panic!("bounded legacy migration did not finish");
}

#[tokio::test]
async fn old_recovery_and_cancellation_outcomes_survive_churn_reopen_and_account_device_fences() {
    let w = World::new();
    let blocked = w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source)[0].clone();
    w.replies(&[Reply::Lost]);
    w.send().await;
    let cancelled = uuid::Uuid::new_v4().to_string();
    w.publisher
        .cancel_unseen_for(&w.token(), &cancelled, &blocked)
        .unwrap();
    let recovery = uuid::Uuid::new_v4().to_string();
    w.publisher
        .resynchronize_for(&w.token(), &recovery, &blocked)
        .await
        .unwrap();
    w.send().await;
    let before = serde_json::to_value(receipt(&w, &recovery)).unwrap();
    for n in 2..=155 {
        w.record(frame(n, vec![song(n)], 0), PublicationEvent::Source);
        w.send().await;
    }
    assert_eq!(w.rows().len(), 129); // latest + 128 other recent terminal bodies
    assert!(w.store.get("9", JOURNAL, &recovery).unwrap().is_none());
    assert_eq!(w.store.get("9", RECEIPTS, &recovery).unwrap(), Some(before));
    assert_eq!(receipt(&w, &blocked).phase, PublicationPhase::Uncertain);
    assert!(
        w.store
            .values("9", RECEIPTS)
            .unwrap()
            .values()
            .all(|r| r.get("upload").is_none())
    );
    let count = w.wire().len();
    w.api.offline.store(true, Ordering::SeqCst);
    let (session, reopened) = w.reopen();
    assert_eq!(
        reopened
            .resynchronize_for(&session.token().unwrap(), &recovery, &blocked)
            .await
            .unwrap()
            .phase,
        PublicationPhase::Confirmed
    );
    assert!(matches!(
        reopened
            .resynchronize_for(&session.token().unwrap(), &cancelled, &blocked)
            .await,
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        reopened
            .resynchronize_for(&session.token().unwrap(), &recovery, &cancelled)
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(w.wire().len(), count);
    *w.api.device.lock().unwrap() = Some("SYNTHETIC-other-device".into());
    assert!(
        reopened
            .recovery_receipt_for(&session.token().unwrap(), &recovery)
            .is_err()
    );
    *w.api.device.lock().unwrap() = Some("SYNTHETIC-native-device".into());
    session
        .adopt(
            w.api.clone(),
            json!({"userId":10}),
            &session.token().unwrap(),
        )
        .unwrap();
    assert!(
        reopened
            .recovery_receipt_for(&session.token().unwrap(), &recovery)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn legacy_migration_is_paged_restartable_and_does_not_repeat_completed_disk_work() {
    let w = World::new();
    w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
    w.send().await;
    let ids = legacy(&w, 300, None);
    w.api.offline.store(true, Ordering::SeqCst);
    w.publisher.compact_for(&w.token()).unwrap();
    assert_eq!(w.store.values("9", &order()).unwrap().len(), 32);
    assert_eq!(
        w.store.get("9", MIGRATION, &device_key()).unwrap().unwrap()["complete"],
        false
    );
    let (session, publisher) = w.reopen();
    publisher.compact_for(&session.token().unwrap()).unwrap();
    assert_eq!(w.store.values("9", &order()).unwrap().len(), 64);
    finish_migration(&w);
    assert_eq!(w.rows().len(), 129);
    assert!(
        w.store
            .get("9", JOURNAL, ids.last().unwrap())
            .unwrap()
            .is_some()
    );
    assert_eq!(receipt(&w, &ids[0]).phase, PublicationPhase::Confirmed);
    let db = rusqlite::Connection::open(w.root.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER no_redundant_write BEFORE INSERT ON state BEGIN SELECT RAISE(FAIL,'unexpected repeat write'); END;").unwrap();
    publisher.inspect_for(&session.token().unwrap()).unwrap();
    assert_eq!(w.wire().len(), 1);
}

#[tokio::test]
async fn failed_body_removal_rolls_back_receipt_index_and_cursor_together() {
    let w = World::new();
    w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
    w.send().await;
    legacy(&w, 200, None);
    for _ in 0..4 {
        w.publisher.compact_for(&w.token()).unwrap();
    }
    let journal = w.store.values("9", JOURNAL).unwrap();
    let index = w.store.values("9", &order()).unwrap();
    let progress = w.store.get("9", MIGRATION, &device_key()).unwrap();
    let db = rusqlite::Connection::open(w.root.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_compaction BEFORE DELETE ON state WHEN OLD.kind='relay-publication-intents-v1' BEGIN SELECT RAISE(FAIL,'synthetic compaction failure'); END;").unwrap();
    assert!(w.publisher.compact_for(&w.token()).is_err());
    assert!(w.store.values("9", RECEIPTS).unwrap().is_empty());
    assert_eq!(w.store.values("9", JOURNAL).unwrap(), journal);
    assert_eq!(w.store.values("9", &order()).unwrap(), index);
    assert_eq!(
        w.store.get("9", MIGRATION, &device_key()).unwrap(),
        progress
    );
    db.execute_batch("DROP TRIGGER fail_compaction;").unwrap();
    finish_migration(&w);
    assert_eq!(w.rows().len(), 129);
    assert_eq!(w.store.values("9", RECEIPTS).unwrap().len(), 72);
}

#[tokio::test]
async fn full_bodies_survive_maintenance_while_sending_unknown_and_recovering() {
    let w = World::new();
    let sending = w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source)[0].clone();
    w.api.hold.store(true, Ordering::SeqCst);
    let publisher = w.publisher.clone();
    let token = w.token();
    let work = tokio::spawn(async move { publisher.publish_next_for(&token).await });
    w.api.started.notified().await;
    legacy(&w, 160, None);
    finish_migration(&w);
    assert_eq!(
        w.store.get("9", JOURNAL, &sending).unwrap().unwrap()["phase"],
        "sending"
    );
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    let recovery = uuid::Uuid::new_v4().to_string();
    w.publisher
        .resynchronize_for(&w.token(), &recovery, &sending)
        .await
        .unwrap();
    legacy(&w, 160, None);
    finish_migration(&w);
    assert_eq!(
        w.store.get("9", JOURNAL, &sending).unwrap().unwrap()["phase"],
        "uncertain"
    );
    assert_eq!(
        w.store.get("9", JOURNAL, &recovery).unwrap().unwrap()["phase"],
        "prepared"
    );
    assert_eq!(
        w.publisher
            .status_for(&w.token())
            .unwrap()
            .blocked
            .unwrap()
            .id,
        sending
    );
    assert_eq!(w.wire().len(), 1);
    assert_eq!(w.send().await, PublishOutcome::Confirmed(recovery));
}

#[tokio::test]
async fn payload_byte_limit_compacts_large_cumulative_commands_before_count_limit() {
    let w = World::new();
    w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
    w.send().await;
    let mut machine = cumulus_core::relay::RelayMachine::start(
        RelaySource::Resources {
            resources: vec![song(1)],
        },
        song(1),
    )
    .unwrap();
    let resources = (2..=129).map(song).collect::<Vec<_>>();
    let mut upload = None;
    for _ in 0..200 {
        upload = Some(
            machine
                .command(RelayCommand::Delete {
                    resources: resources.clone(),
                })
                .unwrap(),
        );
    }
    legacy(&w, 30, upload);
    let before: usize = w
        .store
        .values("9", JOURNAL)
        .unwrap()
        .values()
        .map(|r| serde_json::to_vec(r).unwrap().len())
        .sum();
    assert!(before > 8 * 1024 * 1024);
    finish_migration(&w);
    let latest = w
        .publisher
        .status_for(&w.token())
        .unwrap()
        .latest
        .unwrap()
        .id;
    let retained = w.store.values("9", JOURNAL).unwrap();
    let bytes: usize = retained
        .iter()
        .filter(|(id, _)| *id != &latest)
        .map(|(_, r)| serde_json::to_vec(r).unwrap().len())
        .sum();
    assert!(bytes <= 8 * 1024 * 1024);
    assert!(retained.len() < 30);
    assert_eq!(
        retained.len() + w.store.values("9", RECEIPTS).unwrap().len(),
        31
    );
    assert_eq!(w.wire().len(), 1);
}

#[test]
fn audio_recorder_does_not_run_historical_migration_or_payload_compaction() {
    let w = World::new();
    w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
    legacy(&w, 300, None);
    let id = w.record(frame(2, vec![song(2)], 0), PublicationEvent::Source)[0].clone();
    assert!(
        w.store
            .get("9", MIGRATION, &device_key())
            .unwrap()
            .is_none()
    );
    assert!(w.store.values("9", RECEIPTS).unwrap().is_empty());
    assert_eq!(w.store.values("9", JOURNAL).unwrap().len(), 302);
    assert_eq!(
        w.store.get("9", JOURNAL, &id).unwrap().unwrap()["phase"],
        "prepared"
    );
}

#[tokio::test]
async fn migration_leaves_other_accounts_and_device_bodies_untouched() {
    let w = World::new();
    w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
    w.send().await;
    let ids = legacy(&w, 140, None);
    let mut raw = w.store.get("9", JOURNAL, &ids[0]).unwrap().unwrap();
    w.store.put("10", JOURNAL, &ids[0], &raw).unwrap();
    let foreign = uuid::Uuid::from_u128(5000).to_string();
    raw["id"] = json!(foreign);
    raw["device_id"] = json!("SYNTHETIC-other-device");
    w.store.put("9", JOURNAL, &foreign, &raw).unwrap();
    finish_migration(&w);
    assert_eq!(w.store.get("9", JOURNAL, &foreign).unwrap().unwrap(), raw);
    assert!(w.store.get("9", RECEIPTS, &foreign).unwrap().is_none());
    assert!(w.store.get("10", JOURNAL, &ids[0]).unwrap().is_some());
    assert!(w.store.values("10", RECEIPTS).unwrap().is_empty());
    assert_eq!(w.rows().len(), 129);
}

#[tokio::test]
async fn new_recovery_before_legacy_cursor_is_still_indexed_and_later_compacted() {
    let w = World::new();
    let blocked = w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source)[0].clone();
    w.replies(&[Reply::Lost]);
    w.send().await;
    legacy(&w, 180, None);
    w.publisher.compact_for(&w.token()).unwrap();
    let cursor = w.store.get("9", MIGRATION, &device_key()).unwrap().unwrap()["cursor"]
        .as_str()
        .unwrap()
        .to_owned();
    let recovery = uuid::Uuid::nil().to_string();
    assert!(recovery < cursor);
    w.publisher
        .resynchronize_for(&w.token(), &recovery, &blocked)
        .await
        .unwrap();
    assert_eq!(w.send().await, PublishOutcome::Confirmed(recovery.clone()));
    finish_migration(&w);
    for n in 2..=140 {
        w.record(frame(n, vec![song(n)], 0), PublicationEvent::Source);
        w.send().await;
    }
    assert!(w.store.get("9", JOURNAL, &recovery).unwrap().is_none());
    assert_eq!(receipt(&w, &recovery).phase, PublicationPhase::Confirmed);
    let count = w.wire().len();
    w.api.offline.store(true, Ordering::SeqCst);
    assert_eq!(
        w.publisher
            .resynchronize_for(&w.token(), &recovery, &blocked)
            .await
            .unwrap()
            .phase,
        PublicationPhase::Confirmed
    );
    assert_eq!(w.wire().len(), count);
}
