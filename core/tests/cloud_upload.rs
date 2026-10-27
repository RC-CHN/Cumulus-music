#[path = "support/cloud_upload.rs"]
mod support;
use cumulus_core::{Error, cloud::upload::*};
use serde_json::json;
use std::{
    fs,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use support::*;

#[tokio::test]
async fn complete_nos_and_three_attachments_preserve_bytes_wait_for_encoding_and_confirm_account_readback()
 {
    let f = Fixture::new();
    let cover = f.root.path().join("cover.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([23, 45, 67, 255]))
        .save(&cover)
        .unwrap();
    let lyric = f.root.path().join("词.lrc");
    fs::write(&lyric, b"\xff\xfe[\0t\0i\0:\0x\0]\0").unwrap();
    let cue = f.root.path().join("disc.cue");
    fs::write(
        &cue,
        b"FILE \"input.flac\" WAVE\n TRACK 01 AUDIO\n INDEX 01 00:00:00\n",
    )
    .unwrap();
    let id = f
        .manager
        .create(
            &f.source,
            AttachmentInputs {
                cover: Some(cover.clone()),
                lyric: Some(lyric.clone()),
                cue: Some(cue.clone()),
            },
        )
        .await
        .unwrap()
        .id;
    f.api.check(true);
    f.api.allocate();
    let paused = f
        .manager
        .run(
            &id,
            CloudRunOptions {
                chunk_budget: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(paused.phase, CloudPhase::AudioUploading);
    assert_eq!(paused.bytes, 3);
    for (key, document) in [("cover", 11), ("lyric", 12), ("cue", 13)] {
        f.api
            .push(ATTACH, json!({"code":200,"data":allocation(key,document)}));
    }
    f.api.metadata(false);
    let waiting = f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(waiting.phase, CloudPhase::Encoding);
    assert_eq!(waiting.next_check_ms, 1500);
    assert!(
        waiting
            .attachments
            .iter()
            .all(|a| a.phase == AttachmentPhase::Uploaded)
    );
    for (key, path) in [("cover", cover), ("lyric", lyric), ("cue", cue)] {
        assert_eq!(
            f.objects.object(key).state.lock().unwrap().data,
            fs::read(path).unwrap()
        );
    }
    assert_eq!(f.objects.object("audio").state.lock().unwrap().data, DATA);
    let calls = f.api.calls();
    let count = calls.len();
    let meta = &calls.iter().find(|(p, _)| p == META).unwrap().1;
    assert_eq!(meta["songid"], "opaque-upload-ticket-not-a-catalog-id");
    assert_eq!(meta["objectKey"], "bucket-test/audio");
    assert_eq!(meta["bitrate"], 320000);
    assert_eq!(meta["artist"], "First artist");
    assert_eq!(
        (
            meta["coverid"].clone(),
            meta["lyricid"].clone(),
            meta["cueid"].clone()
        ),
        (json!("11"), json!("12"), json!("13"))
    );
    assert_eq!(meta["lyrictype"], "lrc");
    f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(f.api.calls().len(), count);
    f.tick(500);
    f.api.push(
        STATUS,
        json!({"code":200,"statuses":{"700":{"status":1,"waitTime":1000}}}),
    );
    let waiting = f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(waiting.next_check_ms, 61500);
    f.tick(60000);
    f.api
        .push(STATUS, json!({"code":200,"statuses":{"700":{"status":9}}}));
    f.api.published();
    f.api.listed(false);
    let published = f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(published.phase, CloudPhase::Published);
    assert!(f.manager.control(&id, UploadControl::Cancel).await.is_err());
    let count = f.api.calls().len();
    f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(f.api.calls().len(), count);
    f.tick(1000);
    f.api.listed(true);
    let done = f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(done.phase, CloudPhase::Complete);
    let public = serde_json::to_string(&done).unwrap();
    for secret in [
        "opaque-upload-ticket",
        "scoped-test-token",
        "bucket-test",
        "resourceId",
        "privateCloud",
        "docId",
    ] {
        assert!(!public.contains(secret));
    }
    assert_eq!(f.api.calls().iter().filter(|(p, _)| p == ALLOC).count(), 1);
    assert_eq!(f.api.calls().iter().filter(|(p, _)| p == META).count(), 1);
    assert_eq!(
        f.api.calls().iter().filter(|(p, _)| p == PUBLISH).count(),
        1
    );
}

#[tokio::test]
async fn deduplicated_audio_still_submits_metadata_and_publishes_without_allocating_or_sending_media()
 {
    let f = Fixture::new();
    let id = f.create().await;
    f.api.check(false);
    f.api.metadata(true);
    f.api.published();
    f.api.listed(true);
    let done = f.manager.run(&id, Default::default()).await.unwrap();
    assert_eq!(done.phase, CloudPhase::Complete);
    assert_eq!(done.bytes, DATA.len() as u64);
    assert!(done.transfer.is_none());
    assert_eq!(f.objects.calls(), 0);
    let calls = f.api.calls();
    assert_eq!(
        calls.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
        [CHECK, META, PUBLISH, LIST]
    );
    let meta = &calls[1].1;
    assert!(meta.get("resourceId").is_none());
    assert!(meta.get("objectKey").is_none());
    f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(f.api.calls().len(), 4);
}

#[tokio::test]
async fn unknown_allocation_metadata_and_publication_never_replay_and_only_known_publication_can_reconcile()
 {
    for path in [ALLOC, META, PUBLISH] {
        let f = Fixture::new();
        let id = f.create().await;
        f.api.check(path == ALLOC);
        if path == PUBLISH {
            f.api.metadata(true);
        }
        f.api.lost(path);
        assert!(f.manager.run(&id, Default::default()).await.is_err());
        let count = f.api.calls().len();
        assert_eq!(
            f.manager.get(&id).unwrap().error.unwrap().kind,
            "write_outcome_unknown"
        );
        assert!(matches!(
            f.reopened()
                .run(
                    &id,
                    CloudRunOptions {
                        retry_rejected: true,
                        ..Default::default()
                    }
                )
                .await,
            Err(Error::MutationUncertain)
        ));
        assert!(matches!(
            f.manager.control(&id, UploadControl::Cancel).await,
            Err(Error::MutationUncertain)
        ));
        assert_eq!(f.api.calls().len(), count);
        if path == PUBLISH {
            f.api.listed(false);
            assert_eq!(
                f.reopened().reconcile_publication(&id).await.unwrap().phase,
                CloudPhase::PublicationPending
            );
            f.api.listed(true);
            assert_eq!(
                f.reopened().reconcile_publication(&id).await.unwrap().phase,
                CloudPhase::Complete
            );
        } else {
            assert!(f.manager.reconcile_publication(&id).await.is_err());
        }
        assert_eq!(f.api.calls().iter().filter(|(p, _)| p == path).count(), 1);
    }
}

#[tokio::test]
async fn explicit_service_rejections_require_explicit_retry_and_never_restart_earlier_stages() {
    for path in [ALLOC, META, PUBLISH] {
        let f = Fixture::new();
        let id = f.create().await;
        f.api.check(path == ALLOC);
        if path == PUBLISH {
            f.api.metadata(true);
        }
        f.api.push(path, json!({"code":409}));
        assert!(matches!(
            f.manager.run(&id, Default::default()).await,
            Err(Error::Service { .. })
        ));
        assert_eq!(f.manager.get(&id).unwrap().phase, CloudPhase::Failed);
        let count = f.api.calls().len();
        assert!(f.reopened().run(&id, Default::default()).await.is_err());
        assert_eq!(f.api.calls().len(), count);
        if path == ALLOC {
            f.api.allocate();
        }
        if path != PUBLISH {
            f.api.metadata(true);
        }
        f.api.published();
        f.api.listed(true);
        assert_eq!(
            f.reopened()
                .run(
                    &id,
                    CloudRunOptions {
                        retry_rejected: true,
                        ..Default::default()
                    }
                )
                .await
                .unwrap()
                .phase,
            CloudPhase::Complete
        );
        assert_eq!(f.api.calls().iter().filter(|(p, _)| p == CHECK).count(), 1);
        assert_eq!(f.api.calls().iter().filter(|(p, _)| p == path).count(), 2);
    }
}

#[tokio::test]
async fn attachment_allocation_preserves_unknown_and_image_only_or_invalid_document_boundaries() {
    for failure in ["lost", "invalid_document", "image_only", "rejected"] {
        let f = Fixture::new();
        let lyric = f.root.path().join("input.lrc");
        fs::write(&lyric, b"[00:01.00]Line").unwrap();
        let id = f
            .manager
            .create(
                &f.source,
                AttachmentInputs {
                    lyric: Some(lyric),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id;
        f.api.check(false);
        match failure {
            "lost" => f.api.lost(ATTACH),
            "rejected" => f.api.push(ATTACH, json!({"code":403})),
            kind => {
                let mut a = allocation("lyric", 12);
                if kind == "invalid_document" {
                    a["docId"] = json!(0)
                } else {
                    a["bucket"] = json!("yyimgs")
                };
                f.api.push(ATTACH, json!({"code":200,"data":a}));
            }
        }
        assert!(f.manager.run(&id, Default::default()).await.is_err());
        let count = f.api.calls().len();
        assert_eq!(f.objects.calls(), 0);
        assert!(f.reopened().run(&id, Default::default()).await.is_err());
        assert_eq!(f.api.calls().len(), count);
        let view = f.manager.get(&id).unwrap();
        if failure == "image_only" {
            assert_eq!(view.attachments[0].phase, AttachmentPhase::Allocated);
            assert_eq!(
                view.attachments[0].error.as_ref().unwrap().kind,
                "image_only_bucket_for_text"
            );
        }
        if failure == "rejected" {
            f.api
                .push(ATTACH, json!({"code":200,"data":allocation("lyric",12)}));
            f.api.metadata(true);
            f.api.published();
            f.api.listed(true);
            assert_eq!(
                f.reopened()
                    .run(
                        &id,
                        CloudRunOptions {
                            retry_rejected: true,
                            ..Default::default()
                        }
                    )
                    .await
                    .unwrap()
                    .phase,
                CloudPhase::Complete
            );
        }
    }
}

#[tokio::test]
async fn incomplete_channel_three_credentials_are_retained_without_misrouting_or_reallocation() {
    let f = Fixture::new();
    let id = f.create().await;
    f.api.check(true);
    let mut a = allocation("oss", 1);
    a["channel"] = json!(3);
    f.api.push(ALLOC, json!({"code":200,"data":a}));
    assert!(f.manager.run(&id, Default::default()).await.is_err());
    let view = f.manager.get(&id).unwrap();
    assert_eq!(view.phase, CloudPhase::Allocated);
    assert_eq!(view.channel, Some(3));
    assert!(view.transfer.is_none());
    assert_eq!(f.objects.calls(), 0);
    assert!(f.reopened().run(&id, Default::default()).await.is_err());
    assert_eq!(f.api.calls().len(), 2);
}

#[tokio::test]
async fn pause_and_cancel_stop_active_child_and_preserve_sources_and_accepted_objects() {
    for action in [UploadControl::Pause, UploadControl::Cancel] {
        let f = Fixture::new();
        let id = f.create().await;
        f.api.check(true);
        f.api.allocate();
        let gate = nos::Gate::new();
        *f.objects.object("audio").gate.lock().unwrap() = Some(gate.clone());
        let manager = f.manager.clone();
        let key = id.clone();
        let worker = tokio::spawn(async move { manager.run(&key, Default::default()).await });
        tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
            .await
            .unwrap();
        assert!(matches!(
            f.manager.run(&id, Default::default()).await,
            Err(Error::MutationBusy)
        ));
        f.manager.control(&id, action).await.unwrap();
        gate.release.add_permits(1);
        worker.await.unwrap().unwrap();
        assert_eq!(f.objects.object("audio").state.lock().unwrap().data, b"abc");
        assert_eq!(fs::read(&f.source).unwrap(), DATA);
        if action == UploadControl::Pause {
            assert_eq!(
                f.reopened()
                    .run(&id, Default::default())
                    .await
                    .unwrap()
                    .requested,
                Some(action)
            );
            f.reopened().resume(&id).unwrap();
            f.api.metadata(true);
            f.api.published();
            f.api.listed(true);
            assert_eq!(
                f.reopened()
                    .run(&id, Default::default())
                    .await
                    .unwrap()
                    .phase,
                CloudPhase::Complete
            );
        } else {
            assert!(f.reopened().run(&id, Default::default()).await.is_err());
            assert_eq!(f.manager.get(&id).unwrap().phase, CloudPhase::Cancelled);
        }
    }
}

#[tokio::test]
async fn pending_mutations_reject_cancel_and_account_generation_changes_preserve_unknown_write() {
    let f = Fixture::new();
    let id = f.create().await;
    f.api.check(true);
    f.api.allocate();
    let gate = nos::Gate::new();
    *f.api.gate.lock().unwrap() = Some((ALLOC, gate.clone()));
    let manager = f.manager.clone();
    let key = id.clone();
    let worker = tokio::spawn(async move { manager.run(&key, Default::default()).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    assert!(matches!(
        f.manager.control(&id, UploadControl::Cancel).await,
        Err(Error::MutationUncertain)
    ));
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    gate.release.add_permits(1);
    assert!(matches!(worker.await.unwrap(), Err(Error::StaleResponse)));
    assert!(matches!(f.manager.get(&id), Err(Error::StaleResponse)));
    assert!(matches!(
        f.reopened().run(&id, Default::default()).await,
        Err(Error::MutationUncertain)
    ));
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let other = CloudUploads::with_adapters(
        f.session.clone(),
        UploadAdapters {
            nos: f.objects.clone(),
            oss: f.oss.clone(),
            probe: Arc::new(Probe),
            clock: f.clock.clone(),
            chunk_size: 3,
            oss_part_size: oss::PART,
            oss_concurrency: 3,
        },
    )
    .unwrap();
    assert!(other.get(&id).is_err());
    assert!(other.list().unwrap().is_empty());
    assert_eq!(f.objects.calls(), 0);
}

#[tokio::test]
async fn source_changes_bad_attachments_ncm_and_encoding_failures_stop_before_following_writes() {
    let f = Fixture::new();
    let id = f.create().await;
    fs::write(&f.source, b"different").unwrap();
    assert!(f.manager.run(&id, Default::default()).await.is_err());
    assert!(f.api.calls().is_empty());
    for (name, data) in [
        ("bad.lrc", b"krc1compressed".as_slice()),
        ("null.lrc", b"a\0b"),
        ("bad.png", b"not an image"),
        ("bad.txt", b"text"),
    ] {
        let path = f.root.path().join(name);
        fs::write(&path, data).unwrap();
        let input = if name.ends_with("png") {
            AttachmentInputs {
                cover: Some(path),
                ..Default::default()
            }
        } else {
            AttachmentInputs {
                lyric: Some(path),
                ..Default::default()
            }
        };
        assert!(f.manager.create(&f.source, input).await.is_err());
    }
    let ncm = f.root.path().join("input.ncm");
    fs::write(&ncm, DATA).unwrap();
    assert!(f.manager.create(&ncm, Default::default()).await.is_err());
    let f = Fixture::new();
    let id = f.create().await;
    f.api.check(false);
    f.api.metadata(false);
    f.manager.run(&id, Default::default()).await.unwrap();
    f.tick(500);
    f.api.push(
        STATUS,
        json!({"code":200,"statuses":{"700":{"status":-99}}}),
    );
    let failed = f.reopened().run(&id, Default::default()).await.unwrap();
    assert_eq!(failed.phase, CloudPhase::Failed);
    assert_eq!(failed.error.unwrap().code, Some(-99));
    assert!(
        f.manager
            .run(
                &id,
                CloudRunOptions {
                    retry_rejected: true,
                    ..Default::default()
                }
            )
            .await
            .is_err()
    );
    assert!(!f.api.calls().iter().any(|(p, _)| p == PUBLISH));
}

#[tokio::test]
async fn failed_parent_commit_reuses_fixed_child_without_reallocating_or_resending_finished_media()
{
    let f = Fixture::new();
    let id = f.create().await;
    f.api.check(true);
    f.api.allocate();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_parent BEFORE INSERT ON state WHEN NEW.kind='cloud_uploads_v1' AND json_extract(NEW.value,'$.phase')='audio_uploading' BEGIN SELECT RAISE(ABORT,'fixture commit failure'); END;").unwrap();
    assert!(matches!(
        f.manager.run(&id, Default::default()).await,
        Err(Error::Storage(_))
    ));
    assert_eq!(f.manager.get(&id).unwrap().phase, CloudPhase::Allocated);
    assert_eq!(f.objects.discoveries.load(Ordering::SeqCst), 1);
    db.execute_batch("DROP TRIGGER fail_parent;").unwrap();
    db.execute_batch("CREATE TRIGGER fail_parent BEFORE INSERT ON state WHEN NEW.kind='cloud_uploads_v1' AND json_extract(NEW.value,'$.phase')='audio_uploaded' BEGIN SELECT RAISE(ABORT,'fixture commit failure'); END;").unwrap();
    assert!(matches!(
        f.reopened().run(&id, Default::default()).await,
        Err(Error::Storage(_))
    ));
    assert_eq!(f.objects.discoveries.load(Ordering::SeqCst), 1);
    let requests = f.objects.calls();
    assert_eq!(f.objects.object("audio").state.lock().unwrap().data, DATA);
    db.execute_batch("DROP TRIGGER fail_parent;").unwrap();
    f.api.metadata(true);
    f.api.published();
    f.api.listed(true);
    assert_eq!(
        f.reopened()
            .run(&id, Default::default())
            .await
            .unwrap()
            .phase,
        CloudPhase::Complete
    );
    assert_eq!(f.objects.calls(), requests);
    assert_eq!(f.api.calls().iter().filter(|(p, _)| p == ALLOC).count(), 1);
}

#[tokio::test]
async fn successful_but_malformed_metadata_or_publication_cannot_be_replayed() {
    for (path, bad) in [
        (
            META,
            json!({"code":200,"songId":700,"privateCloud":{"songId":701}}),
        ),
        (PUBLISH, json!({"code":200,"privateCloud":{"songId":701}})),
    ] {
        let f = Fixture::new();
        let id = f.create().await;
        f.api.check(false);
        if path == PUBLISH {
            f.api.metadata(true);
        }
        f.api.push(path, bad);
        assert!(f.manager.run(&id, Default::default()).await.is_err());
        let count = f.api.calls().len();
        assert!(matches!(
            f.reopened().run(&id, Default::default()).await,
            Err(Error::MutationUncertain)
        ));
        assert_eq!(f.api.calls().len(), count);
    }
}

#[tokio::test]
async fn parent_write_intent_commit_failure_sends_no_mutation_and_recovers_from_previous_stage() {
    for (path, phase) in [
        (ALLOC, "allocation_pending"),
        (META, "metadata_pending"),
        (PUBLISH, "publication_pending"),
    ] {
        let f = Fixture::new();
        let id = f.create().await;
        f.api.check(path == ALLOC);
        if path == ALLOC {
            f.api.allocate();
        }
        f.api.metadata(true);
        f.api.published();
        f.api.listed(true);
        let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
        db.execute_batch(&format!("CREATE TRIGGER fail_intent BEFORE INSERT ON state WHEN NEW.kind='cloud_uploads_v1' AND json_extract(NEW.value,'$.phase')='{phase}' BEGIN SELECT RAISE(ABORT,'fixture intent failure'); END;")).unwrap();
        assert!(matches!(
            f.manager.run(&id, Default::default()).await,
            Err(Error::Storage(_))
        ));
        assert!(!f.api.calls().iter().any(|(p, _)| p == path));
        db.execute_batch("DROP TRIGGER fail_intent;").unwrap();
        assert_eq!(
            f.reopened()
                .run(&id, Default::default())
                .await
                .unwrap()
                .phase,
            CloudPhase::Complete
        );
        assert_eq!(f.api.calls().iter().filter(|(p, _)| p == path).count(), 1);
    }
}
