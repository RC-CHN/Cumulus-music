use super::*;
const REQUEST: &str = "0123456789abcdef0123456789abcdef";
fn planned(input: CloudUploadInput) -> CloudCommand {
    CloudCommand::UploadPlanned {
        files: vec![PlannedCloudUploadInput {
            request_id: REQUEST.into(),
            input,
        }],
    }
}
#[tokio::test]
async fn schedule_failure_returns_a_durable_registration_receipt_and_replay_never_starts_it() {
    let f = Fixture::new(0);
    let client = f.client();
    client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let registry = ProfileRegistry::open(f.root.path(), ApiOrigin::Pc).unwrap();
    let db = rusqlite::Connection::open(registry.store().directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_cloud_schedule BEFORE INSERT ON state WHEN NEW.kind='cloud_schedule_v1' BEGIN SELECT RAISE(ABORT,'synthetic schedule failure'); END;").unwrap();
    let input = f.input();
    let receipt = client
        .cloud_command("1", planned(input.clone()))
        .await
        .unwrap();
    assert_eq!(receipt.accepted.len(), 1);
    assert!(receipt.failed.is_empty());
    assert_eq!(receipt.not_started.len(), 1);
    assert_eq!(receipt.not_started[0].id, input.path.to_string_lossy());
    db.execute_batch("DROP TRIGGER fail_cloud_schedule")
        .unwrap();
    let again = client.cloud_command("1", planned(input)).await.unwrap();
    assert_eq!(again.accepted, receipt.accepted);
    assert!(again.failed.is_empty());
    assert_eq!(again.not_started.len(), 1);
    assert!(again.not_started[0].message.contains("保留原状态"));
    let page = client
        .cloud_page("1", query(CloudTab::Uploading), false)
        .await
        .unwrap();
    assert_eq!(page.uploads.len(), 1);
    assert!(!page.uploads[0].enabled);
    assert!(!page.uploads[0].running);
    assert!(page.uploads[0].can_resume);
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path == support::CHECK)
    );
}
#[tokio::test]
async fn receipt_replay_recovers_paused_tasks_even_when_the_account_capacity_is_now_full() {
    let f = Fixture::new(0);
    let client = f.client();
    let input = f.input();
    let gate = Arc::new(support::nos::Gate {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Semaphore::new(0),
    });
    *f.api.allocation_gate.lock().unwrap() = Some(gate.clone());
    let receipt = client
        .cloud_command("1", planned(input.clone()))
        .await
        .unwrap();
    gate.entered.notified().await;
    client
        .cloud_command(
            "1",
            CloudCommand::Pause {
                ids: receipt.accepted.clone(),
            },
        )
        .await
        .unwrap();
    gate.release.add_permits(1);
    for _ in 0..100 {
        let page = client
            .cloud_page("1", query(CloudTab::Uploading), false)
            .await
            .unwrap();
        if !page.uploads[0].running {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    f.api.full.store(true, Ordering::SeqCst);
    client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let before = f
        .api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path == support::CHECK || path == support::ALLOC)
        .count();
    std::fs::remove_file(&input.path).unwrap();
    let again = client
        .cloud_command("1", planned(input.clone()))
        .await
        .unwrap();
    assert_eq!(again.accepted, receipt.accepted);
    assert!(again.failed.is_empty());
    assert_eq!(again.not_started.len(), 1);
    assert!(again.not_started[0].message.contains("保留原状态"));
    let page = client
        .cloud_page("1", query(CloudTab::Uploading), false)
        .await
        .unwrap();
    assert_eq!(page.uploads.len(), 1);
    assert!(!page.uploads[0].enabled);
    assert!(!page.uploads[0].running);
    let after = f
        .api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path == support::CHECK || path == support::ALLOC)
        .count();
    assert_eq!(before, after);
    let new_request = CloudCommand::UploadPlanned {
        files: vec![PlannedCloudUploadInput {
            request_id: "a".repeat(32),
            input,
        }],
    };
    let refused = client.cloud_command("1", new_request).await.unwrap();
    assert!(refused.accepted.is_empty());
    assert_eq!(refused.failed.len(), 1);
    assert_eq!(
        client
            .cloud_page("1", query(CloudTab::Uploading), false)
            .await
            .unwrap()
            .uploads
            .len(),
        1
    );
}
#[tokio::test]
async fn planned_inputs_validate_every_request_id_before_registering_any_file() {
    let f = Fixture::new(0);
    let client = f.client();
    let input = f.input();
    let invalid = CloudCommand::UploadPlanned {
        files: vec![
            PlannedCloudUploadInput {
                request_id: REQUEST.into(),
                input: input.clone(),
            },
            PlannedCloudUploadInput {
                request_id: REQUEST.to_uppercase(),
                input,
            },
        ],
    };
    assert!(client.cloud_command("1", invalid).await.is_err());
    assert!(
        client
            .cloud_page("1", query(CloudTab::Uploading), false)
            .await
            .unwrap()
            .uploads
            .is_empty()
    );
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path == support::CHECK)
    );
}
#[tokio::test]
async fn invalid_attachments_return_specific_bounded_feedback_before_any_registration_or_api_write()
{
    let f = Fixture::new(0);
    let client = f.client();
    let invalid_cover = f.root.path().join("invalid.png");
    let invalid_text = f.root.path().join("invalid.lrc");
    std::fs::write(&invalid_cover, b"not an image - secret fixture content").unwrap();
    std::fs::write(&invalid_text, b"krc1secret fixture content").unwrap();
    let unsupported_cover = f.root.path().join("invalid.bmp");
    std::fs::write(&unsupported_cover, b"synthetic image").unwrap();
    for (cover, lyric, expected) in [
        (Some(invalid_cover.clone()), None, "JPG"),
        (None, Some(invalid_text.clone()), "文本"),
        (Some(unsupported_cover), None, "附件格式"),
    ] {
        let mut input = f.input();
        input.cover = cover;
        input.lyric = lyric;
        let receipt = client.cloud_command("1", planned(input)).await.unwrap();
        assert!(receipt.accepted.is_empty());
        assert_eq!(receipt.failed.len(), 1);
        assert!(receipt.failed[0].message.contains(expected));
        assert!(!receipt.failed[0].message.contains("secret"));
    }
    assert!(
        client
            .cloud_page("1", query(CloudTab::Uploading), false)
            .await
            .unwrap()
            .uploads
            .is_empty()
    );
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path == support::CHECK || path == support::ATTACH)
    );
}
