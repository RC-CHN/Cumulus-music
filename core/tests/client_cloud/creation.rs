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

#[tokio::test]
async fn oversized_files_are_reported_as_partial_or_no_new_tasks_without_a_cloud_call() {
    let f = Fixture::new(0);
    let client = f.client();
    let normal = f.input();
    let mut oversized = normal.clone();
    oversized.path.set_file_name("large.flac");
    std::fs::File::create(&oversized.path)
        .unwrap()
        .set_len(100 * 1024 * 1024 + 1)
        .unwrap();
    let db = rusqlite::Connection::open(
        ProfileRegistry::open(f.root.path(), ApiOrigin::Pc)
            .unwrap()
            .store()
            .directory()
            .join("core.sqlite3"),
    )
    .unwrap();
    db.execute_batch("CREATE TRIGGER fail_cloud_schedule BEFORE INSERT ON state WHEN NEW.kind='cloud_schedule_v1' BEGIN SELECT RAISE(ABORT,'synthetic schedule failure'); END;").unwrap();
    let batch = || CloudCommand::UploadPlanned {
        files: [normal.clone(), oversized.clone()]
            .into_iter()
            .enumerate()
            .map(|(index, input)| PlannedCloudUploadInput {
                input,
                request_id: format!("{index:032x}"),
            })
            .collect(),
    };
    let result = client
        .cloud_command("1", planned(oversized.clone()))
        .await
        .unwrap();
    assert!(result.accepted.is_empty());
    assert_eq!(result.upload_limit, Some(CloudUploadLimit::NoNewTasks));
    assert_eq!(result.failed[0].id, oversized.path.to_string_lossy());
    assert_eq!(result.failed[0].message, "文件超过100M，无法上传");
    let mixed = client.cloud_command("1", batch()).await.unwrap();
    assert_eq!(mixed.accepted.len(), 1);
    assert_eq!(mixed.not_started.len(), 1);
    assert_eq!(mixed.failed.len(), 1);
    assert_eq!(mixed.failed[0].id, oversized.path.to_string_lossy());
    assert_eq!(mixed.upload_limit, Some(CloudUploadLimit::Partial));
    let replay = client.cloud_command("1", batch()).await.unwrap();
    assert_eq!(replay.accepted, mixed.accepted);
    assert_eq!(replay.upload_limit, Some(CloudUploadLimit::NoNewTasks));
    assert_eq!(
        client
            .cloud_page("1", query(CloudTab::Uploading), false)
            .await
            .unwrap()
            .uploads
            .len(),
        1
    );
    assert!(f.api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn legacy_oversized_task_precedes_new_admission_but_a_replacement_at_its_path_does_not() {
    use std::io::{Seek, SeekFrom, Write};
    const BYTES: u64 = 100 * 1024 * 1024 + 1;
    let f = Fixture::new(0);
    let client = f.client();
    let input = f.input();
    let store = ProfileRegistry::open(f.root.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let db = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_cloud_schedule BEFORE INSERT ON state WHEN NEW.kind='cloud_schedule_v1' BEGIN SELECT RAISE(ABORT,'synthetic schedule failure'); END;").unwrap();
    let original = client
        .cloud_command("1", planned(input.clone()))
        .await
        .unwrap();
    let old_id = &original.accepted[0];
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&input.path)
        .unwrap();
    file.set_len(BYTES).unwrap();
    // Legacy row shape from before the admission cap. Known hashes describe
    // the original nine bytes followed by zero-filled sparse bytes to BYTES.
    db.execute(
        "UPDATE state SET value=json_set(value,'$.source.bytes',?1,'$.source.md5',?2,'$.source.sha256',?3) WHERE owner='1' AND kind='cloud_uploads_v1' AND key=?4",
        rusqlite::params![i64::try_from(BYTES).unwrap(),"8a6d6b954121c1bd329d8b5b67ceff23","d3d70ab08914a31ad4aa6829077917aee34b8558532f85a676cc947684b8bb56",old_id],
    ).unwrap();
    let receipt = client
        .cloud_command("1", planned(input.clone()))
        .await
        .unwrap();
    assert_eq!(receipt.accepted, original.accepted);
    assert!(receipt.upload_limit.is_none());
    assert_eq!(receipt.not_started.len(), 1);
    let newly_selected = || CloudCommand::Upload {
        files: vec![input.clone()],
    };
    let duplicate = client.cloud_command("1", newly_selected()).await.unwrap();
    assert!(duplicate.accepted.is_empty());
    assert_eq!(duplicate.failed[0].message, "已经在上传队列中");
    assert!(duplicate.upload_limit.is_none());
    file.write_all(b"Z").unwrap();
    let replacement = client.cloud_command("1", newly_selected()).await.unwrap();
    assert!(replacement.accepted.is_empty());
    assert_eq!(replacement.upload_limit, Some(CloudUploadLimit::NoNewTasks));
    assert_eq!(replacement.failed[0].message, "文件超过100M，无法上传");
    let tasks = client
        .cloud_page("1", query(CloudTab::Uploading), false)
        .await
        .unwrap()
        .uploads;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, *old_id);
    assert_eq!(tasks[0].total, BYTES);
    assert!(tasks[0].can_resume && !tasks[0].enabled && !tasks[0].running);
    assert!(f.api.calls.lock().unwrap().is_empty());

    // Explicitly resuming the original matching source still uses the old
    // durable length. Hold before object allocation completes; no media send.
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"a").unwrap();
    drop(file);
    db.execute_batch("DROP TRIGGER fail_cloud_schedule")
        .unwrap();
    let gate = Arc::new(support::nos::Gate {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Semaphore::new(0),
    });
    *f.api.allocation_gate.lock().unwrap() = Some(gate.clone());
    let resume = client
        .cloud_command(
            "1",
            CloudCommand::Resume {
                ids: vec![old_id.clone()],
                retry_rejected: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(resume.accepted, vec![old_id.clone()]);
    tokio::time::timeout(Duration::from_secs(30), gate.entered.notified())
        .await
        .unwrap();
    {
        let calls = f.api.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, support::CHECK);
        assert_eq!(calls[0].1["length"], BYTES);
        assert_eq!(calls[1].0, support::ALLOC);
    }
    client
        .cloud_command(
            "1",
            CloudCommand::Pause {
                ids: vec![old_id.clone()],
            },
        )
        .await
        .unwrap();
    gate.release.add_permits(1);
}
