#[path = "support/cloud_upload.rs"]
mod support;
use cumulus_core::{Error, cloud::upload::*};
use std::time::Duration;
use support::*;
fn done(f: &Fixture) {
    f.api.metadata(true);
    f.api.published();
    f.api.listed(true);
}
#[tokio::test]
async fn ready_tasks_are_sequential_durable_and_terminal_tasks_are_not_replayed() {
    let f = Fixture::new();
    let first = f.create().await;
    let second = f.create().await;
    let scheduler = UploadScheduler::new(f.manager.clone());
    scheduler.start(&first, false).unwrap();
    f.tick(1);
    scheduler.start(&second, false).unwrap();
    for id in [&first, &second] {
        f.api.check(false);
        done(&f);
        let task = scheduler.step().await.unwrap().unwrap();
        assert_eq!(&task.id, id);
        assert_eq!(task.phase, CloudPhase::Complete);
    }
    assert!(scheduler.step().await.unwrap().is_none());
    assert!(
        scheduler
            .entries()
            .unwrap()
            .iter()
            .all(|e| !e.enabled && !e.running)
    );
    scheduler.dismiss(&first).unwrap();
    let reopened = UploadScheduler::new(f.reopened());
    reopened.recover().await.unwrap();
    assert!(
        reopened
            .entries()
            .unwrap()
            .iter()
            .find(|e| e.id == first)
            .unwrap()
            .hidden
    );
    assert!(reopened.step().await.unwrap().is_none());
    assert_eq!(
        f.api.calls().iter().filter(|(p, _)| p == PUBLISH).count(),
        2
    );
}
#[tokio::test]
async fn pause_during_transfer_wins_over_worker_completion_and_explicit_resume_reuses_child() {
    let f = Fixture::new();
    let id = f.create().await;
    let scheduler = UploadScheduler::new(f.manager.clone());
    scheduler.start(&id, false).unwrap();
    f.api.check(true);
    f.api.allocate();
    let gate = nos::Gate::new();
    *f.objects.object("audio").gate.lock().unwrap() = Some(gate.clone());
    let worker = scheduler.clone();
    let handle = tokio::spawn(async move { worker.step().await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    assert!(matches!(scheduler.step().await, Err(Error::MutationBusy)));
    scheduler.control(&id, UploadControl::Pause).await.unwrap();
    gate.release.add_permits(1);
    assert_eq!(handle.await.unwrap().unwrap().unwrap().bytes, 3);
    let state = scheduler.entries().unwrap().pop().unwrap();
    assert!(!state.enabled && !state.running);
    let calls = f.objects.calls();
    assert!(scheduler.step().await.unwrap().is_none());
    assert_eq!(f.objects.calls(), calls);
    scheduler.start(&id, false).unwrap();
    done(&f);
    assert_eq!(
        scheduler.step().await.unwrap().unwrap().phase,
        CloudPhase::Complete
    );
    assert_eq!(f.api.calls().iter().filter(|(p, _)| p == ALLOC).count(), 1);
}
#[tokio::test]
async fn reopening_pauses_pending_work_and_unknown_mutations_cannot_be_auto_retried() {
    let f = Fixture::new();
    let id = f.create().await;
    let scheduler = UploadScheduler::new(f.manager.clone());
    scheduler.start(&id, false).unwrap();
    let reopened = UploadScheduler::new(f.reopened());
    reopened.recover().await.unwrap();
    assert!(reopened.step().await.unwrap().is_none());
    assert_eq!(
        f.manager.get(&id).unwrap().requested,
        Some(UploadControl::Pause)
    );
    assert!(f.api.calls().is_empty());
    reopened.start(&id, false).unwrap();
    f.api.check(true);
    f.api.lost(ALLOC);
    assert_eq!(
        reopened.step().await.unwrap().unwrap().phase,
        CloudPhase::AllocationPending
    );
    assert_eq!(
        reopened.entries().unwrap()[0].error.as_deref(),
        Some("write_outcome_unknown")
    );
    let calls = f.api.calls().len();
    let again = UploadScheduler::new(f.reopened());
    again.recover().await.unwrap();
    assert!(matches!(
        again.start(&id, true),
        Err(Error::MutationUncertain)
    ));
    assert!(again.step().await.unwrap().is_none());
    assert_eq!(f.api.calls().len(), calls);
}
#[tokio::test]
async fn explicit_rejection_retry_waits_for_user_and_encoding_waits_for_persisted_deadline() {
    let f = Fixture::new();
    let id = f.create().await;
    let scheduler = UploadScheduler::new(f.manager.clone());
    scheduler.start(&id, false).unwrap();
    f.api.check(false);
    f.api.push(META, serde_json::json!({"code":403}));
    assert_eq!(
        scheduler.step().await.unwrap().unwrap().phase,
        CloudPhase::Failed
    );
    assert!(scheduler.step().await.unwrap().is_none());
    assert!(scheduler.start(&id, false).is_err());
    scheduler.start(&id, true).unwrap();
    f.api.metadata(false);
    let waiting = scheduler.step().await.unwrap().unwrap();
    assert_eq!(waiting.phase, CloudPhase::Encoding);
    let calls = f.api.calls().len();
    assert!(scheduler.step().await.unwrap().is_none());
    assert_eq!(f.api.calls().len(), calls);
    f.tick(500);
    f.api.push(
        STATUS,
        serde_json::json!({"code":200,"statuses":{"700":{"status":9}}}),
    );
    f.api.published();
    f.api.listed(true);
    assert_eq!(
        scheduler.step().await.unwrap().unwrap().phase,
        CloudPhase::Complete
    );
    assert_eq!(f.api.calls().iter().filter(|(p, _)| p == CHECK).count(), 1);
}
#[tokio::test]
async fn scheduler_intent_failure_sends_no_requests_and_old_generation_cannot_continue() {
    let f = Fixture::new();
    let id = f.create().await;
    let scheduler = UploadScheduler::new(f.manager.clone());
    scheduler.start(&id, false).unwrap();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_dispatch BEFORE INSERT ON state WHEN NEW.kind='cloud_schedule_v1' AND json_extract(NEW.value,'$.running')=1 BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(matches!(scheduler.step().await, Err(Error::Storage(_))));
    assert!(f.api.calls().is_empty());
    db.execute_batch("DROP TRIGGER fail_dispatch;").unwrap();
    f.session
        .adopt(
            f.api.clone(),
            serde_json::json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(scheduler.step().await, Err(Error::StaleResponse)));
    assert!(matches!(scheduler.entries(), Err(Error::StaleResponse)));
}
