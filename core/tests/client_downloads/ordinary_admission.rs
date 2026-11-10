use super::*;
use cumulus_core::{
    download::{
        directory::DownloadDirectory,
        service::{DownloadAction, DownloadRunOptions, DownloadService},
    },
    library::LocalLibrary,
    storage::OperationLock,
};

async fn start(f: &Fixture, ids: &[&str], options: DownloadOptions) -> DownloadResult {
    f.client
        .download_command(
            "1",
            DownloadCommand::Start {
                song_ids: ids.iter().map(|id| (*id).into()).collect(),
                options,
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn catalog_admission_reports_completed_local_and_replaced_file_without_request() {
    let f = Fixture::new();
    let (id, view) = f.completed().await;
    let published = PathBuf::from(view.tasks[0].path.as_ref().unwrap());
    let local_root = f.root.path().join("matched");
    fs::create_dir(&local_root).unwrap();
    fs::write(local_root.join("ordinary.flac"), FLAC).unwrap();
    let library = LocalLibrary::with_probe(f.store.clone(), Arc::new(MappedProbe)).unwrap();
    library.scan(&local_root).unwrap();
    let item = library.list(Default::default()).unwrap().remove(0);
    library
        .map_online(&f.session, &item.id, &json!(124))
        .unwrap();
    let prior_requests = f.api.count("download/url");
    let options = DownloadOptions {
        quality: Quality::HiRes,
        ..options()
    };
    let result = start(&f, &["123", "124", "123"], options.clone()).await;
    assert!(result.accepted.is_empty() && result.failed.is_empty());
    assert_eq!(
        result
            .skipped
            .iter()
            .map(|row| (row.id.as_str(), &row.reason))
            .collect::<Vec<_>>(),
        [
            ("123", &DownloadSkipReason::Downloaded),
            ("124", &DownloadSkipReason::Local)
        ]
    );
    let mut changed = FLAC.to_vec();
    changed[0] ^= 1;
    fs::write(&published, changed).unwrap();
    let result = start(&f, &["123"], options.clone()).await;
    assert!(result.accepted.is_empty() && result.failed.is_empty());
    assert_eq!(result.skipped[0].reason, DownloadSkipReason::Missing);
    fs::write(&published, FLAC).unwrap();
    let held = published.with_extension("held");
    fs::rename(&published, &held).unwrap();
    fs::write(&published, FLAC).unwrap();
    let result = start(&f, &["123"], options).await;
    assert!(result.accepted.is_empty() && result.failed.is_empty());
    assert_eq!(result.skipped[0].reason, DownloadSkipReason::Missing);
    assert_eq!(fs::read(&published).unwrap(), FLAC);
    assert_eq!(fs::read(&held).unwrap(), FLAC);
    assert_eq!(f.api.count("download/url"), prior_requests);
    assert_eq!(
        f.store
            .values("1", "download_tasks_v1")
            .unwrap()
            .into_keys()
            .collect::<Vec<_>>(),
        [id]
    );
}

#[tokio::test]
async fn catalog_admission_resumes_only_exact_options_and_uses_retained_range() {
    let f = Fixture::new();
    let service = prepare(&f);
    let old = service.create_default(&json!(123), options()).unwrap();
    let paused = service
        .run(
            &old.id,
            DownloadRunOptions {
                byte_budget: Some(17),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused);
    assert_eq!(paused.bytes, 17);
    let changed = start(
        &f,
        &["123"],
        DownloadOptions {
            quality: Quality::Lossless,
            ..options()
        },
    )
    .await;
    assert_eq!(
        changed.skipped[0].reason,
        DownloadSkipReason::OptionsChanged
    );
    assert!(changed.accepted.is_empty());
    assert_eq!(f.api.count("download/url"), 1);
    assert_eq!(service.get(&old.id).unwrap().bytes, 17);

    let resumed = start(&f, &["123"], options()).await;
    assert!(resumed.skipped.is_empty() && resumed.failed.is_empty());
    assert_eq!(resumed.accepted, [old.id.clone()]);
    wait(&f.client, "1", DownloadTab::Completed, |view| {
        view.completed_count == 1
    })
    .await;
    let completed = service.get(&old.id).unwrap();
    assert_eq!(completed.options, old.options);
    assert_eq!(completed.work, old.work);
    assert_eq!(f.backend.audio.calls().last().unwrap().offset, 17);
    assert_eq!(f.api.count("download/url"), 2);
}

#[tokio::test]
async fn catalog_admission_waits_for_durable_cancel_then_uses_new_quality() {
    let f = Fixture::new();
    let service = prepare(&f);
    let old = service.create_default(&json!(123), options()).unwrap();
    let waiting = start(&f, &["123"], options()).await;
    assert!(waiting.accepted.is_empty());
    assert_eq!(waiting.skipped[0].reason, DownloadSkipReason::Waiting);
    service.request(&old.id, DownloadAction::Pause).unwrap();
    let owner_task = serde_json::to_string(&("1", &old.id)).unwrap();
    let live = OperationLock::acquire(f.store.directory(), "download-task", &owner_task).unwrap();
    assert_eq!(
        service
            .request(&old.id, DownloadAction::Cancel)
            .unwrap()
            .status,
        DownloadStatus::Paused
    );
    let changed = DownloadOptions {
        quality: Quality::Lossless,
        ..options()
    };
    let pending = start(&f, &["123"], changed.clone()).await;
    assert_eq!(pending.skipped[0].reason, DownloadSkipReason::Cancelling);
    assert!(pending.accepted.is_empty());
    assert_eq!(f.api.count("download/url"), 0);
    drop(live);
    assert_eq!(
        service
            .request(&old.id, DownloadAction::Cancel)
            .unwrap()
            .status,
        DownloadStatus::Cancelled
    );
    let fresh = start(&f, &["123"], changed.clone()).await;
    assert!(fresh.failed.is_empty() && fresh.skipped.is_empty());
    assert_ne!(fresh.accepted[0], old.id);
    wait(&f.client, "1", DownloadTab::Completed, |view| {
        view.completed_count == 1
    })
    .await;
    let task = service.get(&fresh.accepted[0]).unwrap();
    assert_eq!(task.options, changed);
    assert_eq!(f.backend.audio.calls().last().unwrap().offset, 0);
    let calls = f.api.calls.lock().unwrap();
    let levels = calls
        .iter()
        .filter(|(path, _)| path.ends_with("download/url/v1"))
        .map(|(_, args)| args["level"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(levels, ["lossless"]);
}

fn prepare(f: &Fixture) -> DownloadService {
    DownloadDirectory::new(f.session.clone())
        .unwrap()
        .configure(&f.root.path().join("media"), 2, 1)
        .unwrap();
    DownloadService::with_backend(f.session.clone(), f.backend.clone(), 512 * 1024 * 1024).unwrap()
}
