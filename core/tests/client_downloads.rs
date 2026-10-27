#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod playback_support;
mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    download::{
        completed::FileAction,
        service::{DownloadOptions, DownloadStatus, LyricsStatus},
    },
    protocol::http::ApiOrigin,
};
use serde_json::json;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
const FLAC: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");
struct Fixture {
    root: tempfile::TempDir,
    session: Arc<AccountSession>,
    client: MusicClient,
    api: Arc<support::download::Server>,
    backend: Arc<support::download::Backend>,
    store: Arc<cumulus_core::storage::StateStore>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(&root.path().join("profile"), ApiOrigin::Pc).unwrap();
        let api = Arc::new(support::download::Server::new(FLAC, "flac"));
        let backend = Arc::new(support::download::Backend::new(FLAC.into()));
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let store = registry.store();
        let client = MusicClient::with_session(session.clone(), registry)
            .with_download_backend(backend.clone())
            .with_audio_factory(|| {
                Ok(playback_support::audio_engine(Arc::new(
                    playback_support::Device::default(),
                )))
            });
        Self {
            root,
            session,
            client,
            api,
            backend,
            store,
        }
    }
    async fn configure(&self) {
        self.client
            .download_command(
                "1",
                DownloadCommand::Configure {
                    root: self.root.path().join("media").to_string_lossy().into(),
                    path_type: 2,
                    file_type: 1,
                },
            )
            .await
            .unwrap();
    }
    async fn start(&self) -> String {
        self.configure().await;
        let r = self
            .client
            .download_command(
                "1",
                DownloadCommand::Start {
                    song_ids: vec!["123".into()],
                    options: options(),
                },
            )
            .await
            .unwrap();
        assert!(r.failed.is_empty());
        assert_eq!(r.accepted.len(), 1);
        r.accepted[0].clone()
    }
    async fn completed(&self) -> (String, DownloadsView) {
        let id = self.start().await;
        let view = wait(&self.client, "1", DownloadTab::Completed, |v| {
            v.completed_count == 1 && !v.tasks[0].cleanup_pending
        })
        .await;
        (id, view)
    }
}
fn options() -> DownloadOptions {
    DownloadOptions {
        tags: false,
        cover: false,
        lyrics: true,
        ..Default::default()
    }
}
async fn wait(
    client: &MusicClient,
    epoch: &str,
    tab: DownloadTab,
    predicate: impl Fn(&DownloadsView) -> bool,
) -> DownloadsView {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let v = client
                .downloads(
                    epoch,
                    DownloadQuery {
                        tab,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            if predicate(&v) {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("download view did not reach expected state")
}

#[tokio::test]
async fn native_downloads_publish_real_time_whitelist_data_and_preserve_preferences() {
    let f = Fixture::new();
    f.configure().await;
    f.client
        .download_command("1", DownloadCommand::Preferences { options: options() })
        .await
        .unwrap();
    let (id, view) = f.completed().await;
    let task = &view.tasks[0];
    assert_eq!(task.id, id);
    assert_eq!(task.status, DownloadStatus::Complete);
    assert!(task.completed_ms.is_some_and(|ms| ms >= task.created_ms));
    assert_eq!(task.progress, Some(1.0));
    assert_eq!(task.requested_quality, "standard");
    assert_eq!(task.actual_quality.as_deref(), Some("lossless"));
    assert_eq!(task.total, Some(FLAC.len() as u64));
    assert!(view.options.lyrics);
    assert_eq!(fs::read(task.path.as_ref().unwrap()).unwrap(), FLAC);
    let wire = serde_json::to_string(&view).unwrap();
    assert!(!wire.contains("auth=fixture"));
    assert!(!wire.contains("privilege"));
    assert!(!wire.contains("resource"));
    let empty = f
        .client
        .downloads(
            "1",
            DownloadQuery {
                tab: DownloadTab::Completed,
                keyword: "not present".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(empty.total, 0);
    assert_eq!(empty.completed_count, 1);
    let last = f
        .client
        .downloads(
            "1",
            DownloadQuery {
                tab: DownloadTab::Completed,
                offset: 1,
                limit: 1,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(last.total, 1);
    assert!(last.tasks.is_empty());
}

#[tokio::test]
async fn commands_validate_epoch_and_ids_before_any_task_or_request_and_report_partial_failure() {
    let f = Fixture::new();
    f.configure().await;
    assert!(matches!(
        f.client
            .download_command(
                "0",
                DownloadCommand::Start {
                    song_ids: vec!["123".into()],
                    options: options()
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(
        f.client
            .download_command(
                "1",
                DownloadCommand::Start {
                    song_ids: vec!["123".into(), "invalid".into()],
                    options: options()
                }
            )
            .await
            .is_err()
    );
    assert!(
        f.client
            .downloads(
                "1",
                DownloadQuery {
                    limit: 0,
                    ..Default::default()
                }
            )
            .await
            .is_err()
    );
    let v = f.client.downloads("1", Default::default()).await.unwrap();
    assert_eq!(v.active_count, 0);
    assert_eq!(v.completed_count, 0);
    assert_eq!(f.backend.audio.calls().len(), 0);
    let result = f
        .client
        .download_command(
            "1",
            DownloadCommand::Control {
                task_ids: vec!["00000000000000000000000000000000".into()],
                action: TaskControl::Resume,
            },
        )
        .await
        .unwrap();
    assert!(result.accepted.is_empty());
    assert_eq!(result.failed.len(), 1);
}

#[tokio::test]
async fn paused_download_is_persistent_then_explicitly_resumes_and_complete_rejects_cancel() {
    let f = Fixture::new();
    let gate = Arc::new(support::transfer::ReadGate::default());
    f.backend.audio.settings.lock().unwrap().gate = Some(gate.clone());
    let id = f.start().await;
    tokio::time::timeout(Duration::from_secs(5), gate.reached.notified())
        .await
        .unwrap();
    let result = f
        .client
        .download_command(
            "1",
            DownloadCommand::Control {
                task_ids: vec![id.clone()],
                action: TaskControl::Pause,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.accepted, vec![id.clone()]);
    gate.release.notify_one();
    let v = wait(&f.client, "1", DownloadTab::Active, |v| {
        v.tasks
            .first()
            .is_some_and(|t| t.status == DownloadStatus::Paused)
    })
    .await;
    assert!(v.tasks[0].error.is_none());
    assert_eq!(v.tasks[0].progress, Some(0.0));
    f.backend.audio.settings.lock().unwrap().gate = None;
    f.client
        .download_command(
            "1",
            DownloadCommand::Control {
                task_ids: vec![id.clone()],
                action: TaskControl::Resume,
            },
        )
        .await
        .unwrap();
    wait(&f.client, "1", DownloadTab::Completed, |v| {
        v.completed_count == 1 && !v.tasks[0].cleanup_pending
    })
    .await;
    // Completed downloads cannot be cancelled through task control.
    let result = f
        .client
        .download_command(
            "1",
            DownloadCommand::Control {
                task_ids: vec![id],
                action: TaskControl::Cancel,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.failed.len(), 1);
}

#[tokio::test]
async fn rename_move_directory_and_delete_run_from_native_commands_without_reauthorization() {
    let f = Fixture::new();
    let (id, before) = f.completed().await;
    let old = PathBuf::from(before.tasks[0].path.as_ref().unwrap());
    let calls = f.backend.audio.calls().len();
    let result = f
        .client
        .download_command(
            "1",
            DownloadCommand::Rename {
                task_id: id.clone(),
                name: "新名字".into(),
            },
        )
        .await
        .unwrap();
    let op = result.operation_id.unwrap();
    let renamed = wait(&f.client, "1", DownloadTab::Completed, |v| {
        v.jobs.iter().any(|j| j.id == op && !j.running) && v.operations.is_empty()
    })
    .await;
    assert!(!old.exists());
    assert!(
        renamed.tasks[0]
            .path
            .as_ref()
            .unwrap()
            .ends_with("新名字.flac")
    );
    let target = f.root.path().join("other");
    let root = target.to_string_lossy().into_owned();
    f.client
        .download_command("1", DownloadCommand::Migrate { root: root.clone() })
        .await
        .unwrap();
    let migrated = wait(&f.client, "1", DownloadTab::Completed, |v| {
        v.migration.is_none() && v.directory.as_ref().is_some_and(|d| d.root == target)
    })
    .await;
    let path = PathBuf::from(migrated.tasks[0].path.as_ref().unwrap());
    assert!(path.starts_with(target));
    assert_eq!(fs::read(&path).unwrap(), FLAC);
    f.client
        .download_command(
            "1",
            DownloadCommand::File {
                task_id: id,
                action: FileAction::Delete,
                target: None,
            },
        )
        .await
        .unwrap();
    wait(&f.client, "1", DownloadTab::Completed, |v| {
        v.completed_count == 0 && v.jobs.iter().all(|j| !j.running)
    })
    .await;
    assert!(!path.exists());
    assert!(!path.with_extension("lrc").exists());
    assert_eq!(f.backend.audio.calls().len(), calls);
}

#[tokio::test]
async fn completed_lyric_failure_has_explicit_repair_without_downloading_audio_again() {
    let f = Fixture::new();
    *f.api.lyric.lock().unwrap() = Err(Error::Response("test"));
    let id = f.start().await;
    let v = wait(&f.client, "1", DownloadTab::Completed, |v| {
        v.tasks
            .first()
            .is_some_and(|t| t.cleanup_pending && t.lyrics_status == LyricsStatus::Error)
    })
    .await;
    assert_eq!(v.tasks[0].lyrics_status, LyricsStatus::Error);
    *f.api.lyric.lock().unwrap() = Ok(json!({"code":200,"lrc":{"lyric":"[00:00.00]repair"}}));
    f.client
        .download_command(
            "1",
            DownloadCommand::Control {
                task_ids: vec![id],
                action: TaskControl::Repair,
            },
        )
        .await
        .unwrap();
    let v = wait(&f.client, "1", DownloadTab::Completed, |v| {
        v.tasks.first().is_some_and(|t| !t.cleanup_pending)
    })
    .await;
    assert_eq!(v.tasks[0].lyrics_status, LyricsStatus::Complete);
    assert_eq!(f.backend.audio.calls().len(), 1);
}

#[tokio::test]
async fn same_uid_relogin_rejects_old_commands_and_old_domain_instances() {
    let f = Fixture::new();
    f.configure().await;
    let files =
        cumulus_core::download::completed::CompletedDownloads::new(f.session.clone()).unwrap();
    let service = cumulus_core::download::service::DownloadService::with_backend(
        f.session.clone(),
        f.backend.clone(),
        512 * 1024 * 1024,
    )
    .unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.downloads("1", Default::default()).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(files.list(), Err(Error::StaleResponse)));
    assert!(matches!(service.list(), Err(Error::StaleResponse)));
    assert!(f.client.downloads("2", Default::default()).await.is_ok());
}

#[tokio::test]
async fn local_download_selection_uses_the_owned_file_and_revalidates_integrity() {
    let f = Fixture::new();
    let (id, v) = f.completed().await;
    let path = v.tasks[0].path.clone().unwrap();
    let mut subscription = f.client.subscribe_playback("1").await.unwrap();
    f.client
        .play_downloads("1", vec![id.clone()], 0)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let view = subscription.current().unwrap();
            if view.state == cumulus_core::playback::controller::PlaybackStatus::Playing {
                assert_eq!(view.actual_quality.as_deref(), Some("lossless"));
                break;
            }
            subscription.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(f.api.count("player/url"), 0);
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    fs::write(path, b"modified").unwrap();
    assert!(f.client.play_downloads("1", vec![id], 0).await.is_err());
    assert_eq!(f.api.count("player/url"), 0);
}

#[tokio::test]
async fn forget_keeps_queue_playable_with_account_integrity_and_membership_guards() {
    use cumulus_core::{
        library::LocalLibrary,
        playback::{QueueSnapshot, source::SourceResolver},
    };
    let f = Fixture::new();
    let (id, v) = f.completed().await;
    let path = PathBuf::from(v.tasks[0].path.as_ref().unwrap());
    f.client
        .play_downloads("1", vec![id.clone()], 0)
        .await
        .unwrap();
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    let store = f.store.clone();
    let before: QueueSnapshot =
        serde_json::from_value(store.get("1", "playback", "queue").unwrap().unwrap()).unwrap();
    f.client
        .download_command(
            "1",
            DownloadCommand::File {
                task_id: id,
                action: FileAction::Forget,
                target: None,
            },
        )
        .await
        .unwrap();
    wait(&f.client, "1", DownloadTab::Completed, |v| {
        v.completed_count == 0 && v.jobs.iter().all(|j| !j.running)
    })
    .await;
    assert!(path.exists());
    assert!(path.with_extension("lrc").exists());
    let after: QueueSnapshot =
        serde_json::from_value(store.get("1", "playback", "queue").unwrap().unwrap()).unwrap();
    assert_eq!(after.order, before.order);
    assert_eq!(after.current, before.current);
    let mut resource = after.items[0].resource.clone();
    assert!(resource.get("download_task_id").is_none());
    assert!(resource.get("download_ledger").is_some());
    let resolver = SourceResolver::new(
        f.session.clone(),
        Some(Arc::new(LocalLibrary::new(store).unwrap())),
    );
    let token = f.session.token().unwrap();
    assert_eq!(
        resolver
            .resolve(&resource, &token)
            .await
            .unwrap()
            .actual_quality(),
        Some("lossless")
    );
    resource["download_ledger_owner"] = json!("2");
    assert!(matches!(
        resolver.resolve(&resource, &token).await,
        Err(Error::StaleResponse)
    ));
    resource["download_ledger_owner"] = json!("1");
    // Native ledger fixture: a membership-bound encrypted file must be rejected
    // before decoding when no valid entitlement snapshot exists.
    let mut gated = resource.clone();
    gated["download_ledger"]["encrypted"] = json!(true);
    gated["download_ledger"]["privilege"] = json!({"fee":1,"payed":0,"flag":0});
    assert!(resolver.resolve(&gated, &token).await.is_err());
    let wire = serde_json::to_string(&f.client.player_queue("1").await.unwrap()).unwrap();
    assert!(!wire.contains("download_ledger"));
    assert!(!wire.contains("privilege"));
    fs::write(path, b"changed").unwrap();
    assert!(resolver.resolve(&resource, &token).await.is_err());
    assert_eq!(f.api.count("player/url"), 0);
}

#[tokio::test]
async fn bulk_cancel_covers_tasks_outside_visible_page_and_removes_owned_temporary_files() {
    use cumulus_core::download::service::{DownloadAction, DownloadService};
    let f = Fixture::new();
    f.configure().await;
    let service =
        DownloadService::with_backend(f.session.clone(), f.backend.clone(), 512 * 1024 * 1024)
            .unwrap();
    for _ in 0..103 {
        let task = service.create_default(&json!(123), options()).unwrap();
        service.request(&task.id, DownloadAction::Pause).unwrap();
    }
    let before = f.client.downloads("1", Default::default()).await.unwrap();
    assert_eq!(before.active_count, 103);
    assert_eq!(before.tasks.len(), 100);
    let result = f
        .client
        .download_command(
            "1",
            DownloadCommand::All {
                action: TaskControl::Cancel,
            },
        )
        .await
        .unwrap();
    assert!(result.failed.is_empty());
    assert_eq!(result.accepted.len(), 103);
    wait(&f.client, "1", DownloadTab::Active, |v| v.active_count == 0).await;
    let tasks = service.list().unwrap();
    assert_eq!(tasks.len(), 103);
    assert!(
        tasks
            .iter()
            .all(|t| t.status == DownloadStatus::Cancelled && !t.work.exists())
    );
    assert!(f.backend.audio.calls().is_empty());
}
