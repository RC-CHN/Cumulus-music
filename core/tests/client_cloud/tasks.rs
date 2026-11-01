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
