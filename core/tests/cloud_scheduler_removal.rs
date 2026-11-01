#[path = "support/cloud_upload.rs"]
mod support;
use cumulus_core::{Error, cloud::upload::*};
use support::*;
#[tokio::test]
async fn removing_waiting_work_is_durable_and_never_removes_the_users_input_or_restarts_it() {
    let f = Fixture::new();
    let id = f.create().await;
    let scheduler = UploadScheduler::new(f.manager.clone());
    let removed = scheduler.remove(&id, false).await.unwrap();
    assert_eq!(removed.phase, CloudPhase::Cancelled);
    let entry = scheduler.entries().unwrap().pop().unwrap();
    assert!(entry.hidden && entry.removing && !entry.enabled);
    assert!(scheduler.start(&id, true).is_err());
    assert_eq!(std::fs::read(&f.source).unwrap(), DATA);
    scheduler.remove(&id, false).await.unwrap();
    let reopened = UploadScheduler::new(f.reopened());
    reopened.recover().await.unwrap();
    assert!(reopened.entries().unwrap()[0].hidden);
    assert!(reopened.step().await.unwrap().is_none());
    assert!(f.api.calls().is_empty());
}
#[tokio::test]
async fn removing_an_inflight_transfer_waits_for_its_actual_cancellation_before_hiding() {
    let f = Fixture::new();
    let id = f.create().await;
    let scheduler = UploadScheduler::new(f.manager.clone());
    scheduler.start(&id, false).unwrap();
    f.api.check(true);
    f.api.allocate();
    let gate = nos::Gate::new();
    *f.objects.object("audio").gate.lock().unwrap() = Some(gate.clone());
    let worker = scheduler.clone();
    let running = tokio::spawn(async move { worker.step().await });
    gate.entered.notified().await;
    let pending = scheduler.remove(&id, false).await.unwrap();
    assert_ne!(pending.phase, CloudPhase::Cancelled);
    let entry = scheduler.entries().unwrap().pop().unwrap();
    assert!(entry.removing && !entry.hidden && !entry.enabled);
    let count = f.objects.calls();
    scheduler.remove(&id, false).await.unwrap();
    assert_eq!(count, f.objects.calls());
    gate.release.add_permits(1);
    assert_eq!(
        running.await.unwrap().unwrap().unwrap().phase,
        CloudPhase::Cancelled
    );
    let entry = scheduler.entries().unwrap().pop().unwrap();
    assert!(entry.hidden && !entry.running);
    assert_eq!(
        f.api
            .calls()
            .iter()
            .map(|(p, _)| p.as_str())
            .collect::<Vec<_>>(),
        [CHECK, ALLOC]
    );
    assert!(f.source.exists());
}
#[tokio::test]
async fn unknown_allocation_cannot_be_removed_or_implicitly_replayed() {
    let f = Fixture::new();
    let id = f.create().await;
    let scheduler = UploadScheduler::new(f.manager.clone());
    scheduler.start(&id, false).unwrap();
    f.api.check(true);
    f.api.lost(ALLOC);
    scheduler.step().await.unwrap();
    let count = f.api.calls().len();
    assert!(matches!(
        scheduler.remove(&id, false).await,
        Err(Error::MutationUncertain)
    ));
    let entry = scheduler.entries().unwrap().pop().unwrap();
    assert!(!entry.hidden && !entry.removing);
    assert!(matches!(
        scheduler.remove(&id, true).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(f.api.calls().len(), count);
    assert!(f.source.exists());
}
#[tokio::test]
async fn failed_hide_commit_after_actual_cancel_can_retry_locally_without_another_remote_write() {
    let f = Fixture::new();
    let id = f.create().await;
    let scheduler = UploadScheduler::new(f.manager.clone());
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_upload_removal BEFORE UPDATE ON state WHEN NEW.kind='cloud_schedule_v1' AND json_extract(NEW.value,'$.removing')=1 BEGIN SELECT RAISE(ABORT,'synthetic removal commit failure'); END;").unwrap();
    assert!(scheduler.remove(&id, false).await.is_err());
    assert_eq!(f.manager.get(&id).unwrap().phase, CloudPhase::Cancelled);
    assert!(!scheduler.entries().unwrap()[0].hidden);
    db.execute_batch("DROP TRIGGER reject_upload_removal")
        .unwrap();
    scheduler.remove(&id, false).await.unwrap();
    assert!(scheduler.entries().unwrap()[0].hidden);
    assert!(f.api.calls().is_empty());
    assert!(f.source.exists());
}
#[tokio::test]
async fn completed_history_removal_does_not_delete_the_published_cloud_resource() {
    let f = Fixture::new();
    let id = f.create().await;
    let scheduler = UploadScheduler::new(f.manager.clone());
    scheduler.start(&id, false).unwrap();
    f.api.check(false);
    f.api.metadata(true);
    f.api.published();
    f.api.listed(true);
    scheduler.step().await.unwrap();
    let before = f.api.calls().len();
    assert_eq!(
        scheduler.remove(&id, false).await.unwrap().phase,
        CloudPhase::Complete
    );
    assert!(scheduler.entries().unwrap()[0].hidden);
    assert_eq!(f.api.calls().len(), before);
    assert!(f.source.exists());
}
