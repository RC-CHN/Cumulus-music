#[path = "support/nos.rs"]
mod support;
use cumulus_core::{Error, account::AccountSession, cloud::nos::*, storage::StateStore};
use md5::Digest;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use support::{Backend, Fault, Gate, RenewApi};
const DATA: &[u8] = b"abcdefghi";
struct Fixture {
    _root: tempfile::TempDir,
    source: PathBuf,
    store: Arc<StateStore>,
    session: Arc<AccountSession>,
    api: Arc<RenewApi>,
    backend: Arc<Backend>,
    manager: NosUploads,
}
fn allocation(token: &str) -> NosAllocation {
    NosAllocation::parse(&json!({"channel":1,"bucket":"bucket-test","objectKey":"a/中文+#%.flac","token":token,"resourceId":"resource-model"})).unwrap()
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.flac");
        fs::write(&source, DATA).unwrap();
        let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
        let api = Arc::new(RenewApi::default());
        let backend = Arc::new(Backend::default());
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let manager = NosUploads::with_backend(session.clone(), backend.clone(), 3).unwrap();
        Self {
            _root: root,
            source,
            store,
            session,
            api,
            backend,
            manager,
        }
    }
    async fn create(&self) -> String {
        self.manager
            .create(
                Some("owned-task"),
                &self.source,
                allocation("old"),
                NosNode::parse("http://nosup-test.127.net").unwrap(),
                &hex::encode(md5::Md5::digest(DATA)),
                "audio/flac",
            )
            .await
            .unwrap()
            .id
    }
    fn reopened(&self) -> NosUploads {
        let store = Arc::new(StateStore::open(self.store.directory()).unwrap());
        let session = Arc::new(AccountSession::new(self.api.clone(), Some(store)));
        session
            .adopt(
                self.api.clone(),
                json!({"userId":1}),
                &session.token().unwrap(),
            )
            .unwrap();
        NosUploads::with_backend(session, self.backend.clone(), 1).unwrap()
    }
}
fn budget(n: usize) -> NosRunOptions {
    NosRunOptions {
        chunk_budget: Some(n),
        ..Default::default()
    }
}
#[test]
fn scope_rejects_foreign_hosts_credentials_ambiguous_keys_and_invalid_channels() {
    for value in [
        "https://127.0.0.1",
        "https://nosup-test.127.net.evil.test",
        "https://user@nosup-test.127.net",
        "https://nosup-test.127.net/path",
        "https://nosup-test.127.net?q=1",
        "https://nosup--test.127.net",
        "https://nosup-test.127.net:4000",
    ] {
        assert!(NosNode::parse(value).is_err());
    }
    assert_eq!(
        NosNode::parse("http://nosup-test.127.net")
            .unwrap()
            .as_str(),
        "https://nosup-test.127.net"
    );
    let mut a = json!({"channel":1,"bucket":"bucket-test","objectKey":"key","token":"token","resourceId":1});
    for (key, value) in [
        ("channel", json!(3)),
        ("objectKey", json!("..")),
        ("token", json!("a\r\nb")),
        ("token", json!("a\u{7f}b")),
        ("bucket", json!("bucket/evil")),
        ("key", json!("different")),
    ] {
        let mut b = a.clone();
        b[key] = value;
        assert!(NosAllocation::parse(&b).is_err());
    }
    a["resourceId"] = json!(false);
    assert!(NosAllocation::parse(&a).is_err());
}
#[tokio::test]
async fn durable_pause_reopen_queries_remote_offset_and_keeps_original_chunk_size_and_exact_bytes()
{
    let f = Fixture::new();
    let id = f.create().await;
    let paused = f.manager.run(&id, budget(1)).await.unwrap();
    assert_eq!(paused.status, NosStatus::Paused);
    assert_eq!(paused.bytes, 3);
    assert_eq!(f.create().await, id); // Stable child creation cannot reset progress.
    let done = f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(done.status, NosStatus::Uploaded);
    assert_eq!(done.bytes, DATA.len() as u64);
    let calls = f.backend.calls();
    assert_eq!(
        calls
            .iter()
            .map(|c| (c.query, c.offset, c.body.len(), c.complete))
            .collect::<Vec<_>>(),
        vec![
            (false, 0, 3, false),
            (true, 0, 0, false),
            (false, 3, 3, false),
            (false, 6, 3, true)
        ]
    );
    assert!(
        calls[0]
            .url
            .starts_with("https://nosup-test.127.net/bucket-test/a%2F")
    );
    assert!(calls[0].url.contains("%23%25.flac"));
    assert_eq!(calls[0].md5, hex::encode(md5::Md5::digest(DATA)));
    assert_eq!(calls[0].content_type, "audio/flac");
    assert!(
        calls[1]
            .url
            .contains("?uploadContext&context=context-model&version=1.0")
    );
    assert_eq!(f.backend.state.lock().unwrap().data, DATA);
    let public = serde_json::to_string(&done).unwrap();
    for secret in [
        "context-model",
        "resource-model",
        "objectKey",
        "scoped_token",
        "\"token\"",
        "nosup-test",
    ] {
        assert!(!public.contains(secret));
    }
}
#[tokio::test]
async fn intermediate_lost_ack_is_reconciled_without_resending_accepted_bytes() {
    let f = Fixture::new();
    let id = f.create().await;
    f.manager.run(&id, budget(1)).await.unwrap();
    f.backend
        .state
        .lock()
        .unwrap()
        .faults
        .extend([None, Some(Fault::LostReply)]);
    assert!(f.manager.run(&id, budget(1)).await.is_err());
    assert_eq!(f.manager.get(&id).unwrap().bytes, 3);
    assert!(f.manager.get(&id).unwrap().pending);
    let done = f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(done.status, NosStatus::Uploaded);
    assert_eq!(
        f.backend
            .calls()
            .iter()
            .filter(|c| !c.query)
            .map(|c| c.offset)
            .collect::<Vec<_>>(),
        vec![0, 3, 6]
    );
}
#[tokio::test]
async fn first_and_final_lost_ack_never_replay_a_write_even_after_reopen_or_explicit_token_replacement()
 {
    for final_chunk in [false, true] {
        let f = Fixture::new();
        let id = f.create().await;
        if final_chunk {
            f.manager.run(&id, budget(2)).await.unwrap();
            f.backend.state.lock().unwrap().faults.push_back(None);
        }
        f.backend.fault(Fault::LostReply);
        assert!(matches!(
            f.manager.run(&id, Default::default()).await,
            Err(Error::MutationUncertain)
        ));
        let count = f.backend.calls().len();
        let reopened = f.reopened();
        reopened.replace_token(&id, "replacement").unwrap();
        assert!(matches!(
            reopened.run(&id, Default::default()).await,
            Err(Error::MutationUncertain)
        ));
        assert_eq!(count, f.backend.calls().len());
        assert_eq!(reopened.get(&id).unwrap().status, NosStatus::Ambiguous);
    }
}
#[tokio::test]
async fn missing_first_context_and_out_of_range_ack_are_uncertain_but_unknown_remote_bytes_never_advance()
 {
    for fault in [Fault::MissingContext, Fault::Offset(8)] {
        let f = Fixture::new();
        let id = f.create().await;
        f.backend.fault(fault);
        assert!(matches!(
            f.manager.run(&id, Default::default()).await,
            Err(Error::MutationUncertain)
        ));
        assert_eq!(f.manager.get(&id).unwrap().bytes, 0);
    }
    let f = Fixture::new();
    let id = f.create().await;
    f.manager.run(&id, budget(1)).await.unwrap();
    f.backend.fault(Fault::Offset(9));
    assert!(f.manager.run(&id, Default::default()).await.is_err());
    assert_eq!(f.manager.get(&id).unwrap().bytes, 3);
    assert_eq!(f.backend.calls().iter().filter(|c| !c.query).count(), 1);
}
#[tokio::test]
async fn acknowledged_auth_rejection_renews_only_the_same_object_once_and_can_resend_first_or_last()
{
    for final_chunk in [false, true] {
        let f = Fixture::new();
        let id = f.create().await;
        if final_chunk {
            f.manager.run(&id, budget(2)).await.unwrap();
            f.backend.state.lock().unwrap().faults.push_back(None);
            f.backend.fault(Fault::Http(403));
        } else {
            f.backend.state.lock().unwrap().required_token = Some("fresh".into());
        }
        let done = f.manager.run(&id, Default::default()).await.unwrap();
        assert_eq!(done.status, NosStatus::Uploaded);
        assert_eq!(done.renewal, Some(NosRenewal::Confirmed));
        assert_eq!(f.api.calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.backend.state.lock().unwrap().data, DATA);
        assert!(f.backend.calls().iter().any(|c| c.token == "fresh"));
    }
}
#[tokio::test]
async fn failed_or_rejected_refresh_is_durable_and_requires_explicit_renewal() {
    for failed in [false, true] {
        let f = Fixture::new();
        let id = f.create().await;
        f.backend.state.lock().unwrap().required_token = Some("allowed".into());
        if failed {
            f.api
                .replies
                .lock()
                .unwrap()
                .push_back(Err(Error::Transport("lost refresh".into())));
        }
        assert!(f.manager.run(&id, Default::default()).await.is_err());
        assert!(!f.manager.get(&id).unwrap().pending);
        let count = f.api.calls.load(Ordering::SeqCst);
        assert_eq!(count, 1);
        assert!(f.reopened().run(&id, Default::default()).await.is_err());
        assert_eq!(f.api.calls.load(Ordering::SeqCst), count);
        f.api
            .replies
            .lock()
            .unwrap()
            .push_back(Ok("allowed".into()));
        f.manager.renew_token(&id).await.unwrap();
        assert_eq!(
            f.manager.run(&id, Default::default()).await.unwrap().status,
            NosStatus::Uploaded
        );
        assert_eq!(f.api.calls.load(Ordering::SeqCst), 2);
    }
}
#[tokio::test]
async fn lost_context_can_restart_only_when_first_or_final_write_is_not_unknown() {
    let f = Fixture::new();
    let id = f.create().await;
    f.manager.run(&id, budget(1)).await.unwrap();
    f.backend.fault(Fault::ContextLost);
    assert_eq!(
        f.reopened()
            .run(&id, Default::default())
            .await
            .unwrap()
            .status,
        NosStatus::Uploaded
    );
    assert_eq!(f.backend.state.lock().unwrap().data, DATA);
    assert_eq!(
        f.backend
            .calls()
            .iter()
            .filter(|c| !c.query && c.offset == 0)
            .count(),
        2
    );
}
#[tokio::test]
async fn source_changes_block_before_any_request_and_after_last_ack_before_success() {
    let f = Fixture::new();
    let id = f.create().await;
    fs::write(&f.source, b"changed").unwrap();
    assert!(f.manager.run(&id, Default::default()).await.is_err());
    assert!(f.backend.calls().is_empty());
    let f = Fixture::new();
    let id = f.create().await;
    f.manager.run(&id, budget(2)).await.unwrap();
    // Hold the final reply, not the reconciliation query.
    let gate = Gate::new();
    *f.backend.gate.lock().unwrap() = Some(gate.clone());
    let m = f.manager.clone();
    let task = tokio::spawn(async move { m.run(&id, Default::default()).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    fs::write(&f.source, b"abcdefghiX").unwrap();
    gate.release.add_permits(1);
    assert!(task.await.unwrap().is_err());
    assert_ne!(
        f.manager.get("owned-task").unwrap().status,
        NosStatus::Uploaded
    );
}
#[tokio::test]
async fn concurrent_worker_controls_are_durable_and_cancel_does_not_delete_source_or_remote_object()
{
    let f = Fixture::new();
    let id = f.create().await;
    let gate = Gate::new();
    *f.backend.gate.lock().unwrap() = Some(gate.clone());
    let m = f.manager.clone();
    let key = id.clone();
    let task = tokio::spawn(async move { m.run(&key, Default::default()).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    assert!(matches!(
        f.manager.run(&id, Default::default()).await,
        Err(Error::MutationBusy)
    ));
    f.manager.control(&id, NosControl::Pause).unwrap();
    gate.release.add_permits(1);
    assert_eq!(task.await.unwrap().unwrap().status, NosStatus::Paused);
    assert_eq!(
        f.manager.run(&id, Default::default()).await.unwrap().status,
        NosStatus::Paused
    );
    assert_eq!(f.backend.calls().len(), 1);
    f.manager.resume(&id).unwrap();
    f.manager.control(&id, NosControl::Cancel).unwrap();
    assert_eq!(f.manager.get(&id).unwrap().status, NosStatus::Cancelled);
    assert!(f.manager.run(&id, Default::default()).await.is_err());
    assert_eq!(fs::read(&f.source).unwrap(), DATA);
    assert_eq!(f.backend.state.lock().unwrap().data, b"abc");
}
#[tokio::test]
async fn same_account_new_generation_rejects_inflight_ack_and_old_manager_and_other_account_cannot_read()
 {
    let f = Fixture::new();
    let id = f.create().await;
    let gate = Gate::new();
    *f.backend.gate.lock().unwrap() = Some(gate.clone());
    let m = f.manager.clone();
    let key = id.clone();
    let task = tokio::spawn(async move { m.run(&key, Default::default()).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    gate.release.add_permits(1);
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert!(matches!(f.manager.get(&id), Err(Error::StaleResponse)));
    let new = NosUploads::with_backend(f.session.clone(), f.backend.clone(), 3).unwrap();
    assert!(new.get(&id).unwrap().pending);
    assert!(matches!(
        new.run(&id, Default::default()).await,
        Err(Error::MutationUncertain)
    ));
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let other = NosUploads::with_backend(f.session.clone(), f.backend.clone(), 3).unwrap();
    assert!(other.get(&id).is_err());
    assert!(other.list().unwrap().is_empty());
}
#[tokio::test]
async fn abort_releases_worker_lock_and_preserves_unknown_first_write() {
    let f = Fixture::new();
    let id = f.create().await;
    let gate = Gate::new();
    *f.backend.gate.lock().unwrap() = Some(gate.clone());
    let m = f.manager.clone();
    let key = id.clone();
    let task = tokio::spawn(async move { m.run(&key, Default::default()).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(matches!(
        f.reopened().run(&id, Default::default()).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(f.backend.calls().len(), 1);
}

#[tokio::test]
async fn renewal_on_context_query_preserves_existing_offset_and_does_not_allocate_a_new_object() {
    let f = Fixture::new();
    let id = f.create().await;
    f.manager.run(&id, budget(1)).await.unwrap();
    f.backend.state.lock().unwrap().required_token = Some("fresh".into());
    assert_eq!(
        f.reopened()
            .run(&id, Default::default())
            .await
            .unwrap()
            .status,
        NosStatus::Uploaded
    );
    let calls = f.backend.calls();
    assert!(calls[1].query && calls[2].query);
    assert_eq!(calls[3].offset, 3);
    assert_eq!(f.api.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.backend.state.lock().unwrap().data, DATA);
}
#[tokio::test]
async fn failed_intent_commit_prevents_send_and_failed_ack_commit_preserves_unknown_write() {
    for after_ack in [false, true] {
        let f = Fixture::new();
        let id = f.create().await;
        let connection =
            rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
        let condition = if after_ack {
            "json_extract(NEW.value,'$.offset')=3"
        } else {
            "json_extract(NEW.value,'$.pending') IS NOT NULL"
        };
        connection.execute_batch(&format!("CREATE TRIGGER reject_upload BEFORE INSERT ON state WHEN NEW.kind='nos_uploads_v1' AND ({condition}) BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;")).unwrap();
        assert!(matches!(
            f.manager.run(&id, Default::default()).await,
            Err(Error::Storage(_))
        ));
        assert_eq!(f.backend.calls().len(), usize::from(after_ack));
        connection
            .execute_batch("DROP TRIGGER reject_upload;")
            .unwrap();
        if after_ack {
            assert!(matches!(
                f.reopened().run(&id, Default::default()).await,
                Err(Error::MutationUncertain)
            ));
            assert_eq!(f.backend.calls().len(), 1);
        } else {
            assert_eq!(
                f.reopened()
                    .run(&id, Default::default())
                    .await
                    .unwrap()
                    .status,
                NosStatus::Uploaded
            );
        }
    }
}

#[tokio::test]
async fn a_final_ack_preserves_uploaded_truth_when_pause_or_cancel_arrives_in_flight() {
    for control in [NosControl::Pause, NosControl::Cancel] {
        let f = Fixture::new();
        let id = f.create().await;
        f.manager.run(&id, budget(2)).await.unwrap();
        let gate = Gate::new();
        *f.backend.gate.lock().unwrap() = Some(gate.clone());
        let manager = f.manager.clone();
        let task_id = id.clone();
        let task = tokio::spawn(async move { manager.run(&task_id, Default::default()).await });
        tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
            .await
            .unwrap();
        assert_eq!(f.backend.state.lock().unwrap().data, DATA);
        assert_eq!(
            f.manager.control(&id, control).unwrap().requested,
            Some(control)
        );
        gate.release.add_permits(1);
        let done = task.await.unwrap().unwrap();
        assert_eq!(done.status, NosStatus::Uploaded);
        assert_eq!(done.requested, None);
        assert!(!done.pending);
        assert!(f.manager.control(&id, control).is_err());
        assert_eq!(f.reopened().get(&id).unwrap().status, NosStatus::Uploaded);
    }
}

#[tokio::test]
async fn stop_after_confirmed_auth_rejection_prevents_renewal_and_resending_the_chunk() {
    for control in [NosControl::Pause, NosControl::Cancel] {
        let f = Fixture::new();
        let id = f.create().await;
        f.backend.fault(Fault::Http(403));
        let gate = Gate::new();
        *f.backend.gate.lock().unwrap() = Some(gate.clone());
        let manager = f.manager.clone();
        let task_id = id.clone();
        let task = tokio::spawn(async move { manager.run(&task_id, Default::default()).await });
        tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
            .await
            .unwrap();
        f.manager.control(&id, control).unwrap();
        gate.release.add_permits(1);
        let done = task.await.unwrap().unwrap();
        assert_eq!(
            done.status,
            if control == NosControl::Pause {
                NosStatus::Paused
            } else {
                NosStatus::Cancelled
            }
        );
        assert!(!done.pending);
        assert_eq!(f.backend.calls().len(), 1);
        assert!(f.backend.state.lock().unwrap().data.is_empty());
        assert_eq!(f.api.calls.load(Ordering::SeqCst), 0);
        if control == NosControl::Pause {
            f.reopened().resume(&id).unwrap();
            assert_eq!(
                f.reopened()
                    .run(&id, Default::default())
                    .await
                    .unwrap()
                    .status,
                NosStatus::Uploaded
            );
        }
    }
}
