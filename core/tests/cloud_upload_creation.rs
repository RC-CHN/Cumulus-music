#[path = "support/cloud_upload.rs"]
mod support;
use cumulus_core::{Error, cloud::upload::*};
use std::sync::Arc;
use support::*;
const REQUEST: &str = "0123456789abcdef0123456789abcdef";
#[tokio::test]
async fn a_lost_creation_reply_reopens_the_same_task_without_source_access_or_another_remote_write()
{
    let f = Fixture::new();
    let (first, fresh) = f
        .manager
        .create_once(REQUEST, &f.source, AttachmentInputs::default(), true)
        .await
        .unwrap();
    assert!(fresh);
    f.manager
        .control(&first.id, UploadControl::Pause)
        .await
        .unwrap();
    std::fs::remove_file(&f.source).unwrap();
    let (again, fresh) = f
        .reopened()
        .create_once(REQUEST, &f.source, AttachmentInputs::default(), false)
        .await
        .unwrap();
    assert!(!fresh);
    assert_eq!(again.id, first.id);
    assert_eq!(again.requested, Some(UploadControl::Pause));
    assert_eq!(f.manager.list().unwrap().len(), 1);
    assert!(f.api.calls().is_empty());
    let different = f.root.path().join("other.flac");
    assert!(
        f.manager
            .create_once(REQUEST, &different, AttachmentInputs::default(), true)
            .await
            .is_err()
    );
    assert!(
        f.manager
            .create_once(
                "fedcba9876543210fedcba9876543210",
                &different,
                AttachmentInputs::default(),
                false
            )
            .await
            .is_err()
    );
}
#[tokio::test]
async fn receipt_and_task_commit_together_and_the_same_selection_can_retry_after_a_failed_commit() {
    let f = Fixture::new();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_creation BEFORE INSERT ON state WHEN NEW.kind='cloud_upload_creations_v1' BEGIN SELECT RAISE(ABORT,'synthetic receipt failure'); END;").unwrap();
    assert!(
        f.manager
            .create_once(REQUEST, &f.source, AttachmentInputs::default(), true)
            .await
            .is_err()
    );
    assert!(f.manager.list().unwrap().is_empty());
    db.execute_batch("DROP TRIGGER fail_creation").unwrap();
    let (task, fresh) = f
        .manager
        .create_once(REQUEST, &f.source, AttachmentInputs::default(), true)
        .await
        .unwrap();
    assert!(fresh);
    let (again, fresh) = f
        .reopened()
        .create_once(REQUEST, &f.source, AttachmentInputs::default(), true)
        .await
        .unwrap();
    assert!(!fresh);
    assert_eq!(task.id, again.id);
    assert_eq!(f.manager.list().unwrap().len(), 1);
    assert!(f.api.calls().is_empty());
}
#[tokio::test]
async fn a_receipt_is_readable_while_the_remote_worker_holds_the_task_lock() {
    let f = Fixture::new();
    let (first, _) = f
        .manager
        .create_once(REQUEST, &f.source, AttachmentInputs::default(), true)
        .await
        .unwrap();
    let gate = Arc::new(support::nos::Gate {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Semaphore::new(0),
    });
    f.api.check(true);
    *f.api.gate.lock().unwrap() = Some((CHECK, gate.clone()));
    let manager = f.manager.clone();
    let id = first.id.clone();
    let worker = tokio::spawn(async move { manager.run(&id, CloudRunOptions::default()).await });
    gate.entered.notified().await;
    let (same, fresh) = f
        .manager
        .create_once(REQUEST, &f.source, AttachmentInputs::default(), true)
        .await
        .unwrap();
    assert!(!fresh);
    assert_eq!(same.id, first.id);
    f.manager
        .control(&first.id, UploadControl::Pause)
        .await
        .unwrap();
    gate.release.add_permits(1);
    worker.await.unwrap().unwrap();
    assert_eq!(f.api.calls().len(), 1);
}
#[tokio::test]
async fn reused_creation_ids_never_cross_accounts_or_restart_cancelled_tasks() {
    let f = Fixture::new();
    let (task, _) = f
        .manager
        .create_once(REQUEST, &f.source, AttachmentInputs::default(), true)
        .await
        .unwrap();
    f.manager
        .control(&task.id, UploadControl::Cancel)
        .await
        .unwrap();
    let (same, fresh) = f
        .manager
        .create_once(REQUEST, &f.source, AttachmentInputs::default(), true)
        .await
        .unwrap();
    assert!(!fresh);
    assert_eq!(same.phase, CloudPhase::Cancelled);
    f.session
        .adopt(
            f.api.clone(),
            serde_json::json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.manager
            .create_once(REQUEST, &f.source, AttachmentInputs::default(), true)
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(f.api.calls().is_empty());
}
