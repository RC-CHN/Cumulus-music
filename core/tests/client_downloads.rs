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
fn online_missing(view: &PlayerView) -> PlayerAction {
    PlayerAction::PlayMissingDownloadOnline {
        play_id: view.play_id.clone().unwrap(),
        expected_queue_revision: view.queue_revision.clone(),
    }
}

#[tokio::test]
async fn a_missing_published_file_only_uses_fresh_online_rights_after_explicit_action() {
    use cumulus_core::playback::controller::PlaybackStatus;
    const URL_API: &str = "/api/song/enhance/player/url/v1";
    let f = Fixture::new();
    let (id, completed) = f.completed().await;
    let path = PathBuf::from(completed.tasks[0].path.as_ref().unwrap());
    fs::remove_file(&path).unwrap();
    let failure = f.client.play_downloads("1", vec![id.clone()], 0).await;
    assert!(matches!(
        failure.err().unwrap().root(),
        Error::DownloadedFileMissing
    ));
    let view = f.client.player_snapshot("1").await.unwrap();
    assert_eq!(view.state, PlaybackStatus::Error);
    assert_eq!(view.error_code, Some("playback_download_missing"));
    assert_eq!(view.error_retryable, Some(false));
    assert_eq!(view.download_task_id.as_deref(), Some(id.as_str()));
    assert!(view.entry_id.is_some());
    assert_eq!(f.api.count(URL_API), 0);
    assert!(
        f.client
            .player_action("0", online_missing(&view))
            .await
            .is_err()
    );
    assert!(
        f.client
            .player_action("1", PlayerAction::Refresh)
            .await
            .is_err()
    );
    assert_eq!(f.api.count(URL_API), 0);
    let view = f.client.player_snapshot("1").await.unwrap();
    *f.api.play.lock().unwrap() = Ok(json!({"code":200,"data":[{"id":123,"code":403}]}));
    let denied = f.client.player_action("1", online_missing(&view)).await;
    assert!(denied.err().unwrap().is_service_code(403));
    let still = f.client.player_snapshot("1").await.unwrap();
    assert_eq!(still.play_id, view.play_id);
    assert_eq!(still.entry_id, view.entry_id);
    assert_eq!(still.queue_revision, view.queue_revision);
    assert_eq!(still.error_code, view.error_code);
    assert_eq!(f.api.count(URL_API), 1);

    *f.api.play.lock().unwrap() = Ok(json!({"code":200,"data":[{"id":123,"code":200,
        "url":"https://m.test.music.126.net/trial?auth=fixture","md5":"fixture","expi":120,
        "level":"standard","freeTrialInfo":{"start":0.2,"end":0.8}}]}));
    let online = f
        .client
        .player_action("1", online_missing(&still))
        .await
        .unwrap();
    assert_eq!(online.state, PlaybackStatus::Playing);
    assert_eq!(online.entry_id, still.entry_id);
    assert_ne!(online.play_id, still.play_id);
    assert_eq!(online.download_task_id, None);
    assert!(online.can_change_quality);
    let timeline = online.timeline.unwrap();
    assert_eq!(timeline.start, 0.2);
    assert!((timeline.end.unwrap() - 0.8).abs() < 1e-8);
    assert_eq!(online.position, Some(0.2));
    let calls = f.api.calls.lock().unwrap();
    let online: Vec<_> = calls.iter().filter(|(p, _)| p == URL_API).collect();
    assert_eq!(online.len(), 2);
    assert!(online.iter().all(|(_, p)| p["level"] == "standard"));
    let record = f.store.values("1", "download_tasks_v1").unwrap();
    assert!(record.contains_key(&id));
    assert_eq!(f.store.values("1", "offline_music_v1").unwrap().len(), 1);
}

#[tokio::test]
async fn restored_or_corrupt_files_and_missing_parent_are_not_online_fallbacks() {
    const URL_API: &str = "/api/song/enhance/player/url/v1";
    let f = Fixture::new();
    let (id, completed) = f.completed().await;
    let path = PathBuf::from(completed.tasks[0].path.as_ref().unwrap());
    let saved = path.with_extension("held");
    fs::rename(&path, &saved).unwrap();
    assert!(
        f.client
            .play_downloads("1", vec![id.clone()], 0)
            .await
            .is_err()
    );
    let missing = f.client.player_snapshot("1").await.unwrap();
    assert_eq!(missing.error_code, Some("playback_download_missing"));
    fs::rename(&saved, &path).unwrap();
    assert!(
        f.client
            .player_action("1", online_missing(&missing))
            .await
            .is_err()
    );
    fs::write(&path, b"corrupt replacement").unwrap();
    assert!(
        f.client
            .play_downloads("1", vec![id.clone()], 0)
            .await
            .is_err()
    );
    let corrupt = f.client.player_snapshot("1").await.unwrap();
    assert_ne!(corrupt.error_code, Some("playback_download_missing"));
    assert!(
        f.client
            .player_action("1", online_missing(&corrupt))
            .await
            .is_err()
    );

    let parent = path.parent().unwrap();
    let moved = parent.with_extension("moved");
    fs::rename(parent, &moved).unwrap();
    assert!(f.client.play_downloads("1", vec![id], 0).await.is_err());
    let absent_parent = f.client.player_snapshot("1").await.unwrap();
    assert_ne!(absent_parent.error_code, Some("playback_download_missing"));
    assert_eq!(f.api.count(URL_API), 0);
}

#[tokio::test]
async fn an_explicit_missing_download_reply_cannot_undo_a_new_stop() {
    use cumulus_core::playback::controller::PlaybackStatus;
    let f = Fixture::new();
    let (id, completed) = f.completed().await;
    fs::remove_file(completed.tasks[0].path.as_ref().unwrap()).unwrap();
    assert!(
        f.client
            .play_downloads("1", vec![id.clone()], 0)
            .await
            .is_err()
    );
    let view = f.client.player_snapshot("1").await.unwrap();
    assert_eq!(view.error_code, Some("playback_download_missing"));
    let gate = Arc::new(support::download::PlaybackGate::default());
    *f.api.play_gate.lock().unwrap() = Some(gate.clone());
    let fallback = f.client.player_action("1", online_missing(&view));
    let stop = async {
        tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
            .await
            .unwrap();
        let stopped = f
            .client
            .player_action("1", PlayerAction::Stop)
            .await
            .unwrap();
        gate.release.notify_one();
        stopped
    };
    let (fallback, stopped) = tokio::join!(fallback, stop);
    assert!(matches!(
        fallback.err().unwrap().root(),
        Error::Cancelled | Error::StaleResponse
    ));
    let current = f.client.player_snapshot("1").await.unwrap();
    assert_eq!(current.state, PlaybackStatus::Idle);
    assert_eq!(current.entry_id, view.entry_id);
    assert_eq!(current.download_task_id.as_deref(), Some(id.as_str()));
    assert_eq!(current.queue_revision, stopped.queue_revision);
    assert_eq!(f.api.count("/api/song/enhance/player/url/v1"), 1);
}

#[tokio::test]
async fn restoring_the_exact_file_during_online_authorization_keeps_the_download_selection() {
    use cumulus_core::playback::controller::PlaybackStatus;
    let f = Fixture::new();
    let (id, completed) = f.completed().await;
    let path = PathBuf::from(completed.tasks[0].path.as_ref().unwrap());
    let saved = path.with_extension("held");
    fs::rename(&path, &saved).unwrap();
    assert!(
        f.client
            .play_downloads("1", vec![id.clone()], 0)
            .await
            .is_err()
    );
    let failed = f.client.player_snapshot("1").await.unwrap();
    let gate = Arc::new(support::download::PlaybackGate::default());
    *f.api.play_gate.lock().unwrap() = Some(gate.clone());
    let authorization = f.client.player_action("1", online_missing(&failed));
    let restore = async {
        tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
            .await
            .unwrap();
        fs::rename(&saved, &path).unwrap();
        gate.release.notify_one();
    };
    let (authorization, ()) = tokio::join!(authorization, restore);
    assert!(matches!(
        authorization.err().unwrap().root(),
        Error::StaleResponse
    ));
    let current = f.client.player_snapshot("1").await.unwrap();
    assert_eq!(current.state, PlaybackStatus::Error);
    assert_eq!(current.play_id, failed.play_id);
    assert_eq!(current.queue_revision, failed.queue_revision);
    let restored = f
        .client
        .play_downloads("1", vec![id.clone()], 0)
        .await
        .unwrap();
    assert_eq!(restored.state, PlaybackStatus::Playing);
    assert_eq!(restored.download_task_id.as_deref(), Some(id.as_str()));
    assert_eq!(f.api.count("/api/song/enhance/player/url/v1"), 1);
}

#[tokio::test]
async fn downloaded_sidecar_fills_missing_lyrics_without_leaking_into_other_selections() {
    use cumulus_core::playback::PlaybackQueue;

    let f = Fixture::new();
    let (id, completed) = f.completed().await;
    let audio = PathBuf::from(completed.tasks[0].path.as_ref().unwrap());
    let sidecar = audio.with_extension("lrc");
    fs::write(&sidecar, "[00:01.00]本地附件歌词\n[00:02.00]第二行").unwrap();
    *f.api.lyric.lock().unwrap() = Ok(json!({"code":200}));

    let play = f
        .client
        .play_downloads("1", vec![id.clone()], 0)
        .await
        .unwrap();
    let play_id = play.play_id.as_deref().unwrap();
    let lyrics = f.client.player_lyrics("1", play_id).await.unwrap();
    assert_eq!(lyrics.lines[0].lyric, "本地附件歌词");
    assert!(lyrics.from_sidecar);
    assert_eq!(
        f.client
            .lyric_offset("1", play_id, 1.0)
            .await
            .unwrap()
            .offset,
        0.5
    );

    // A selection with the same song ID and local path but no published task
    // provenance must not inherit the download service's sidecar document.
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    PlaybackQueue::new(Some(f.store.clone()), "1")
        .unwrap()
        .replace(vec![json!({"id":"123","path":audio})], 0, json!(null))
        .unwrap();
    let other = f
        .client
        .player_action("1", PlayerAction::Play)
        .await
        .unwrap();
    let other_id = other.play_id.as_deref().unwrap();
    assert_ne!(other_id, play_id);
    let other_lyrics = f.client.player_lyrics("1", other_id).await.unwrap();
    assert!(other_lyrics.lines.is_empty());
    assert!(!other_lyrics.from_sidecar);
    assert!(matches!(
        f.client.player_lyrics("1", play_id).await,
        Err(Error::StaleResponse)
    ));

    *f.api.lyric.lock().unwrap() = Err(Error::Transport("offline lyric fixture".into()));
    let play = f
        .client
        .play_downloads("1", vec![id.clone()], 0)
        .await
        .unwrap();
    let lyrics = f
        .client
        .player_lyrics("1", play.play_id.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(lyrics.lines[0].lyric, "本地附件歌词");
    assert_eq!(lyrics.offset, 0.5);
    assert!(lyrics.network_failed && lyrics.from_sidecar);
    *f.api.lyric.lock().unwrap() = Ok(json!({"code":200}));

    #[cfg(unix)]
    {
        let unrelated = f.root.path().join("unrelated.txt");
        fs::write(&unrelated, "[00:01]不得读取").unwrap();
        fs::remove_file(&sidecar).unwrap();
        std::os::unix::fs::symlink(unrelated, &sidecar).unwrap();
        f.client
            .player_action("1", PlayerAction::Stop)
            .await
            .unwrap();
        // Re-enter through an unscoped selection so a fresh sidecar read
        // cannot be satisfied by the previously authorized display snapshot.
        PlaybackQueue::new(Some(f.store.clone()), "1")
            .unwrap()
            .replace(vec![json!({"id":"123","path":audio})], 0, json!(null))
            .unwrap();
        let other = f
            .client
            .player_action("1", PlayerAction::Play)
            .await
            .unwrap();
        f.client
            .player_lyrics("1", other.play_id.as_deref().unwrap())
            .await
            .unwrap();
        let play = f
            .client
            .play_downloads("1", vec![id.clone()], 0)
            .await
            .unwrap();
        assert!(
            f.client
                .player_lyrics("1", play.play_id.as_deref().unwrap())
                .await
                .unwrap()
                .lines
                .is_empty()
        );
        fs::remove_file(&sidecar).unwrap();
        fs::write(&sidecar, "[00:01.00]本地附件歌词").unwrap();
    }

    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    PlaybackQueue::new(Some(f.store.clone()), "1")
        .unwrap()
        .replace(vec![json!({"id":"123","path":audio})], 0, json!(null))
        .unwrap();
    let other = f
        .client
        .player_action("1", PlayerAction::Play)
        .await
        .unwrap();
    f.client
        .player_lyrics("1", other.play_id.as_deref().unwrap())
        .await
        .unwrap();

    // Usable remote lyrics take precedence over the neighboring attachment.
    *f.api.lyric.lock().unwrap() =
        Ok(json!({"code":200,"lrc":{"version":1,"lyric":"[00:01]在线正文"}}));
    let play = f.client.play_downloads("1", vec![id], 0).await.unwrap();
    let lyrics = f
        .client
        .player_lyrics("1", play.play_id.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(lyrics.lines[0].lyric, "在线正文");
    assert!(!lyrics.from_sidecar);
    assert_eq!(lyrics.offset, 0.0);
    assert_eq!(f.api.count("player/url"), 0);
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
    let target = f.root.path().canonicalize().unwrap().join("other");
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
                assert_eq!(view.download_task_id.as_deref(), Some(id.as_str()));
                assert!(
                    !serde_json::to_string(&view)
                        .unwrap()
                        .contains(&serde_json::to_string(&path).unwrap())
                );
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
async fn forgetting_a_download_keeps_suspended_ordinary_queues_playable() {
    use cumulus_core::{
        catalog::intelligence::{IntelligenceRequest, IntelligenceStart},
        library::LocalLibrary,
        playback::{
            PlaybackQueue, QueueSnapshot,
            intelligence::{IntelligenceContext, IntelligencePhase},
            source::SourceResolver,
        },
    };
    for intelligence in [false, true] {
        let f = Fixture::new();
        let (task, _) = f.completed().await;
        f.client
            .play_downloads("1", vec![task.clone()], 0)
            .await
            .unwrap();
        f.client
            .player_action("1", PlayerAction::Stop)
            .await
            .unwrap();
        let before = f.store.get("1", "playback", "queue").unwrap().unwrap();
        let backup: QueueSnapshot = serde_json::from_value(before.clone()).unwrap();
        let mut fallback = backup.clone();
        fallback.items[0].resource = json!({"id":7,"dt":120000});
        fallback.items[0].source = json!({"kind":"playlist","id":"31"});
        let mut current = fallback.clone();
        let session_key = "playback-intelligence-v1";
        let fm_key = "fm-backup-queue";
        let old_revision = uuid::Uuid::new_v4().to_string();
        if intelligence {
            let context = IntelligenceContext {
                id: uuid::Uuid::new_v4().to_string(),
                phase: IntelligencePhase::Seeded,
            };
            let request = IntelligenceRequest {
                playlist_id: "31".into(),
                song_id: "7".into(),
                start_music_id: "7".into(),
                count: 1,
                start: IntelligenceStart::PlayOne,
            };
            current.intelligence = Some(context.clone());
            f.store
                .put(
                    "1",
                    session_key,
                    "session",
                    &json!({
                        "schema":1,"revision":old_revision,"context":context,"request":request,
                        "seed_entry_id":current.current,"last_mode":backup.mode,
                        "backup":backup,"fallback":fallback,"recommendations":{},"failure":null,
                    }),
                )
                .unwrap();
        } else {
            f.store.put("1", "playback", fm_key, &before).unwrap();
        }
        f.store.put("2", "playback", fm_key, &before).unwrap();
        let active = serde_json::to_value(current).unwrap();
        f.store.put("1", "playback", "queue", &active).unwrap();
        assert!(PlaybackQueue::new(Some(f.store.clone()), "1").is_ok());
        f.client
            .download_command(
                "1",
                DownloadCommand::File {
                    task_id: task,
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
        assert_eq!(f.store.get("1", "playback", "queue").unwrap(), Some(active));
        assert_eq!(f.store.get("2", "playback", fm_key).unwrap(), Some(before));
        let after: QueueSnapshot = if intelligence {
            let queue = PlaybackQueue::new(Some(f.store.clone()), "1").unwrap();
            let session = queue.intelligence_session().unwrap();
            assert_ne!(session.revision, old_revision);
            assert_eq!(session.fallback, fallback);
            session.backup.clone()
        } else {
            serde_json::from_value(f.store.get("1", "playback", fm_key).unwrap().unwrap()).unwrap()
        };
        assert_eq!(after.current, backup.current);
        assert_eq!(after.order, backup.order);
        let resource = &after.items[0].resource;
        assert!(resource.get("download_task_id").is_none());
        assert_eq!(resource["download_ledger_owner"], "1");
        let resolver = SourceResolver::new(
            f.session.clone(),
            Some(Arc::new(LocalLibrary::new(f.store.clone()).unwrap())),
        );
        assert_eq!(
            resolver
                .resolve(resource, &f.session.token().unwrap())
                .await
                .unwrap()
                .actual_quality(),
            Some("lossless")
        );
        assert_eq!(f.api.count("player/url"), 0);
    }
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

#[tokio::test]
async fn history_reuses_completed_download_after_native_offline_verification() {
    let f = Fixture::new();
    let (id, _) = f.completed().await;
    f.client.play_downloads("1", vec![id], 0).await.unwrap();
    let history = f
        .client
        .history_page("1", HistoryKind::Song, false)
        .await
        .unwrap()
        .value;
    assert_eq!(history.entries.len(), 1);
    let requests = f.api.calls.lock().unwrap().len();
    let state = f
        .client
        .play_selection(
            "1",
            PlaySelection::History {
                history_kind: HistoryKind::Song,
                revision: history.revision,
                ids: vec![history.entries[0].id.clone()],
                index: 0,
                quality: cumulus_core::membership::Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.queue_len, 1);
    assert!(state.error.is_none());
    assert_eq!(state.track.unwrap().id, "123");
    assert!(
        f.api.calls.lock().unwrap()[requests..]
            .iter()
            .all(|(path, _)| path == "/api/music-vip-membership/client/vip/info")
    );
}

#[tokio::test]
async fn download_categories_filter_before_pagination_and_keep_cloud_audio_with_songs() {
    use cumulus_core::download::service::DownloadOrigin;
    let f = Fixture::new();
    let (original, _) = f.completed().await;
    let seed = f
        .store
        .get("1", "download_tasks_v1", &original)
        .unwrap()
        .unwrap();
    for i in 0..240_u64 {
        let mut record = seed.clone();
        let id = format!("{i:032x}");
        record["id"] = json!(id);
        record["song"] = json!({"id":10000+i,"name":format!("分类对照 {i:03}"),"ar":[{"name":"fixture"}],"al":{"name":"fixture"}});
        record["origin"] = serde_json::to_value(match i % 4 {
            0 => DownloadOrigin::Song,
            1 => DownloadOrigin::Cloud {
                file_name: "original.wav".into(),
                file_size: 42,
            },
            2 => DownloadOrigin::Voice {
                voice_id: i.to_string(),
            },
            _ => DownloadOrigin::Mv { resolution: 1080 },
        })
        .unwrap();
        f.store.put("1", "download_tasks_v1", &id, &record).unwrap();
    }
    let query = DownloadQuery {
        tab: DownloadTab::Completed,
        category: DownloadCategory::Song,
        keyword: "分类对照".into(),
        sort: DownloadSort::Name,
        descending: false,
        offset: 37,
        limit: 37,
    };
    let view = f.client.downloads("1", query.clone()).await.unwrap();
    assert_eq!(view.total, 120);
    assert_eq!(view.tasks.len(), 37);
    assert_eq!(view.completed_count, 241);
    assert_eq!(view.tasks[0].track.name, "分类对照 073");
    assert!(
        view.tasks
            .iter()
            .all(|t| matches!(t.source, "song" | "cloud"))
    );
    for (category, source) in [
        (DownloadCategory::Voice, "voice"),
        (DownloadCategory::Mv, "mv"),
    ] {
        let v = f
            .client
            .downloads(
                "1",
                DownloadQuery {
                    category,
                    ..query.clone()
                },
            )
            .await
            .unwrap();
        assert_eq!(v.total, 60);
        assert_eq!(v.tasks.len(), 23);
        assert!(v.tasks.iter().all(|t| t.source == source));
    }
    let legacy: DownloadQuery=serde_json::from_value(json!({"tab":"completed","keyword":"分类对照","sort":"name","descending":false,"offset":0,"limit":100})).unwrap();
    assert_eq!(legacy.category, DownloadCategory::All);
    assert_eq!(f.client.downloads("1", legacy).await.unwrap().total, 240);
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(f.client.downloads("1", query.clone()).await.is_err());
    let epoch = f.client.account().unwrap().epoch;
    assert_eq!(f.client.downloads(&epoch, query).await.unwrap().total, 0);
}

#[tokio::test]
async fn filtered_download_playback_uses_the_entire_order_not_the_visible_prefix() {
    use cumulus_core::download::offline::{OfflineLibrary, PublishedDownload};
    let f = Fixture::new();
    let (original, view) = f.completed().await;
    let seed = f
        .store
        .get("1", "download_tasks_v1", &original)
        .unwrap()
        .unwrap();
    let offline = OfflineLibrary::new(f.session.clone()).unwrap();
    let token = f.session.token().unwrap();
    let original_path = PathBuf::from(view.tasks[0].path.as_ref().unwrap());
    for i in 1..=140 {
        let mut record = seed.clone();
        let id = format!("{i:032x}");
        let song = format!("{}", 20000 + i);
        let path = f.root.path().join(format!("queue-{i}.flac"));
        fs::hard_link(&original_path, &path).unwrap();
        record["id"] = json!(id);
        record["song_id"] = json!(song);
        record["target"] = json!(path);
        record["song"] = json!({"id":song,"name":format!("完整下载 {i:03}"),"ar":[{"name":"合成"}],"al":{"name":"fixture"},"dt":1000});
        record["resource"]["id"] = json!(song);
        let task: cumulus_core::download::service::DownloadTask =
            serde_json::from_value(record.clone()).unwrap();
        offline
            .register(
                &token,
                &PublishedDownload {
                    task_id: task.id.clone(),
                    song_id: task.song_id.clone(),
                    target: path,
                    requested_quality: task.options.quality,
                    song: task.song.unwrap(),
                    resource: task.resource.unwrap(),
                    encrypted: false,
                    integrity: task.output_integrity.unwrap(),
                },
            )
            .unwrap();
        f.store.put("1", "download_tasks_v1", &id, &record).unwrap();
    }
    let query = DownloadQuery {
        tab: DownloadTab::Completed,
        category: DownloadCategory::Song,
        keyword: "完整下载".into(),
        sort: DownloadSort::Name,
        descending: true,
        offset: 0,
        limit: 100,
    };
    assert_eq!(
        f.client
            .downloads("1", query.clone())
            .await
            .unwrap()
            .tasks
            .len(),
        100
    );
    let mut wider = query.clone();
    wider.limit = 200;
    assert_eq!(
        f.client.downloads("1", wider).await.unwrap().tasks.len(),
        140
    );
    f.client
        .play_selection(
            "1",
            PlaySelection::Downloads {
                query: query.clone(),
                start_id: Some(format!("{:032x}", 17)),
            },
        )
        .await
        .unwrap();
    let queue = f.client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 140);
    assert_eq!(queue.tracks.first().unwrap().track.id, "20140");
    assert_eq!(queue.tracks.last().unwrap().track.id, "20001");
    let current = queue
        .tracks
        .iter()
        .find(|t| Some(&t.entry_id) == queue.current.as_ref())
        .unwrap();
    assert_eq!(current.track.id, "20017");
    let before = queue.revision;
    assert!(matches!(
        f.client
            .play_selection(
                "1",
                PlaySelection::Downloads {
                    query: query.clone(),
                    start_id: Some("not-present".into())
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.client.player_queue("1").await.unwrap().revision, before);
    let mut active = query.clone();
    active.tab = DownloadTab::Active;
    assert!(
        f.client
            .play_selection(
                "1",
                PlaySelection::Downloads {
                    query: active,
                    start_id: None
                }
            )
            .await
            .is_err()
    );
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    let mut filtered = query;
    filtered.keyword = "完整下载 01".into();
    filtered.descending = false;
    f.client
        .play_selection(
            "1",
            PlaySelection::Downloads {
                query: filtered,
                start_id: None,
            },
        )
        .await
        .unwrap();
    let queue = f.client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 10);
    assert_eq!(queue.tracks[0].track.id, "20010");
    assert_eq!(queue.tracks[9].track.id, "20019");
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    assert_eq!(f.api.count("player/url"), 0);
}

#[tokio::test]
async fn unsorted_downloads_preserve_service_order_across_windows() {
    use cumulus_core::download::service::DownloadOrigin;
    let f = Fixture::new();
    let (original, _) = f.completed().await;
    let seed = f
        .store
        .get("1", "download_tasks_v1", &original)
        .unwrap()
        .unwrap();
    for (i, name, created, completed) in [(1, "A", 30, 10), (2, "Z", 10, 20), (3, "M", 20, 30)] {
        let mut value = seed.clone();
        let id = format!("{i:032x}");
        value["id"] = json!(id);
        value["song"] = json!({"id":i,"name":format!("取消排序 {name}")});
        value["origin"] = serde_json::to_value(DownloadOrigin::Voice {
            voice_id: i.to_string(),
        })
        .unwrap();
        value["created_ms"] = json!(created);
        value["completed_ms"] = json!(completed);
        f.store.put("1", "download_tasks_v1", &id, &value).unwrap();
    }
    let query = DownloadQuery {
        tab: DownloadTab::Completed,
        category: DownloadCategory::Voice,
        keyword: "取消排序".into(),
        sort: DownloadSort::None,
        descending: false,
        offset: 0,
        limit: 100,
    };
    for descending in [false, true] {
        let rows = f
            .client
            .downloads(
                "1",
                DownloadQuery {
                    descending,
                    ..query.clone()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            rows.tasks
                .iter()
                .map(|t| t.track.id.as_str())
                .collect::<Vec<_>>(),
            ["2", "3", "1"]
        );
    }
    let page = f
        .client
        .downloads(
            "1",
            DownloadQuery {
                offset: 1,
                limit: 1,
                ..query.clone()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.tasks[0].track.id, "3");
    let sorted = f
        .client
        .downloads(
            "1",
            DownloadQuery {
                sort: DownloadSort::Completed,
                descending: true,
                ..query
            },
        )
        .await
        .unwrap();
    assert_eq!(
        sorted
            .tasks
            .iter()
            .map(|t| t.track.id.as_str())
            .collect::<Vec<_>>(),
        ["3", "2", "1"]
    );
}

#[tokio::test]
async fn downloaded_mini_projection_preserves_the_service_like_identity_without_reauthorization() {
    use cumulus_core::client::{mini::MiniAction, native_media::NativeLikeResource};
    let f = Fixture::new();
    let (id, _) = f.completed().await;
    let calls = f.api.count("player/url");
    let selected = f
        .client
        .play_selection(
            "1",
            PlaySelection::Downloads {
                query: DownloadQuery {
                    tab: DownloadTab::Completed,
                    ..Default::default()
                },
                start_id: Some(id),
            },
        )
        .await
        .unwrap();
    let like = f.client.native_like_selection().unwrap().unwrap();
    assert_eq!(like.resource, NativeLikeResource::Song("123".into()));
    let mini = f.client.mini_snapshot().unwrap();
    assert_eq!(
        mini.view.item.as_ref().unwrap().title,
        selected.track.unwrap().name
    );
    assert!(mini.view.like.is_some());
    let public = serde_json::to_string(&mini.view).unwrap();
    assert!(!public.contains(f.root.path().to_str().unwrap()) && !public.contains("auth=fixture"));
    f.client
        .mini_action(&mini, MiniAction::Toggle)
        .await
        .unwrap();
    assert!(!f.client.mini_snapshot().unwrap().view.playing);
    assert_eq!(f.api.count("player/url"), calls);
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
}

#[tokio::test]
async fn downloaded_batch_queue_keeps_the_offline_entry_current_and_rejects_old_revisions() {
    let f = Fixture::new();
    let (id, _) = f.completed().await;
    let add = |revision: String, ids: Vec<String>| PlayerAction::QueueDownloads {
        task_ids: ids,
        expected_queue_revision: revision,
    };
    let initial = f.client.player_queue("1").await.unwrap().revision;
    f.client
        .player_action("1", add(initial.clone(), vec![id.clone(), id.clone()]))
        .await
        .unwrap();
    let before = f.client.player_queue("1").await.unwrap();
    assert_eq!(before.tracks.len(), 1);
    let same = f
        .client
        .player_action("1", add(before.revision.clone(), vec![id.clone()]))
        .await
        .unwrap();
    assert_eq!(same.queue_len, 1);
    assert_eq!(
        f.client.player_queue("1").await.unwrap().current,
        before.current
    );
    assert!(matches!(
        f.client.player_action("1", add(initial, vec![id])).await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(f.api.count("player/url"), 0);
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
}

#[tokio::test]
async fn batch_file_delete_reports_partial_completion_and_does_not_follow_a_changed_file() {
    let f = Fixture::new();
    let (id, view) = f.completed().await;
    let path = PathBuf::from(view.tasks[0].path.as_ref().unwrap());
    let command = |ids: Vec<String>| DownloadCommand::DeleteFiles { task_ids: ids };
    fs::write(&path, b"replacement data").unwrap();
    let protected = f
        .client
        .download_command("1", command(vec![id.clone()]))
        .await
        .unwrap();
    assert!(protected.accepted.is_empty());
    assert_eq!(protected.failed[0].id, id);
    assert_eq!(fs::read(&path).unwrap(), b"replacement data");
    let other = Fixture::new();
    let (valid, v) = other.completed().await;
    let other_path = PathBuf::from(v.tasks[0].path.as_ref().unwrap());
    let result = other
        .client
        .download_command(
            "1",
            command(vec!["absent".into(), valid.clone(), valid.clone()]),
        )
        .await
        .unwrap();
    assert_eq!(result.accepted, vec![valid]);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].id, "absent");
    assert!(!other_path.exists());
    assert_eq!(fs::read(path).unwrap(), b"replacement data");
}

#[tokio::test]
async fn file_operations_block_direct_and_queued_download_playback_but_not_other_downloads() {
    use cumulus_core::{
        library::LocalLibrary,
        playback::{QueueSnapshot, source::SourceResolver},
    };
    let f = Fixture::new();
    let (id, _) = f.completed().await;
    f.client
        .play_downloads("1", vec![id.clone()], 0)
        .await
        .unwrap();
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    let before: QueueSnapshot =
        serde_json::from_value(f.store.get("1", "playback", "queue").unwrap().unwrap()).unwrap();
    let resource = &before.items[0].resource;
    let mut task = f.store.get("1", "download_tasks_v1", &id).unwrap().unwrap();
    let original = task.clone();
    task["file_operation"] = json!("synthetic-move-journal");
    f.store.put("1", "download_tasks_v1", &id, &task).unwrap();
    assert!(matches!(
        f.client.play_downloads("1", vec![id.clone()], 0).await,
        Err(Error::MutationBusy)
    ));
    let resolver = SourceResolver::new(
        f.session.clone(),
        Some(Arc::new(LocalLibrary::new(f.store.clone()).unwrap())),
    );
    assert!(matches!(
        resolver
            .resolve(resource, &f.session.token().unwrap())
            .await,
        Err(Error::MutationBusy)
    ));
    // A different displayed record is being moved; the available original
    // must still start when playing the whole filtered results.
    let blocked_id = "synthetic-other-download";
    task["id"] = json!(blocked_id);
    f.store
        .put("1", "download_tasks_v1", blocked_id, &task)
        .unwrap();
    let mut ready = original;
    // A completed audio file can still be playable while sidecar cleanup is
    // pending; the independently verified ledger remains authoritative.
    ready["cleanup_pending"] = json!(true);
    f.store.put("1", "download_tasks_v1", &id, &ready).unwrap();
    let query = DownloadQuery {
        tab: DownloadTab::Completed,
        category: DownloadCategory::Song,
        ..Default::default()
    };
    assert!(matches!(
        f.client
            .play_selection(
                "1",
                PlaySelection::Downloads {
                    query: query.clone(),
                    start_id: Some(blocked_id.into())
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    let playing = f
        .client
        .play_selection(
            "1",
            PlaySelection::Downloads {
                query,
                start_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(playing.queue_len, 1);
    assert_eq!(playing.actual_quality.as_deref(), Some("lossless"));
    assert_eq!(f.api.count("player/url"), 0);
}

#[tokio::test]
async fn forgotten_sidecar_requires_a_completed_receipt_and_still_verified_audio() {
    use cumulus_core::{
        download::{
            completed::{CompletedDownloads, FilePhase},
            offline::OfflineLibrary,
        },
        playback::PlaybackQueue,
    };

    let f = Fixture::new();
    let (id, completed) = f.completed().await;
    let audio = PathBuf::from(completed.tasks[0].path.as_ref().unwrap());
    let sidecar = audio.with_extension("lrc");
    fs::write(&sidecar, "[00:01]保留的歌词").unwrap();
    *f.api.lyric.lock().unwrap() = Ok(json!({"code":200}));
    let token = f.session.token().unwrap();
    let published = OfflineLibrary::new(f.session.clone())
        .unwrap()
        .record(&token, &audio)
        .unwrap()
        .unwrap();

    // The same native inline ledger also appears in intelligence before
    // Forget. A live publication by itself cannot satisfy this reader.
    PlaybackQueue::new(Some(f.store.clone()), "1")
        .unwrap()
        .replace(
            vec![json!({
                "id":"123","path":audio,"download_ledger_owner":"1","download_ledger":published,
            })],
            0,
            json!(null),
        )
        .unwrap();
    let inline = f
        .client
        .player_action("1", PlayerAction::Play)
        .await
        .unwrap();
    assert!(
        f.client
            .player_lyrics("1", inline.play_id.as_deref().unwrap())
            .await
            .unwrap()
            .lines
            .is_empty()
    );

    f.client
        .play_downloads("1", vec![id.clone()], 0)
        .await
        .unwrap();
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    let files = CompletedDownloads::new(f.session.clone()).unwrap();
    let op = files.start(&id, FileAction::Forget, None).unwrap();
    assert_eq!(files.resume(&op.id).unwrap().phase, FilePhase::Complete);
    assert!(audio.exists() && sidecar.exists());
    assert!(
        OfflineLibrary::new(f.session.clone())
            .unwrap()
            .record(&token, &audio)
            .unwrap()
            .is_none()
    );
    let play = f
        .client
        .player_action("1", PlayerAction::Play)
        .await
        .unwrap();
    let play_id = play.play_id.as_deref().unwrap();
    let found = f.client.player_lyrics("1", play_id).await.unwrap();
    assert_eq!(found.lines[0].lyric, "保留的歌词");
    assert_eq!(
        f.client
            .lyric_offset("1", play_id, 1.0)
            .await
            .unwrap()
            .offset,
        0.5
    );

    // Without the receipt, the exact same bytes still support legacy detached
    // playback, but cannot authorize an additional file read.
    let kind = "download_file_operations_v1";
    let receipt = f.store.get("1", kind, &op.id).unwrap().unwrap();
    f.store.delete("1", kind, &op.id).unwrap();
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    let replay = f
        .client
        .player_action("1", PlayerAction::Play)
        .await
        .unwrap();
    let replay_id = replay.play_id.as_deref().unwrap();
    assert_ne!(replay_id, play_id);
    assert!(
        f.client
            .player_lyrics("1", replay_id)
            .await
            .unwrap()
            .lines
            .is_empty()
    );
    assert!(matches!(
        f.client.player_lyrics("1", play_id).await,
        Err(Error::StaleResponse)
    ));

    // A live decoder can still hold the old media while the path changes.
    // Same length and inode are insufficient: verify SHA before sibling read.
    f.store.put("1", kind, &op.id, &receipt).unwrap();
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    let current = f
        .client
        .player_action("1", PlayerAction::Play)
        .await
        .unwrap();
    let mut replacement = fs::read(&audio).unwrap();
    *replacement.last_mut().unwrap() ^= 1;
    fs::write(&audio, replacement).unwrap();
    fs::write(&sidecar, "[00:01]不能读取的替换附件").unwrap();
    assert!(
        f.client
            .player_lyrics("1", current.play_id.as_deref().unwrap())
            .await
            .unwrap()
            .lines
            .is_empty()
    );
    assert_eq!(f.api.count("player/url"), 0);
}

#[tokio::test]
async fn corrupt_download_requires_one_explicit_restart_before_redownloading_from_zero() {
    let f = Fixture::new();
    f.backend.audio.settings.lock().unwrap().data[0] ^= 0x20;
    let id = f.start().await;
    let failed = wait(&f.client, "1", DownloadTab::Active, |v| {
        v.tasks.first().is_some_and(|t| t.needs_transfer_restart)
    })
    .await;
    assert_eq!(failed.tasks[0].status, DownloadStatus::Error);
    assert_eq!(failed.tasks[0].error, Some("文件校验失败，需要从头下载"));
    let authorized = f.api.count("/download/url/");
    assert_eq!(authorized, 1);
    assert_eq!(f.backend.audio.calls().len(), 1);
    let retry = f
        .client
        .download_command(
            "1",
            DownloadCommand::Control {
                task_ids: vec![id.clone()],
                action: TaskControl::Resume,
            },
        )
        .await
        .unwrap();
    assert!(retry.accepted.is_empty());
    assert_eq!(retry.failed.len(), 1);
    let bulk = f
        .client
        .download_command(
            "1",
            DownloadCommand::All {
                action: TaskControl::Resume,
            },
        )
        .await
        .unwrap();
    assert!(bulk.accepted.is_empty() && bulk.failed.is_empty());
    assert_eq!(f.api.count("/download/url/"), authorized);
    assert_eq!(f.backend.audio.calls().len(), 1);

    f.backend.audio.settings.lock().unwrap().data = FLAC.to_vec();
    let command = || DownloadCommand::RestartFailedTransfer {
        task_id: id.clone(),
    };
    let restarted = f.client.download_command("1", command()).await.unwrap();
    assert_eq!(restarted.accepted, vec![id.clone()]);
    let done = wait(&f.client, "1", DownloadTab::Completed, |v| {
        v.tasks.first().is_some_and(|t| !t.cleanup_pending)
    })
    .await;
    assert!(!done.tasks[0].needs_transfer_restart);
    assert_eq!(
        fs::read(done.tasks[0].path.as_ref().unwrap()).unwrap(),
        FLAC
    );
    let calls = f.backend.audio.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].offset, 0);
    assert!(calls[1].if_range.is_none());
    assert!(f.client.download_command("1", command()).await.is_err());
    assert_eq!(f.backend.audio.calls().len(), 2);
    assert_eq!(
        fs::read(done.tasks[0].path.as_ref().unwrap()).unwrap(),
        FLAC
    );
}

#[tokio::test]
async fn completed_download_names_and_albums_use_pinyin_before_pagination() {
    let f = Fixture::new();
    let (original, _) = f.completed().await;
    let seed = f
        .store
        .get("1", "download_tasks_v1", &original)
        .unwrap()
        .unwrap();
    for (i, title, album) in [
        (1, "上海", "北京"),
        (2, "北京", "上海"),
        (3, "广州", "广州"),
    ] {
        let id = format!("{i:032x}");
        let mut value = seed.clone();
        value["id"] = json!(id);
        value["song"] = json!({"id":i,"name":format!("合成排序 {title}"),"al":{"name":album}});
        f.store.put("1", "download_tasks_v1", &id, &value).unwrap();
    }
    for (sort, expected) in [
        (DownloadSort::Name, ["2", "3", "1"]),
        (DownloadSort::Album, ["1", "3", "2"]),
    ] {
        let query = DownloadQuery {
            tab: DownloadTab::Completed,
            category: DownloadCategory::Song,
            keyword: "合成排序".into(),
            sort,
            descending: false,
            offset: 0,
            limit: 2,
        };
        for (descending, order) in [
            (false, expected),
            (true, [expected[2], expected[1], expected[0]]),
        ] {
            let mut ids = vec![];
            for offset in [0, 2] {
                let view = f
                    .client
                    .downloads(
                        "1",
                        DownloadQuery {
                            descending,
                            offset,
                            ..query.clone()
                        },
                    )
                    .await
                    .unwrap();
                assert_eq!(view.total, 3);
                ids.extend(view.tasks.into_iter().map(|task| task.track.id));
            }
            assert_eq!(ids, order);
        }
    }
}
