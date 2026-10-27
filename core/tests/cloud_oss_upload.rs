#[path = "support/cloud_upload.rs"]
mod support;
use cumulus_core::{
    Error,
    cloud::{oss::*, upload::*},
};
use serde_json::json;
use std::{fs, sync::atomic::Ordering, time::Duration};
use support::*;

fn allocate(f: &Fixture) {
    f.api.check(true);
    f.api
        .push(ALLOC, json!({"code":200,"data":oss::raw("old-token")}));
}
fn finish(f: &Fixture) {
    f.api.metadata(true);
    f.api.published();
    f.api.listed(true);
}
fn media(f: &Fixture, size: usize) -> Vec<u8> {
    let data = (0..size).map(|i| (i % 251) as u8).collect::<Vec<_>>();
    fs::write(&f.source, &data).unwrap();
    data
}
fn oss_view(view: &CloudView) -> &OssView {
    match view.transfer.as_ref().unwrap() {
        CloudTransfer::Oss(v) => v,
        _ => panic!("server selected OSS, not NOS"),
    }
}
fn account_writes(f: &Fixture) -> Vec<String> {
    f.api
        .calls()
        .into_iter()
        .filter(|(p, _)| [META, PUBLISH].contains(&p.as_str()))
        .map(|(p, _)| p)
        .collect()
}
async fn entered(gate: &oss::Gate) {
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
}

#[tokio::test]
async fn server_selected_oss_resumes_multipart_then_nos_attachment_metadata_and_account_publication()
 {
    let f = Fixture::new();
    let data = media(&f, 5 * oss::PART + 17);
    let cover = f.root.path().join("cover.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([23, 45, 67, 255]))
        .save(&cover)
        .unwrap();
    let id = f
        .manager
        .create(
            &f.source,
            AttachmentInputs {
                cover: Some(cover.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
    allocate(&f);
    let partial = f
        .manager
        .run(
            &id,
            CloudRunOptions {
                chunk_budget: Some(3),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(partial.phase, CloudPhase::AudioUploading);
    assert_eq!(partial.channel, Some(3));
    assert_eq!(partial.bytes, (3 * oss::PART) as u64);
    assert_eq!(oss_view(&partial).status, OssStatus::Paused);
    assert_eq!(f.objects.calls(), 0);
    f.oss.state.lock().unwrap().page_size = 2;
    f.api
        .push(ATTACH, json!({"code":200,"data":allocation("cover",11)}));
    finish(&f);
    let done = f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(done.phase, CloudPhase::Complete);
    assert!(oss_view(&done).crc64_verified);
    assert_eq!(oss_view(&done).part_size, oss::PART);
    assert_eq!(oss_view(&done).concurrency, 3);
    assert_eq!(f.oss.state.lock().unwrap().object.as_ref(), Some(&data));
    assert_eq!(
        f.objects.object("cover").state.lock().unwrap().data,
        fs::read(cover).unwrap()
    );
    assert_eq!(f.objects.discoveries.load(Ordering::SeqCst), 1);
    let calls = f.api.calls();
    let meta = &calls.iter().find(|(p, _)| p == META).unwrap().1;
    assert_eq!(meta["objectKey"], "fixture-bucket/audio.flac");
    assert_eq!(meta["resourceId"], "resource");
    assert_eq!(meta["coverid"], "11");
    assert_eq!(meta["songid"], "opaque-upload-ticket-not-a-catalog-id");
    assert_eq!(calls.iter().filter(|(p, _)| p == ALLOC).count(), 1);
    let count = f.oss.events().len();
    f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(f.oss.events().len(), count);
    let public = serde_json::to_string(&done).unwrap();
    for secret in [
        "fixture-secret",
        "fixture-access",
        "old-token",
        "aliyuncs",
        "fixture-bucket",
        "owned-upload",
        "opaque-upload-ticket",
    ] {
        assert!(!public.contains(secret));
    }
}

#[tokio::test]
async fn lost_put_and_merge_require_readonly_transfer_reconciliation_before_publication() {
    for (size, kind) in [
        (9, OssRequestKind::Put),
        (4 * oss::PART, OssRequestKind::Complete),
    ] {
        let f = Fixture::new();
        let data = media(&f, size);
        let id = f.create().await;
        allocate(&f);
        f.oss.fault(kind, None, oss::Fault::Lost);
        assert!(matches!(
            f.manager.run(&id, Default::default()).await,
            Err(Error::MutationUncertain)
        ));
        assert!(account_writes(&f).is_empty());
        let count = f.oss.events().len();
        fs::remove_file(&f.source).unwrap();
        let reconciled = f.reopened().reconcile_transfer(&id).await.unwrap();
        assert_eq!(reconciled.phase, CloudPhase::AudioUploaded);
        assert!(oss_view(&reconciled).content_verified);
        assert!(!oss_view(&reconciled).crc64_verified);
        assert_eq!(
            f.oss.events()[count..]
                .iter()
                .map(|e| e.kind)
                .collect::<Vec<_>>(),
            [OssRequestKind::Head, OssRequestKind::Verify]
        );
        assert!(account_writes(&f).is_empty());
        assert!(f.reopened().run(&id, Default::default()).await.is_err());
        fs::write(&f.source, data).unwrap();
        finish(&f);
        assert_eq!(
            f.reopened()
                .run(&id, Default::default())
                .await
                .unwrap()
                .phase,
            CloudPhase::Complete
        );
        assert_eq!(f.oss.events().iter().filter(|e| e.kind == kind).count(), 1);
    }
}

#[tokio::test]
async fn cancellation_waits_for_abort_confirmation_and_unknown_initiation_cannot_claim_success() {
    let f = Fixture::new();
    let data = media(&f, 4 * oss::PART);
    let id = f.create().await;
    allocate(&f);
    f.manager
        .run(
            &id,
            CloudRunOptions {
                chunk_budget: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    f.oss.fault(OssRequestKind::Abort, None, oss::Fault::Lost);
    assert!(f.manager.control(&id, UploadControl::Cancel).await.is_err());
    let pending = f.reopened().get(&id).unwrap();
    assert_eq!(pending.phase, CloudPhase::AudioUploading);
    assert_eq!(pending.requested, Some(UploadControl::Cancel));
    assert_eq!(oss_view(&pending).pending, Some(OssPending::Abort));
    assert!(f.reopened().resume(&id).is_err());
    let cancelled = f
        .reopened()
        .control(&id, UploadControl::Cancel)
        .await
        .unwrap();
    assert_eq!(cancelled.phase, CloudPhase::Cancelled);
    assert_eq!(oss_view(&cancelled).status, OssStatus::Cancelled);
    assert_eq!(fs::read(&f.source).unwrap(), data);
    assert!(account_writes(&f).is_empty());
    assert!(
        f.oss
            .events()
            .iter()
            .filter(|e| e.kind == OssRequestKind::Abort)
            .all(|e| e.url.contains("uploadId="))
    );

    let f = Fixture::new();
    media(&f, 2 * oss::PART);
    let id = f.create().await;
    allocate(&f);
    f.oss
        .fault(OssRequestKind::Initiate, None, oss::Fault::Lost);
    assert!(matches!(
        f.manager.run(&id, Default::default()).await,
        Err(Error::MutationUncertain)
    ));
    for manager in [f.manager.clone(), f.reopened()] {
        assert!(matches!(
            manager.control(&id, UploadControl::Cancel).await,
            Err(Error::MutationUncertain)
        ));
        assert_eq!(manager.get(&id).unwrap().phase, CloudPhase::AudioUploading);
    }
    assert_eq!(f.oss.events().len(), 1);
    assert!(account_writes(&f).is_empty());
}

#[tokio::test]
async fn parent_controls_drain_active_parts_before_pause_or_abort_and_resume_without_duplicate_parts()
 {
    for action in [UploadControl::Pause, UploadControl::Cancel] {
        let f = Fixture::new();
        media(&f, 5 * oss::PART + 17);
        let id = f.create().await;
        allocate(&f);
        let gate = oss::Gate::new();
        *f.oss.gate.lock().unwrap() = Some((OssRequestKind::Part, gate.clone()));
        let manager = f.manager.clone();
        let key = id.clone();
        let worker = tokio::spawn(async move { manager.run(&key, Default::default()).await });
        entered(&gate).await;
        assert!(matches!(
            f.manager.run(&id, Default::default()).await,
            Err(Error::MutationBusy)
        ));
        let requested = f.manager.control(&id, action).await.unwrap();
        assert_eq!(requested.phase, CloudPhase::AudioUploading);
        assert_eq!(requested.requested, Some(action));
        gate.release.add_permits(1);
        let stopped = worker.await.unwrap().unwrap();
        assert_eq!(f.oss.active.load(Ordering::SeqCst), 0);
        assert_eq!(
            f.oss
                .events()
                .iter()
                .filter(|e| e.kind == OssRequestKind::Part)
                .count(),
            3
        );
        assert!(
            !f.oss
                .events()
                .iter()
                .any(|e| e.kind == OssRequestKind::Complete)
        );
        assert!(account_writes(&f).is_empty());
        if action == UploadControl::Pause {
            assert_eq!(oss_view(&stopped).status, OssStatus::Paused);
            assert_eq!(oss_view(&stopped).confirmed_parts, 3);
            let manager = f.reopened();
            manager.resume(&id).unwrap();
            finish(&f);
            assert_eq!(
                manager.run(&id, Default::default()).await.unwrap().phase,
                CloudPhase::Complete
            );
            assert_eq!(
                f.oss
                    .events()
                    .iter()
                    .filter(|e| e.kind == OssRequestKind::Part)
                    .count(),
                6
            );
        } else {
            assert_eq!(stopped.phase, CloudPhase::Cancelled);
            assert_eq!(oss_view(&stopped).status, OssStatus::Cancelled);
            assert!(!f.oss.state.lock().unwrap().active_upload);
        }
    }
}

#[tokio::test]
async fn cancelling_late_put_or_merge_keeps_accepted_object_and_prevents_metadata_and_publication()
{
    for (size, kind) in [
        (9, OssRequestKind::Put),
        (2 * oss::PART, OssRequestKind::Complete),
    ] {
        let f = Fixture::new();
        let data = media(&f, size);
        let id = f.create().await;
        allocate(&f);
        let gate = oss::Gate::new();
        *f.oss.gate.lock().unwrap() = Some((kind, gate.clone()));
        let manager = f.manager.clone();
        let key = id.clone();
        let worker = tokio::spawn(async move { manager.run(&key, Default::default()).await });
        entered(&gate).await;
        f.manager.control(&id, UploadControl::Cancel).await.unwrap();
        gate.release.add_permits(1);
        let stopped = worker.await.unwrap().unwrap();
        assert_eq!(stopped.phase, CloudPhase::Cancelled);
        assert_eq!(oss_view(&stopped).status, OssStatus::Uploaded);
        assert_eq!(f.oss.state.lock().unwrap().object.as_ref(), Some(&data));
        assert!(
            !f.oss
                .events()
                .iter()
                .any(|e| e.kind == OssRequestKind::Abort)
        );
        assert!(account_writes(&f).is_empty());
    }
}

#[tokio::test]
async fn source_changed_during_parts_cannot_merge_or_publish_but_original_can_resume() {
    let f = Fixture::new();
    let data = media(&f, 3 * oss::PART);
    let id = f.create().await;
    allocate(&f);
    let gate = oss::Gate::new();
    *f.oss.gate.lock().unwrap() = Some((OssRequestKind::Part, gate.clone()));
    let manager = f.manager.clone();
    let key = id.clone();
    let worker = tokio::spawn(async move { manager.run(&key, Default::default()).await });
    entered(&gate).await;
    fs::write(&f.source, vec![42; data.len()]).unwrap();
    gate.release.add_permits(1);
    assert!(worker.await.unwrap().is_err());
    assert!(
        !f.oss
            .events()
            .iter()
            .any(|e| e.kind == OssRequestKind::Complete)
    );
    assert!(account_writes(&f).is_empty());
    fs::write(&f.source, &data).unwrap();
    finish(&f);
    assert_eq!(
        f.reopened()
            .run(&id, Default::default())
            .await
            .unwrap()
            .phase,
        CloudPhase::Complete
    );
    assert_eq!(
        f.oss
            .events()
            .iter()
            .filter(|e| e.kind == OssRequestKind::Part)
            .count(),
        3
    );
}

#[tokio::test]
async fn sts_replacement_is_same_object_only_and_never_uses_nos_refresh_or_reallocation() {
    let f = Fixture::new();
    media(&f, 4 * oss::PART);
    let id = f.create().await;
    allocate(&f);
    f.manager
        .run(
            &id,
            CloudRunOptions {
                chunk_budget: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    f.oss.state.lock().unwrap().required_token = Some("fresh-token".into());
    let manager = f.reopened();
    assert!(manager.run(&id, Default::default()).await.is_err());
    assert!(manager.renew_token(&id).await.is_err());
    let mut wrong = oss::raw("fresh-token");
    wrong["objectKey"] = json!("another.flac");
    assert!(
        manager
            .replace_oss_credentials(&id, OssAllocation::parse(&wrong).unwrap())
            .is_err()
    );
    manager
        .replace_oss_credentials(&id, oss::allocation("fresh-token"))
        .unwrap();
    finish(&f);
    assert_eq!(
        manager.run(&id, Default::default()).await.unwrap().phase,
        CloudPhase::Complete
    );
    assert_eq!(f.api.calls().iter().filter(|(p, _)| p == ALLOC).count(), 1);
    assert!(!f.api.calls().iter().any(|(p, _)| p.contains("refresh")));
    assert_eq!(f.objects.calls(), 0);
    assert_eq!(
        f.oss
            .events()
            .iter()
            .filter(|e| e.kind == OssRequestKind::Part && e.number == Some(1))
            .count(),
        1
    );
}

#[tokio::test]
async fn failed_parent_commits_reuse_original_oss_child_and_do_not_repeat_upload_or_allocation() {
    let f = Fixture::new();
    let id = f.create().await;
    allocate(&f);
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    for (phase, expected) in [
        ("audio_uploading", CloudPhase::Allocated),
        ("audio_uploaded", CloudPhase::AudioUploading),
    ] {
        db.execute_batch(&format!("CREATE TRIGGER fail_parent BEFORE INSERT ON state WHEN NEW.kind='cloud_uploads_v1' AND json_extract(NEW.value,'$.phase')='{phase}' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;")).unwrap();
        assert!(matches!(
            f.reopened().run(&id, Default::default()).await,
            Err(Error::Storage(_))
        ));
        assert_eq!(f.manager.get(&id).unwrap().phase, expected);
        db.execute_batch("DROP TRIGGER fail_parent;").unwrap();
    }
    assert_eq!(
        f.oss
            .events()
            .iter()
            .filter(|e| e.kind == OssRequestKind::Put)
            .count(),
        1
    );
    finish(&f);
    assert_eq!(
        f.reopened()
            .run(&id, Default::default())
            .await
            .unwrap()
            .phase,
        CloudPhase::Complete
    );
    assert_eq!(f.oss.events().len(), 1);
    assert_eq!(f.api.calls().iter().filter(|(p, _)| p == ALLOC).count(), 1);
}
