use super::*;
use cumulus_core::download::service::{
    DownloadDestination, DownloadRunOptions, DownloadService, DownloadStatus,
};

async fn prepare(client: &MusicClient, ids: &[&str]) -> CloudRemovalView {
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    client
        .cloud_delete_prepare(
            "1",
            ids.iter().map(|s| (*s).into()).collect(),
            page.revision.unwrap(),
        )
        .await
        .unwrap()
}
async fn uploaded(f: &Fixture) -> (MusicClient, std::path::PathBuf) {
    let client = f.client();
    let input = f.input();
    let path = input.path.clone();
    client
        .cloud_command("1", CloudCommand::Upload { files: vec![input] })
        .await
        .unwrap();
    wait_done(&client).await;
    (client, path)
}
fn writes(f: &Fixture) -> usize {
    f.api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p == "/api/cloud/del")
        .count()
}
#[tokio::test]
async fn prepared_scope_stays_native_and_unchecked_choice_keeps_the_source_after_restart() {
    let f = Fixture::new(0);
    let (client, path) = uploaded(&f).await;
    let plan = prepare(&client, &["700"]).await;
    assert_eq!(plan.local_count, 1);
    assert_eq!(plan.phase, CloudRemovalPhase::Prepared);
    assert_eq!(writes(&f), 0);
    let wire = serde_json::to_string(&plan).unwrap();
    assert!(!wire.contains(f.root.path().to_str().unwrap()));
    for secret in ["SECRET", "sha256", "fingerprint", "source", "parent"] {
        assert!(!wire.contains(secret), "{secret}: {wire}");
    }
    let result = client
        .cloud_delete_execute("1", &plan.id, false, false)
        .await
        .unwrap();
    assert_eq!(result.files[0].state, CloudRemovalFileState::Skipped);
    assert_eq!(
        result.deletion.unwrap().phase,
        cumulus_core::cloud::library::DeletePhase::Confirmed
    );
    assert!(path.is_file());
    drop(client);
    let reopened = f.client();
    let again = reopened
        .cloud_delete_execute("1", &plan.id, false, false)
        .await
        .unwrap();
    assert_eq!(again.delete_local, Some(false));
    assert!(
        reopened
            .cloud_delete_execute("1", &plan.id, true, true)
            .await
            .is_err()
    );
    assert_eq!(writes(&f), 1);
    assert!(path.is_file());
}
#[tokio::test]
async fn optional_source_removal_is_durable_and_duplicate_execution_never_unlinks_a_replacement() {
    let f = Fixture::new(0);
    let (client, path) = uploaded(&f).await;
    let plan = prepare(&client, &["700"]).await;
    let result = client
        .cloud_delete_execute("1", &plan.id, true, false)
        .await
        .unwrap();
    assert_eq!(result.files[0].state, CloudRemovalFileState::Removed);
    assert!(!path.exists());
    assert_eq!(result.deletion.unwrap().removed, 1);
    std::fs::write(&path, b"replacement belongs to user").unwrap();
    drop(client);
    let receipt = f
        .client()
        .cloud_delete_execute("1", &plan.id, true, true)
        .await
        .unwrap();
    assert_eq!(receipt.files[0].state, CloudRemovalFileState::Removed);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"replacement belongs to user"
    );
    assert_eq!(writes(&f), 1);
}
#[tokio::test]
async fn same_size_modified_source_is_retained_while_remote_deletion_still_finishes() {
    let f = Fixture::new(0);
    let (client, path) = uploaded(&f).await;
    let plan = prepare(&client, &["700"]).await;
    let replacement = vec![b'X'; support::DATA.len()];
    std::fs::write(&path, &replacement).unwrap();
    let result = client
        .cloud_delete_execute("1", &plan.id, true, false)
        .await
        .unwrap();
    assert_eq!(result.files[0].state, CloudRemovalFileState::Retained);
    assert!(result.files[0].issue.unwrap().contains("已保留"));
    assert_eq!(std::fs::read(&path).unwrap(), replacement);
    assert_eq!(result.deletion.unwrap().removed, 1);
    assert_eq!(writes(&f), 1);
}
#[tokio::test]
async fn remote_preflight_change_does_not_touch_files_and_account_switch_cannot_resume_it() {
    let f = Fixture::new(0);
    let (client, path) = uploaded(&f).await;
    let plan = prepare(&client, &["700"]).await;
    f.api.rows.lock().unwrap().push(row(800));
    let result = client
        .cloud_delete_execute("1", &plan.id, true, false)
        .await
        .unwrap();
    assert_eq!(result.phase, CloudRemovalPhase::Result);
    assert!(!result.can_continue);
    assert!(result.error.unwrap().contains("重新确认"));
    assert!(path.exists());
    assert_eq!(writes(&f), 0);
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        client.cloud_delete_view("1", &plan.id, false).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        client
            .cloud_delete_execute("2", &plan.id, true, true)
            .await
            .is_err()
    );
    assert!(path.exists());
    assert_eq!(writes(&f), 0);
}
#[tokio::test]
async fn rejected_partial_and_lost_remote_replies_return_readable_receipts_without_reposting() {
    use cumulus_core::cloud::library::DeletePhase;
    for (mode, phase, removed) in [
        (1, DeletePhase::Rejected, 0),
        (2, DeletePhase::Partial, 1),
        (3, DeletePhase::Confirmed, 2),
    ] {
        let f = Fixture::new(2);
        f.api.delete_mode.store(mode, Ordering::SeqCst);
        let client = f.client();
        let plan = prepare(&client, &["1", "2"]).await;
        let result = client
            .cloud_delete_execute("1", &plan.id, false, false)
            .await
            .unwrap();
        assert_eq!(result.deletion.as_ref().unwrap().phase, phase);
        assert_eq!(result.deletion.as_ref().unwrap().removed, removed);
        let read = f
            .client()
            .cloud_delete_view("1", &plan.id, true)
            .await
            .unwrap();
        assert_eq!(read.deletion.unwrap().removed, removed);
        client
            .cloud_delete_execute("1", &plan.id, false, true)
            .await
            .unwrap();
        assert_eq!(writes(&f), 1);
    }
}
#[tokio::test]
async fn completed_cloud_download_is_removed_through_its_existing_file_journal() {
    let f = Fixture::new(1);
    f.api.rows.lock().unwrap()[0]["fileSize"] = json!(resources::MEDIA_BYTES.len());
    let service =
        DownloadService::with_backend(f.session.clone(), Arc::new(resources::Files), 1024 * 1024)
            .unwrap();
    let directory = f.root.path().join("download");
    std::fs::create_dir(&directory).unwrap();
    let target = directory.join("cloud.flac");
    let task = service
        .create_cloud(
            &json!(1),
            DownloadDestination::Path {
                path: target.clone(),
            },
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        service
            .run(&task.id, DownloadRunOptions::default())
            .await
            .unwrap()
            .status,
        DownloadStatus::Complete
    );
    let client = f.client();
    let plan = prepare(&client, &["1"]).await;
    assert_eq!(plan.local_count, 1);
    let result = client
        .cloud_delete_execute("1", &plan.id, true, false)
        .await
        .unwrap();
    assert_eq!(result.files[0].state, CloudRemovalFileState::Removed);
    assert!(!target.exists());
    assert!(service.get(&task.id).is_err());
    client
        .cloud_delete_execute("1", &plan.id, true, true)
        .await
        .unwrap();
    assert_eq!(writes(&f), 1);
}
#[tokio::test]
async fn cancelling_preview_performs_no_remote_write_and_leaves_the_source_unchanged() {
    let f = Fixture::new(0);
    let (client, path) = uploaded(&f).await;
    let plan = prepare(&client, &["700"]).await;
    client.cloud_delete_discard("1", &plan.id).await.unwrap();
    assert!(
        client
            .cloud_delete_view("1", &plan.id, false)
            .await
            .is_err()
    );
    assert!(path.exists());
    assert_eq!(writes(&f), 0);
}
#[tokio::test]
async fn cancelled_async_caller_cannot_unlock_a_still_running_file_worker_or_send_remote_early() {
    let f = Fixture::new(1);
    f.api.rows.lock().unwrap()[0]["fileSize"] = json!(resources::MEDIA_BYTES.len());
    let service =
        DownloadService::with_backend(f.session.clone(), Arc::new(resources::Files), 1024 * 1024)
            .unwrap();
    let directory = f.root.path().join("leased-download");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("cloud.flac");
    let task = service
        .create_cloud(
            &json!(1),
            DownloadDestination::Path { path: path.clone() },
            false,
        )
        .await
        .unwrap();
    service
        .run(&task.id, DownloadRunOptions::default())
        .await
        .unwrap();
    let client = Arc::new(f.client());
    let plan = prepare(&client, &["1"]).await;
    let store = ProfileRegistry::open(f.root.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    // Hold the real decoder's cooperative OS lease so the file worker must wait.
    let lease = cumulus_core::storage::OperationLock::acquire(
        store.directory(),
        "download-playback",
        &serde_json::to_string(&("1", &task.id)).unwrap(),
    )
    .unwrap();
    let captured = client.clone();
    let id = plan.id.clone();
    let dispatch =
        tokio::spawn(async move { captured.cloud_delete_execute("1", &id, true, false).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while service.get(&task.id).unwrap().file_operation.is_none() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    dispatch.abort();
    assert!(matches!(dispatch.await, Err(e) if e.is_cancelled()));
    assert!(
        client
            .cloud_delete_view("1", &plan.id, true)
            .await
            .unwrap()
            .running
    );
    assert!(matches!(
        client.cloud_delete_execute("1", &plan.id, true, true).await,
        Err(Error::MutationBusy)
    ));
    assert_eq!(writes(&f), 0);
    assert!(path.exists());
    drop(lease);
    tokio::time::timeout(Duration::from_secs(5), async {
        while client
            .cloud_delete_view("1", &plan.id, false)
            .await
            .unwrap()
            .running
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(!path.exists());
    assert_eq!(
        writes(&f),
        0,
        "a cancelled caller leaves remote submission explicitly pending"
    );
    let result = client
        .cloud_delete_execute("1", &plan.id, true, true)
        .await
        .unwrap();
    assert_eq!(result.files[0].state, CloudRemovalFileState::Removed);
    assert_eq!(result.deletion.unwrap().removed, 1);
    assert_eq!(writes(&f), 1);
}

fn index_cue(f: &Fixture, source: &std::path::Path) {
    let cue = source.with_extension("cue");
    std::fs::write(&cue, format!("FILE \"{}\" WAVE\n TRACK 01 AUDIO\n INDEX 01 00:00:00\n TRACK 02 AUDIO\n INDEX 01 00:01:00\n",source.file_name().unwrap().to_str().unwrap())).unwrap();
    local_files::library(f)
        .scan(source.parent().unwrap())
        .unwrap();
}
#[tokio::test]
async fn remote_change_after_local_cleanup_records_not_submitted_instead_of_claiming_a_rejection() {
    let f = Fixture::new(0);
    let (client, path) = uploaded(&f).await;
    let plan = prepare(&client, &["700"]).await;
    f.api.change_on_list.store(2, Ordering::SeqCst);
    let result = client
        .cloud_delete_execute("1", &plan.id, true, false)
        .await
        .unwrap();
    assert!(!path.exists());
    let remote = result.deletion.unwrap();
    assert_eq!(
        remote.phase,
        cumulus_core::cloud::library::DeletePhase::NotSubmitted
    );
    assert_eq!(
        remote.removed, 1,
        "observed external removal is distinct from submitting a request"
    );
    assert_eq!(remote.remaining, 0);
    assert!(result.error.unwrap().contains("未提交"));
    assert_eq!(writes(&f), 0);
    client
        .cloud_delete_execute("1", &plan.id, true, true)
        .await
        .unwrap();
    assert_eq!(writes(&f), 0);
}
#[tokio::test]
async fn missing_upload_record_invalidates_local_authority_without_blocking_remote_deletion() {
    let f = Fixture::new(0);
    let (client, path) = uploaded(&f).await;
    let plan = prepare(&client, &["700"]).await;
    let history = wait_done(&client).await;
    let id = history.uploads[0].id.clone();
    ProfileRegistry::open(f.root.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .delete("1", "cloud_uploads_v1", &id)
        .unwrap();
    let result = client
        .cloud_delete_execute("1", &plan.id, true, false)
        .await
        .unwrap();
    assert_eq!(result.files[0].state, CloudRemovalFileState::Retained);
    assert!(!result.can_continue);
    assert!(path.exists());
    assert_eq!(result.deletion.unwrap().removed, 1);
}
#[tokio::test]
async fn cue_sources_are_excluded_at_preview_and_rechecked_when_indexed_after_confirmation() {
    for before in [true, false] {
        let f = Fixture::new(1);
        let media = tempfile::tempdir().unwrap();
        let mut input = f.input();
        input.path = media.path().join("input.flac");
        std::fs::write(&input.path, support::DATA).unwrap();
        let path = input.path.clone();
        let client = f.client();
        client
            .cloud_command("1", CloudCommand::Upload { files: vec![input] })
            .await
            .unwrap();
        wait_done(&client).await;
        if before {
            index_cue(&f, &path);
        }
        let plan = prepare(&client, &["700", "1"]).await;
        assert_eq!(plan.cue_count, usize::from(before));
        assert_eq!(plan.local_count, usize::from(!before));
        if !before {
            index_cue(&f, &path);
        }
        let result = client
            .cloud_delete_execute("1", &plan.id, true, false)
            .await
            .unwrap();
        assert!(path.is_file());
        assert_eq!(result.deletion.unwrap().removed, 2);
        if !before {
            assert_eq!(result.files[0].state, CloudRemovalFileState::Retained);
            assert!(result.files[0].issue.unwrap().contains("CUE"));
        }
        assert_eq!(writes(&f), 1);
    }
}
#[tokio::test]
async fn local_failure_does_not_block_remote_and_explicit_retry_only_retries_local_work() {
    let f = Fixture::new(0);
    let (client, path) = uploaded(&f).await;
    let plan = prepare(&client, &["700"]).await;
    let store = ProfileRegistry::open(f.root.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let busy =
        cumulus_core::storage::OperationLock::acquire(store.directory(), "local", "library-index")
            .unwrap();
    let result = client
        .cloud_delete_execute("1", &plan.id, true, false)
        .await
        .unwrap();
    assert_eq!(result.files[0].state, CloudRemovalFileState::Failed);
    assert_eq!(result.deletion.unwrap().removed, 1);
    assert!(result.can_continue && path.is_file());
    drop(busy);
    let unchanged = client
        .cloud_delete_execute("1", &plan.id, true, false)
        .await
        .unwrap();
    assert_eq!(unchanged.files[0].state, CloudRemovalFileState::Failed);
    assert!(path.is_file());
    let retried = f
        .client()
        .cloud_delete_execute("1", &plan.id, true, true)
        .await
        .unwrap();
    assert_eq!(retried.files[0].state, CloudRemovalFileState::Removed);
    assert!(!path.exists());
    assert_eq!(writes(&f), 1);
}
#[tokio::test]
async fn a_crash_after_unlink_recovers_the_same_intent_and_never_replays_the_remote_request() {
    let f = Fixture::new(0);
    let (client, path) = uploaded(&f).await;
    let plan = prepare(&client, &["700"]).await;
    let store = ProfileRegistry::open(f.root.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let db = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER stop_after_unlink BEFORE INSERT ON state WHEN NEW.kind='cloud_removal_plans_v1' AND json_extract(NEW.value,'$.files[0].state')='removed' BEGIN SELECT RAISE(ABORT,'synthetic interruption'); END;").unwrap();
    assert!(
        client
            .cloud_delete_execute("1", &plan.id, true, false)
            .await
            .is_err()
    );
    assert!(!path.exists());
    assert_eq!(writes(&f), 0);
    let read = client.cloud_delete_view("1", &plan.id, true).await.unwrap();
    assert!(read.can_continue);
    assert_eq!(read.files[0].state, CloudRemovalFileState::Removing);
    assert_eq!(
        writes(&f),
        0,
        "read-only reconciliation must not send a reserved request"
    );
    assert!(
        client
            .cloud_command(
                "1",
                CloudCommand::Delete {
                    ids: vec!["700".into()],
                    revision: plan.revision.clone()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        writes(&f),
        0,
        "legacy deletion must respect the reservation"
    );
    db.execute_batch("DROP TRIGGER stop_after_unlink").unwrap();
    drop(client);
    let reopened = f.client();
    let recovered = prepare(&reopened, &["700"]).await;
    assert_eq!(recovered.id, plan.id);
    let result = reopened
        .cloud_delete_execute("1", &plan.id, true, true)
        .await
        .unwrap();
    assert_eq!(result.files[0].state, CloudRemovalFileState::Missing);
    assert_eq!(result.deletion.unwrap().removed, 1);
    assert_eq!(writes(&f), 1);
}
