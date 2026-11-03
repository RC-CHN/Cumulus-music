use super::*;
fn manager(f: &Fixture) -> CloudUploads {
    CloudUploads::with_adapters(
        f.session.clone(),
        UploadAdapters {
            nos: f.objects.clone(),
            oss: f.oss.clone(),
            probe: Arc::new(support::Probe),
            clock: f.clock.clone(),
            chunk_size: 3,
            oss_part_size: support::oss::PART,
            oss_concurrency: 3,
        },
    )
    .unwrap()
}
#[tokio::test]
async fn complete_task_selection_ignores_display_page_and_filter_and_never_expands_to_later_tasks()
{
    let f = Fixture::new(0);
    let manager = manager(&f);
    let input = f.input();
    for _ in 0..205 {
        manager
            .create(&input.path, Default::default())
            .await
            .unwrap();
    }
    let client = f.client();
    let mut q = query(CloudTab::Uploading);
    q.offset = 200;
    assert_eq!(
        client
            .cloud_page("1", q, false)
            .await
            .unwrap()
            .uploads
            .len(),
        5
    );
    let captured = client.cloud_upload_selection("1", false).await.unwrap();
    assert_eq!(captured.total, 205);
    assert_eq!(captured.uploads.len(), 205);
    assert_eq!(captured.history_count, 0);
    let late = manager
        .create(&input.path, Default::default())
        .await
        .unwrap();
    let ids = captured
        .uploads
        .into_iter()
        .map(|v| v.id)
        .collect::<Vec<_>>();
    let result = client
        .cloud_command(
            "1",
            CloudCommand::RemoveUploads {
                ids: ids.clone(),
                retry: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.accepted.len(), 205);
    assert!(result.failed.is_empty());
    let removal = client.cloud_upload_removals("1", ids).await.unwrap();
    assert_eq!(removal.items.len(), 205);
    assert!(
        removal
            .items
            .iter()
            .all(|item| item.removed && item.requested)
    );
    let pending = client
        .cloud_page("1", query(CloudTab::Uploading), false)
        .await
        .unwrap();
    assert_eq!(pending.total, 1);
    assert_eq!(pending.uploads[0].id, late.id);
    assert_eq!(pending.uploading_count, 1);
    assert_eq!(pending.history_count, 0);
    assert!(f.api.calls.lock().unwrap().is_empty());
    assert_eq!(std::fs::read(&input.path).unwrap(), support::DATA);
}
#[tokio::test]
async fn completed_and_cancelled_records_leave_pending_rows_and_remain_in_separate_history() {
    let f = Fixture::new(0);
    let manager = manager(&f);
    let input = f.input();
    let cancelled = manager
        .create(&input.path, Default::default())
        .await
        .unwrap();
    manager
        .control(&cancelled.id, UploadControl::Cancel)
        .await
        .unwrap();
    let active = manager
        .create(&input.path, Default::default())
        .await
        .unwrap();
    let client = f.client();
    let page = client
        .cloud_page("1", query(CloudTab::Uploading), false)
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.uploads[0].id, active.id);
    assert_eq!(page.uploading_count, 1);
    assert_eq!(page.history_count, 1);
    let history = client
        .cloud_page("1", query(CloudTab::History), false)
        .await
        .unwrap();
    assert_eq!(history.total, 1);
    assert_eq!(history.uploads[0].id, cancelled.id);
    assert!(history.uploads[0].can_remove);
    let query_text = CloudQuery {
        keyword: "input".into(),
        ..query(CloudTab::Uploading)
    };
    assert_eq!(
        client
            .cloud_page("1", query_text, false)
            .await
            .unwrap()
            .total,
        1
    );
    let result = client
        .cloud_command(
            "1",
            CloudCommand::RemoveUploads {
                ids: vec![cancelled.id],
                retry: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.accepted.len(), 1);
    assert_eq!(
        client
            .cloud_upload_selection("1", true)
            .await
            .unwrap()
            .total,
        0
    );
    assert!(f.api.calls.lock().unwrap().is_empty());
}
#[tokio::test]
async fn task_directory_requires_recorded_unchanged_source_and_its_account_without_projecting_a_path()
 {
    let f = Fixture::new(0);
    let manager = manager(&f);
    let input = f.input();
    let task = manager
        .create(&input.path, Default::default())
        .await
        .unwrap();
    let client = f.client();
    let path = client
        .cloud_upload_directory_path("1", &task.id)
        .await
        .unwrap();
    assert_eq!(path, input.path.canonicalize().unwrap().parent().unwrap());
    let page = client.cloud_upload_selection("1", false).await.unwrap();
    let public = serde_json::to_string(&page).unwrap();
    assert!(!public.contains(&f.root.path().to_string_lossy().to_string()));
    std::fs::write(&input.path, b"different input").unwrap();
    assert!(
        client
            .cloud_upload_directory_path("1", &task.id)
            .await
            .is_err()
    );
    assert!(
        client
            .cloud_upload_removals("old", vec![task.id.clone()])
            .await
            .is_err()
    );
    assert!(
        client
            .cloud_upload_removals("1", vec![task.id.clone(), task.id])
            .await
            .is_err()
    );
    assert!(f.api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn encoding_retry_only_rechecks_the_known_song_then_publishes_once_even_if_source_is_gone() {
    let f = Fixture::new(0);
    f.api.encoding_mode.store(1, Ordering::Release);
    let client = f.client();
    let input = f.input();
    let created = client
        .cloud_command(
            "1",
            CloudCommand::Upload {
                files: vec![input.clone()],
            },
        )
        .await
        .unwrap();
    let id = created.accepted[0].clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            f.clock.0.fetch_add(1000, Ordering::AcqRel);
            let p = client
                .cloud_page("1", query(CloudTab::Uploading), false)
                .await
                .unwrap();
            if p.uploads[0].phase == CloudPhase::Failed && !p.uploads[0].running {
                assert!(p.uploads[0].can_retry_encoding);
                assert!(!p.uploads[0].can_resume);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let bulk = client
        .cloud_command(
            "1",
            CloudCommand::ResumeAll {
                retry_rejected: true,
            },
        )
        .await
        .unwrap();
    assert!(bulk.accepted.is_empty());
    let wrong = client
        .cloud_command(
            "1",
            CloudCommand::Resume {
                ids: vec![id.clone()],
                retry_rejected: true,
            },
        )
        .await
        .unwrap();
    assert!(wrong.accepted.is_empty());
    assert_eq!(wrong.failed.len(), 1);
    std::fs::remove_file(&input.path).unwrap();
    f.api.encoding_mode.store(2, Ordering::Release);
    client
        .cloud_command("1", CloudCommand::RetryEncoding { id })
        .await
        .unwrap();
    let done = wait_done(&client).await;
    assert_eq!(done.uploads.len(), 1);
    let calls = f.api.calls.lock().unwrap();
    for path in [
        support::CHECK,
        support::ALLOC,
        support::META,
        support::PUBLISH,
    ] {
        assert_eq!(
            calls.iter().filter(|(p, _)| p == path).count(),
            1,
            "must not repeat {path}"
        );
    }
    assert_eq!(
        calls.iter().filter(|(p, _)| p == support::STATUS).count(),
        2
    );
}

#[tokio::test]
async fn blocked_text_omission_is_scoped_durable_and_never_resumes_the_upload() {
    use cumulus_core::cloud::AttachmentKind;
    let f = Fixture::new(0);
    let mut input = f.input();
    let cover = f.root.path().join("cover.png");
    image::RgbaImage::from_pixel(1, 1, image::Rgba([2, 4, 6, 255]))
        .save(&cover)
        .unwrap();
    input.cover = Some(cover);
    let lyric = f.root.path().join("input.lrc");
    let cue = f.root.path().join("input.cue");
    std::fs::write(&lyric, b"[00:01.00]Line").unwrap();
    std::fs::write(&cue, b"FILE \"input.flac\" WAVE\n").unwrap();
    input.lyric = Some(lyric.clone());
    input.cue = Some(cue.clone());
    let mut client = f.client();
    let id = client
        .cloud_command("1", CloudCommand::Upload { files: vec![input] })
        .await
        .unwrap()
        .accepted
        .pop()
        .unwrap();
    let command = |attachment, expected_revision| CloudCommand::OmitTextAttachment {
        id: id.clone(),
        attachment,
        expected_revision,
    };
    for (attachment, file) in [(AttachmentKind::Lyric, lyric), (AttachmentKind::Cue, cue)] {
        // Let the background worker flush both the task and its separate
        // schedule record without competing with a tight SQLite read loop.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let choice = loop {
            let mut row = client
                .cloud_page("1", query(CloudTab::Uploading), false)
                .await
                .unwrap()
                .uploads
                .pop()
                .unwrap();
            if let Some(choice) = row.omittable_attachment.take() {
                assert!(!row.can_resume);
                break choice;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "waiting for {attachment:?} omission: phase={:?}, enabled={}, running={}, error={:?}, attachments={:?}",
                row.phase,
                row.enabled,
                row.running,
                row.error,
                row.attachments
                    .iter()
                    .map(|a| (a.kind, a.phase))
                    .collect::<Vec<_>>()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert_eq!(choice.attachment, attachment);
        let before = f.api.calls.lock().unwrap().len();
        let objects = f.objects.calls();
        assert!(matches!(
            client
                .cloud_command("1", command(AttachmentKind::Cover, choice.revision.clone()))
                .await,
            Err(Error::StaleResponse)
        ));
        assert!(matches!(
            client
                .cloud_command("1", command(attachment, "0".into()))
                .await,
            Err(Error::StaleResponse)
        ));
        let resume = || CloudCommand::Resume {
            ids: vec![id.clone()],
            retry_rejected: true,
        };
        assert_eq!(
            client
                .cloud_command("1", resume())
                .await
                .unwrap()
                .failed
                .len(),
            1
        );
        assert_eq!(
            client
                .cloud_command("1", command(attachment, choice.revision.clone()))
                .await
                .unwrap()
                .accepted,
            vec![id.clone()]
        );
        assert!(matches!(
            client
                .cloud_command("1", command(attachment, choice.revision))
                .await,
            Err(Error::StaleResponse)
        ));
        assert!(file.exists());
        std::fs::remove_file(file).unwrap();
        drop(client);
        client = f.client();
        let mut page = client
            .cloud_page("1", query(CloudTab::Uploading), false)
            .await
            .unwrap();
        let row = page.uploads.pop().unwrap();
        assert!(row.can_resume);
        assert!(!row.enabled && !row.running && row.omittable_attachment.is_none());
        assert_eq!(
            row.attachments
                .iter()
                .find(|a| a.kind == attachment)
                .unwrap()
                .phase,
            AttachmentPhase::Omitted
        );
        assert_eq!(row.attachments[0].phase, AttachmentPhase::Uploaded);
        assert_eq!(f.api.calls.lock().unwrap().len(), before);
        assert_eq!(f.objects.calls(), objects);
        assert_eq!(
            client.cloud_command("1", resume()).await.unwrap().accepted,
            vec![id.clone()]
        );
    }
    let _ = wait_done(&client).await;
    let calls = f.api.calls.lock().unwrap();
    for path in [
        support::CHECK,
        support::ALLOC,
        support::META,
        support::PUBLISH,
    ] {
        assert_eq!(calls.iter().filter(|(p, _)| p == path).count(), 1, "{path}");
    }
    assert_eq!(
        calls.iter().filter(|(p, _)| p == support::ATTACH).count(),
        3
    );
    let (_, payload) = calls.iter().find(|(p, _)| p == support::META).unwrap();
    assert_eq!(payload["coverid"], "11");
    assert!(payload.get("lyricid").is_none());
    assert!(payload.get("lyrictype").is_none());
    assert!(payload.get("cueid").is_none());
}

#[tokio::test]
async fn unresolved_attachment_allocation_cannot_be_omitted() {
    use cumulus_core::cloud::AttachmentKind;
    let f = Fixture::new(0);
    f.api.text_attachment_mode.store(2, Ordering::SeqCst);
    let mut input = f.input();
    let lyric = f.root.path().join("input.lrc");
    std::fs::write(&lyric, b"[00:01.00]Line").unwrap();
    input.lyric = Some(lyric);
    let client = f.client();
    let id = client
        .cloud_command("1", CloudCommand::Upload { files: vec![input] })
        .await
        .unwrap()
        .accepted
        .pop()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let row = client
                .cloud_page("1", query(CloudTab::Uploading), false)
                .await
                .unwrap()
                .uploads
                .pop()
                .unwrap();
            if !row.running && row.attachments[0].phase == AttachmentPhase::AllocationPending {
                assert!(row.omittable_attachment.is_none());
                assert!(!row.can_resume);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        client
            .cloud_command(
                "1",
                CloudCommand::OmitTextAttachment {
                    id,
                    attachment: AttachmentKind::Lyric,
                    expected_revision: "1".into()
                }
            )
            .await,
        Err(Error::MutationUncertain)
    ));
    assert!(
        f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|(p, _)| p != support::META)
    );
}

async fn stopped_upload(client: &MusicClient, error: &str) -> CloudUploadView {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let page = client
                .cloud_page("1", query(CloudTab::Uploading), false)
                .await
                .unwrap();
            if let Some(row) = page
                .uploads
                .into_iter()
                .find(|row| !row.enabled && !row.running && row.error.as_deref() == Some(error))
            {
                return row;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn lost_nos_boundary_is_still_unresolved_after_reopen_and_can_only_stop_locally() {
    for (key, lost_chunk) in [("audio", 0), ("lyric", 2)] {
        let f = Fixture::new(0);
        let mut input = f.input();
        if key == "lyric" {
            f.api.text_attachment_mode.store(1, Ordering::SeqCst);
            let lyric = f.root.path().join("fixture.lrc");
            std::fs::write(&lyric, b"[00:01]x\n").unwrap();
            input.lyric = Some(lyric);
        }
        let object = f.objects.object(key);
        object
            .state
            .lock()
            .unwrap()
            .faults
            .extend((0..lost_chunk).map(|_| None));
        object.fault(support::nos::Fault::LostReply);
        let gate = support::nos::Gate::new();
        *object.gate.lock().unwrap() = Some(gate.clone());
        let client = f.client();
        let id = client
            .cloud_command("1", CloudCommand::Upload { files: vec![input] })
            .await
            .unwrap()
            .accepted
            .pop()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
            .await
            .unwrap();
        let page = client
            .cloud_page("1", query(CloudTab::Uploading), false)
            .await
            .unwrap();
        assert!(page.uploads[0].running);
        assert_ne!(page.uploads[0].label, "无法核对上传结果");
        gate.release.add_permits(1);
        let row = stopped_upload(&client, "nos_boundary_unverified").await;
        assert_eq!(row.id, id);
        assert_eq!(row.label, "无法核对上传结果");
        assert_eq!(object.calls().len(), lost_chunk + 1);
        assert_eq!(object.calls().last().unwrap().complete, key == "lyric");
        let calls = f.api.calls.lock().unwrap().clone();
        let object_calls = f.objects.calls();
        drop(client);

        let client = f.client();
        let row = stopped_upload(&client, "nos_boundary_unverified").await;
        assert!(!row.can_resume && !row.can_renew && !row.can_reconcile);
        assert!(row.can_cancel);
        if key == "lyric" {
            assert_eq!(
                row.attachments[0].transfer,
                Some(cumulus_core::cloud::nos::NosStatus::Paused)
            );
        }
        assert!(matches!(
            manager(&f).resume(&id),
            Err(Error::MutationUncertain)
        ));
        let result = client
            .cloud_command(
                "1",
                CloudCommand::Resume {
                    ids: vec![id.clone()],
                    retry_rejected: true,
                },
            )
            .await
            .unwrap();
        assert!(result.accepted.is_empty());
        assert_eq!(result.failed.len(), 1);
        let bulk = client
            .cloud_command(
                "1",
                CloudCommand::ResumeAll {
                    retry_rejected: true,
                },
            )
            .await
            .unwrap();
        assert!(bulk.accepted.is_empty() && bulk.failed.is_empty());
        assert!(matches!(
            client
                .cloud_command("1", CloudCommand::Renew { id: id.clone() })
                .await,
            Err(Error::MutationUncertain)
        ));
        assert_eq!(
            client
                .cloud_command("1", CloudCommand::Cancel { ids: vec![id] })
                .await
                .unwrap()
                .accepted
                .len(),
            1
        );
        let history = client
            .cloud_page("1", query(CloudTab::History), false)
            .await
            .unwrap();
        assert_eq!(history.uploads[0].phase, CloudPhase::Cancelled);
        assert!(history.uploads[0].error.is_none());
        assert_eq!(*f.api.calls.lock().unwrap(), calls);
        assert_eq!(f.objects.calls(), object_calls);
    }
}

#[tokio::test]
async fn lost_middle_nos_ack_still_queries_offset_before_explicit_resume() {
    let f = Fixture::new(0);
    let object = f.objects.object("audio");
    object.state.lock().unwrap().faults.push_back(None);
    object.fault(support::nos::Fault::LostReply);
    let client = f.client();
    let id = client
        .cloud_command(
            "1",
            CloudCommand::Upload {
                files: vec![f.input()],
            },
        )
        .await
        .unwrap()
        .accepted
        .pop()
        .unwrap();
    let row = stopped_upload(&client, "network").await;
    assert!(row.can_resume);
    assert_eq!(object.calls().len(), 2);
    assert_eq!(
        client
            .cloud_command(
                "1",
                CloudCommand::Resume {
                    ids: vec![id],
                    retry_rejected: false
                }
            )
            .await
            .unwrap()
            .accepted
            .len(),
        1
    );
    wait_done(&client).await;
    let calls = object.calls();
    assert_eq!(calls.len(), 4);
    assert!(calls[2].query);
    assert!(!calls[3].query);
    assert_eq!(calls[3].offset, 6);
    assert_eq!(object.state.lock().unwrap().data, support::DATA);
}
