#[path = "support/oss.rs"]
mod support;
use cumulus_core::{Error, cloud::oss::*};
use serde_json::json;
use std::{
    fs,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use support::*;
fn budget(n: usize) -> OssRunOptions {
    OssRunOptions {
        chunk_budget: Some(n),
        ..Default::default()
    }
}
#[tokio::test]
async fn small_put_checks_md5_and_available_crc_without_treating_missing_crc_as_verified() {
    for fault in [None, Some(Fault::MissingCrc), Some(Fault::ZeroCrc)] {
        let expected = fault.is_none();
        let f = Fixture::new(4096);
        let id = f.create().await;
        if let Some(fault) = fault {
            f.backend.fault(OssRequestKind::Put, None, fault);
        }
        let done = f.manager.run(&id, Default::default()).await.unwrap();
        assert_eq!(done.status, OssStatus::Uploaded);
        assert_eq!(done.bytes, 4096);
        assert_eq!(done.crc64_verified, expected);
        assert_eq!(
            f.backend.state.lock().unwrap().object.as_ref().unwrap(),
            &f.data
        );
        assert_eq!(f.backend.events().len(), 1);
        f.reopened().run(&id, Default::default()).await.unwrap();
        assert_eq!(f.backend.events().len(), 1);
        let public = serde_json::to_string(&done).unwrap();
        for secret in [
            "old-token",
            "fixture-secret",
            "owned-upload",
            "resource",
            "aliyuncs",
        ] {
            assert!(!public.contains(secret));
        }
    }
}
#[tokio::test]
async fn multipart_reopens_with_original_config_lists_pages_and_completes_exact_bytes() {
    let f = Fixture::new(5 * PART + 17);
    let id = f.create().await;
    let paused = f.manager.run(&id, budget(3)).await.unwrap();
    assert_eq!(paused.status, OssStatus::Paused);
    assert_eq!(paused.bytes, (3 * PART) as u64);
    assert_eq!(f.backend.maximum.load(Ordering::SeqCst), 3);
    f.backend.state.lock().unwrap().page_size = 2;
    let done = f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(done.status, OssStatus::Uploaded);
    assert_eq!(done.part_size, PART);
    assert_eq!(done.concurrency, 3);
    assert_eq!(done.confirmed_parts, 6);
    assert_eq!(
        f.backend.state.lock().unwrap().object.as_ref().unwrap(),
        &f.data
    );
    assert!(done.crc64_verified);
    let events = f.backend.events();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == OssRequestKind::Initiate)
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == OssRequestKind::Parts)
            .count(),
        2
    );
    for number in 1..=6 {
        assert_eq!(
            events.iter().filter(|e| e.number == Some(number)).count(),
            1
        );
    }
}
#[tokio::test]
async fn failed_batch_drains_all_three_workers_and_reconciles_only_missing_part() {
    let f = Fixture::new(4 * PART + 1);
    let id = f.create().await;
    f.backend.fault(
        OssRequestKind::Part,
        Some(2),
        Fault::Reject(503, "Unavailable"),
    );
    assert!(f.manager.run(&id, Default::default()).await.is_err());
    assert_eq!(f.backend.active.load(Ordering::SeqCst), 0);
    let saved = f.manager.get(&id).unwrap();
    assert_eq!(saved.confirmed_parts, 2);
    assert_eq!(saved.pending, Some(OssPending::Parts));
    assert!(
        !f.backend
            .events()
            .iter()
            .any(|e| e.number == Some(4) || e.kind == OssRequestKind::Complete)
    );
    assert_eq!(
        f.reopened()
            .run(&id, Default::default())
            .await
            .unwrap()
            .status,
        OssStatus::Uploaded
    );
    let events = f.backend.events();
    assert_eq!(events.iter().filter(|e| e.number == Some(2)).count(), 2);
    assert_eq!(events.iter().filter(|e| e.number == Some(1)).count(), 1);
    assert_eq!(events.iter().filter(|e| e.number == Some(3)).count(), 1);
}
#[tokio::test]
async fn lost_part_ack_is_adopted_by_listparts_but_unknown_or_modified_remote_parts_are_rejected() {
    let f = Fixture::new(3 * PART + 1);
    let id = f.create().await;
    f.backend.fault(OssRequestKind::Part, Some(2), Fault::Lost);
    assert!(f.manager.run(&id, Default::default()).await.is_err());
    f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(
        f.backend
            .events()
            .iter()
            .filter(|e| e.number == Some(2))
            .count(),
        1
    );
    for unknown in [false, true] {
        let f = Fixture::new(3 * PART + 1);
        let id = f.create().await;
        f.manager.run(&id, budget(1)).await.unwrap();
        f.backend
            .state
            .lock()
            .unwrap()
            .parts
            .insert(if unknown { 2 } else { 1 }, vec![9; PART]);
        let count = f.backend.events().len();
        assert!(matches!(
            f.reopened().run(&id, Default::default()).await,
            Err(Error::MutationUncertain)
        ));
        assert!(
            f.backend.events()[count..]
                .iter()
                .all(|e| e.kind == OssRequestKind::Parts)
        );
    }
}
#[tokio::test]
async fn lost_put_or_merge_uses_only_head_and_streamed_object_verification_without_write_replay() {
    for (size, kind) in [
        (4096, OssRequestKind::Put),
        (2 * PART + 7, OssRequestKind::Complete),
    ] {
        let f = Fixture::new(size);
        let id = f.create().await;
        f.backend.fault(kind, None, Fault::Lost);
        assert!(matches!(
            f.manager.run(&id, Default::default()).await,
            Err(Error::MutationUncertain)
        ));
        let count = f.backend.events().len();
        let done = f.reopened().run(&id, Default::default()).await.unwrap();
        assert_eq!(done.status, OssStatus::Uploaded);
        assert!(done.content_verified);
        assert!(!done.crc64_verified);
        assert_eq!(
            f.backend.events()[count..]
                .iter()
                .map(|e| e.kind)
                .collect::<Vec<_>>(),
            [OssRequestKind::Head, OssRequestKind::Verify]
        );
        assert_eq!(
            f.backend.events().iter().filter(|e| e.kind == kind).count(),
            1
        );
    }
}
#[tokio::test]
async fn uncertain_objects_require_correct_owner_length_and_content_even_with_fresh_credentials() {
    for mode in ["owner", "content", "permission"] {
        let f = Fixture::new(4096);
        let id = f.create().await;
        f.backend.fault(OssRequestKind::Put, None, Fault::Lost);
        assert!(f.manager.run(&id, Default::default()).await.is_err());
        match mode {
            "owner" => f.backend.state.lock().unwrap().marker = "foreign".into(),
            "content" => f.backend.state.lock().unwrap().object = Some(vec![0; 4096]),
            _ => f.backend.fault(
                OssRequestKind::Head,
                None,
                Fault::Reject(403, "AccessDenied"),
            ),
        };
        f.manager
            .replace_credentials(&id, allocation("fresh-token"))
            .unwrap();
        assert!(f.reopened().reconcile(&id).await.is_err());
        assert_eq!(f.manager.get(&id).unwrap().pending, Some(OssPending::Put));
        assert_eq!(
            f.backend
                .events()
                .iter()
                .filter(|e| e.kind == OssRequestKind::Put)
                .count(),
            1
        );
    }
}
#[tokio::test]
async fn initialization_lost_ack_or_task_abort_retains_unknown_id_and_never_reinitializes_or_deletes()
 {
    let f = Fixture::new(2 * PART);
    let id = f.create().await;
    f.backend.fault(OssRequestKind::Initiate, None, Fault::Lost);
    assert!(matches!(
        f.manager.run(&id, Default::default()).await,
        Err(Error::MutationUncertain)
    ));
    assert!(matches!(
        f.reopened().run(&id, Default::default()).await,
        Err(Error::MutationUncertain)
    ));
    assert!(f.manager.cancel(&id).await.is_err());
    assert_eq!(f.backend.events().len(), 1);
    let f = Fixture::new(2 * PART);
    let id = f.create().await;
    let gate = Gate::new();
    *f.backend.gate.lock().unwrap() = Some((OssRequestKind::Initiate, gate.clone()));
    let manager = f.manager.clone();
    let key = id.clone();
    let work = tokio::spawn(async move { manager.run(&key, Default::default()).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    assert!(matches!(
        f.reopened().run(&id, Default::default()).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(f.backend.events().len(), 1);
}
#[tokio::test]
async fn cancellation_aborts_only_known_multipart_and_retry_confirms_no_such_upload() {
    let f = Fixture::new(3 * PART);
    let id = f.create().await;
    f.manager.run(&id, budget(1)).await.unwrap();
    f.backend.fault(OssRequestKind::Abort, None, Fault::Lost);
    assert!(f.manager.cancel(&id).await.is_err());
    assert_eq!(f.manager.get(&id).unwrap().pending, Some(OssPending::Abort));
    assert_eq!(
        f.reopened().cancel(&id).await.unwrap().status,
        OssStatus::Cancelled
    );
    assert_eq!(fs::read(&f.source).unwrap(), f.data);
    assert!(f.backend.state.lock().unwrap().object.is_none());
    let aborts = f
        .backend
        .events()
        .into_iter()
        .filter(|e| e.kind == OssRequestKind::Abort)
        .collect::<Vec<_>>();
    assert_eq!(aborts.len(), 2);
    assert!(
        aborts
            .iter()
            .all(|e| e.url.ends_with("?uploadId=owned-upload"))
    );
}
#[tokio::test]
async fn credential_replacement_keeps_scope_and_acknowledged_rejection_can_retry_same_put() {
    let f = Fixture::new(4096);
    let id = f.create().await;
    f.backend.state.lock().unwrap().required_token = Some("fresh".into());
    assert!(f.manager.run(&id, Default::default()).await.is_err());
    assert!(f.manager.get(&id).unwrap().pending.is_none());
    let mut wrong = raw("fresh");
    wrong["objectKey"] = json!("other.flac");
    assert!(
        f.manager
            .replace_credentials(&id, OssAllocation::parse(&wrong).unwrap())
            .is_err()
    );
    f.manager
        .replace_credentials(&id, allocation("fresh"))
        .unwrap();
    assert_eq!(
        f.reopened()
            .run(&id, Default::default())
            .await
            .unwrap()
            .status,
        OssStatus::Uploaded
    );
    assert_eq!(f.backend.events().len(), 2);
    assert_eq!(
        f.backend.state.lock().unwrap().object.as_ref().unwrap(),
        &f.data
    );
}
#[tokio::test]
async fn account_relogin_rejects_inflight_acks_and_other_account_cannot_read_task() {
    let f = Fixture::new(4096);
    let id = f.create().await;
    let gate = Gate::new();
    *f.backend.gate.lock().unwrap() = Some((OssRequestKind::Put, gate.clone()));
    let manager = f.manager.clone();
    let key = id.clone();
    let work = tokio::spawn(async move { manager.run(&key, Default::default()).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    f.session
        .adopt(
            Arc::new(NoAccountApi),
            json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    gate.release.add_permits(1);
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert!(matches!(f.manager.get(&id), Err(Error::StaleResponse)));
    assert_eq!(
        f.reopened()
            .run(&id, Default::default())
            .await
            .unwrap()
            .status,
        OssStatus::Uploaded
    );
    f.session
        .adopt(
            Arc::new(NoAccountApi),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let other = OssUploads::with_backend(f.session.clone(), f.backend.clone(), PART, 3).unwrap();
    assert!(other.get(&id).is_err());
    assert!(other.list().unwrap().is_empty());
}
#[tokio::test]
async fn changed_source_and_wrong_crc_never_become_uploaded_and_failed_commits_preserve_intent() {
    let f = Fixture::new(4096);
    let id = f.create().await;
    fs::write(&f.source, b"changed").unwrap();
    assert!(f.manager.run(&id, Default::default()).await.is_err());
    assert!(f.backend.events().is_empty());
    let f = Fixture::new(4096);
    let id = f.create().await;
    f.backend.fault(OssRequestKind::Put, None, Fault::WrongCrc);
    assert!(f.manager.run(&id, Default::default()).await.is_err());
    assert_eq!(f.manager.get(&id).unwrap().status, OssStatus::Ambiguous);
    for ack in [false, true] {
        let f = Fixture::new(4096);
        let id = f.create().await;
        let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
        let condition = if ack {
            "json_extract(NEW.value,'$.status')='uploaded'"
        } else {
            "json_extract(NEW.value,'$.pending') IS NOT NULL"
        };
        db.execute_batch(&format!("CREATE TRIGGER fail_oss BEFORE INSERT ON state WHEN NEW.kind='oss_uploads_v1' AND ({condition}) BEGIN SELECT RAISE(ABORT,'fixture failure'); END;")).unwrap();
        assert!(matches!(
            f.manager.run(&id, Default::default()).await,
            Err(Error::Storage(_))
        ));
        assert_eq!(f.backend.events().len(), usize::from(ack));
        db.execute_batch("DROP TRIGGER fail_oss;").unwrap();
        if ack {
            fs::remove_file(&f.source).unwrap();
        }
        assert_eq!(
            f.reopened()
                .run(&id, Default::default())
                .await
                .unwrap()
                .status,
            OssStatus::Uploaded
        );
        assert_eq!(
            f.backend
                .events()
                .iter()
                .filter(|e| e.kind == OssRequestKind::Put)
                .count(),
            1
        );
    }
}
