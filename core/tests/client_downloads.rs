#[path = "support/download_collection.rs"]
mod download_collection;
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
    membership::Quality,
    playback::{controller::PlaybackStatus, engine::MediaInput},
    protocol::http::ApiOrigin,
};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
const FLAC: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");
struct Fixture {
    root: tempfile::TempDir,
    session: Arc<AccountSession>,
    client: MusicClient,
    api: Arc<support::download::Server>,
    backend: Arc<support::download::Backend>,
    device: Arc<playback_support::Device>,
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
        let device = Arc::new(playback_support::Device::default());
        device
            .pinned_search_supported
            .store(true, Ordering::Release);
        let native = device.clone();
        let client = MusicClient::with_session(session.clone(), registry)
            .with_download_backend(backend.clone())
            .with_audio_factory(move || Ok(playback_support::audio_engine(native.clone())));
        Self {
            root,
            session,
            client,
            api,
            backend,
            device,
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

async fn followed_header(client: &MusicClient) -> impl Fn() -> PlaySelection + use<> {
    let value = serde_json::to_value(client.followed_new_song_latest("1").await.unwrap()).unwrap();
    let revision = value["value"]["revision"].as_str().unwrap().to_owned();
    move || PlaySelection::FollowLatest {
        revision: revision.clone(),
        quality: cumulus_core::membership::Quality::Standard,
    }
}

#[tokio::test]
async fn followed_latest_can_use_the_exact_published_song_without_online_rights() {
    use cumulus_core::playback::controller::PlaybackStatus;
    let f = Fixture::new();
    let (id, completed) = f.completed().await;
    let path = PathBuf::from(completed.tasks[0].path.as_ref().unwrap());
    let online = f
        .client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["123".into()],
                index: 0,
                quality: cumulus_core::membership::Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(online.download_task_id, None);
    *f.api.follow_latest.lock().unwrap() = json!([
        {"id":123,"name":"合成已下载首曲","dt":1000,"privilege":{"pl":0,"flag":0}}
    ]);
    let choice = followed_header(&f.client).await;
    let local = f.client.play_selection("1", choice()).await.unwrap();
    assert_eq!(local.download_task_id.as_deref(), Some(id.as_str()));
    assert_ne!(
        local.play_id, online.play_id,
        "old online grant was not reused"
    );
    assert_eq!(local.state, PlaybackStatus::Playing);
    assert_eq!(local.actual_quality.as_deref(), Some("lossless"));
    assert_eq!(local.track.as_ref().unwrap().name, "合成已下载首曲");
    let paused = f
        .client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let repeated = f.client.play_selection("1", choice()).await.unwrap();
    assert_eq!(repeated.state, PlaybackStatus::Playing);
    assert_eq!(repeated.play_id, local.play_id);
    assert_eq!(repeated.entry_id, local.entry_id);
    assert_ne!(repeated.queue_revision, paused.queue_revision);
    let before = f
        .client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    // A corrupt same-name file must not trigger either an online retry or the
    // next track. The previously authorized paused decoder stays attached.
    fs::write(&path, b"corrupt replacement").unwrap();
    assert!(matches!(
        f.client.play_selection("1", choice()).await,
        Err(Error::State(
            "the latest release's first song is unavailable"
        ))
    ));
    let after = f.client.player_snapshot("1").await.unwrap();
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.position, before.position);
    assert_eq!(after.queue_revision, before.queue_revision);
    assert_eq!(f.api.count("/api/song/enhance/player/url/v1"), 1);
}

fn replace_with_protected_flac(f: &Fixture, plain: &str, path: &std::path::Path) -> PathBuf {
    use cumulus_core::{
        download::offline::{OfflineLibrary, PublishedDownload},
        media::{
            integrity::inspect_file,
            ncm::{NcmWriteOptions, write_file},
        },
    };
    let encrypted = path.with_file_name("published-ncm.ncm");
    write_file(path, &encrypted, NcmWriteOptions::default()).unwrap();
    let token = f.session.token().unwrap();
    let offline = OfflineLibrary::new(f.session.clone()).unwrap();
    let integrity = inspect_file(&encrypted).unwrap();
    let song = json!({"id":123,"name":"合成会员下载","dt":1000,"privilege":{"fee":1,"flag":4}});
    let resource = json!({"id":123,"level":"lossless","fee":1,"flag":4,"payed":0});
    let record = offline
        .register(
            &token,
            &PublishedDownload {
                task_id: plain.to_owned(),
                song_id: "123".into(),
                requested_quality: Quality::Lossless,
                target: encrypted.clone(),
                song: song.clone(),
                resource: resource.clone(),
                encrypted: true,
                integrity: integrity.clone(),
            },
        )
        .unwrap();
    let mut task = f
        .store
        .get("1", "download_tasks_v1", plain)
        .unwrap()
        .unwrap();
    task["target"] = json!(record.path);
    task["encrypted"] = json!(true);
    task["song"] = song;
    task["resource"] = resource;
    task["output_integrity"] = json!(integrity);
    f.store.put("1", "download_tasks_v1", plain, &task).unwrap();
    encrypted
}

#[tokio::test]
async fn followed_latest_requires_a_current_encrypted_download_entitlement() {
    use cumulus_core::download::offline::OfflineLibrary;
    let f = Fixture::new();
    f.api.resource.lock().unwrap()["flag"] = json!(0);
    let (plain, completed) = f.completed().await;
    let membership = "/api/music-vip-membership/client/vip/info";
    assert_eq!(
        f.api.count(membership),
        0,
        "fixture starts without a renewal"
    );
    assert!(f.store.get("1", "membership", "current").unwrap().is_none());
    let path = PathBuf::from(completed.tasks[0].path.as_ref().unwrap());
    replace_with_protected_flac(&f, &plain, &path);
    let token = f.session.token().unwrap();
    let offline = OfflineLibrary::new(f.session.clone()).unwrap();
    *f.api.follow_latest.lock().unwrap() = json!([
        {"id":123,"name":"合成会员下载","dt":1000,"privilege":{"pl":0,"flag":0}}
    ]);
    let choice = followed_header(&f.client).await;
    let denial = f
        .client
        .play_selection("1", choice())
        .await
        .err()
        .expect("unlicensed file");
    assert!(
        matches!(
            denial,
            Error::State("the latest release's first song is unavailable")
        ),
        "{denial:?}"
    );
    assert_eq!(f.api.count(membership), 0);
    offline.refresh_membership(&token).await.unwrap();
    let local = f.client.play_selection("1", choice()).await.unwrap();
    assert_eq!(local.download_task_id.as_deref(), Some(plain.as_str()));
    assert_eq!(f.api.count(membership), 1);
    assert_eq!(f.api.count("/api/song/enhance/player/url/v1"), 0);
}

const SEARCH_URL: &str = "/api/song/enhance/player/url/v1";
const MEMBERSHIP_INFO: &str = "/api/music-vip-membership/client/vip/info";

fn search_row(toggle_current: bool) -> PlaySelection {
    PlaySelection::SearchResultTrack {
        id: "123".into(),
        quality: Quality::Standard,
        toggle_current,
    }
}

fn assert_search_online(f: &Fixture, level: &str) {
    assert!(matches!(
        &f.device.state.lock().unwrap().last_source,
        Some(MediaInput::Url(_))
    ));
    let calls = f.api.calls.lock().unwrap();
    let requests = calls
        .iter()
        .filter(|(path, _)| path == SEARCH_URL)
        .map(|(_, payload)| payload)
        .collect::<Vec<_>>();
    let [request] = requests.as_slice() else {
        panic!("expected one authorized search URL request");
    };
    assert_eq!(request["trialMode"], 42);
    assert_eq!(request["level"], level);
}

#[tokio::test]
async fn search_row_verified_download_keeps_catalog_and_disables_quality_switch() {
    let f = Fixture::new();
    let (_task, completed) = f.completed().await;
    let path = fs::canonicalize(completed.tasks[0].path.as_ref().unwrap()).unwrap();
    let local = f
        .client
        .play_selection("1", search_row(false))
        .await
        .unwrap();
    assert_eq!(local.state, PlaybackStatus::Playing);
    assert_eq!(local.resource_kind, Some(PlayerResourceKind::Catalog));
    assert!(local.download_task_id.is_none());
    assert_eq!(local.actual_quality.as_deref(), Some("lossless"));
    assert_eq!(local.requested_quality.as_deref(), Some("standard"));
    assert!(local.catalog_file_source);
    assert!(!local.can_change_quality);
    let wire = serde_json::to_value(&local).unwrap();
    assert_eq!(wire["catalogFileSource"], true);
    assert!(wire.get("downloadTaskId").is_none());
    assert!(wire.get("path").is_none());
    let queue = f.client.player_queue("1").await.unwrap();
    assert_eq!(queue.current, local.entry_id);
    assert_eq!(queue.tracks.len(), 1);
    assert_eq!(queue.tracks[0].track.id, "123");
    {
        let native = f.device.state.lock().unwrap();
        assert!(native.last_source.is_none());
        assert_eq!(native.last_pinned, Some((false, fs::read(path).unwrap())));
    }
    assert_eq!(f.api.count(SEARCH_URL), 0);
    assert_eq!(f.api.count(MEMBERSHIP_INFO), 0);

    let detail_reads = f.api.count("/api/v3/song/detail");
    let paused = f
        .client
        .play_selection("1", search_row(true))
        .await
        .unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    assert_eq!(paused.play_id, local.play_id);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    assert_eq!(f.api.count("/api/v3/song/detail"), detail_reads);
    assert_eq!(f.api.count(SEARCH_URL), 0);

    let expected_calls = f.api.calls.lock().unwrap().len();
    let play_id = paused.play_id.clone().unwrap();
    let revision = paused.quality_preferences.revision.clone().unwrap();
    for action in [
        PlayerAction::Quality {
            level: Quality::ExHigh,
        },
        PlayerAction::QualityFor {
            play_id: play_id.clone(),
            expected_revision: revision.clone(),
            level: Quality::ExHigh,
        },
    ] {
        assert!(matches!(
            f.client
                .player_action("1", action)
                .await
                .err()
                .unwrap()
                .root(),
            Error::State("quality requires loaded online track")
        ));
    }
    let late = f
        .client
        .player_action(
            "1",
            PlayerAction::QualityFor {
                play_id: "superseded-online-play".into(),
                expected_revision: revision.clone(),
                level: Quality::ExHigh,
            },
        )
        .await;
    assert!(matches!(late.err().unwrap().root(), Error::StaleResponse));
    assert!(matches!(
        f.client
            .player_quality_details("1", &play_id)
            .await
            .err()
            .unwrap()
            .root(),
        Error::State("quality requires loaded online track")
    ));
    assert!(matches!(
        f.client
            .player_quality_trial(
                "1",
                QualityTrialCommand::Start {
                    play_id: play_id.clone(),
                    details_revision: "old-offer".into(),
                    level: Quality::JyMaster,
                }
            )
            .await
            .err()
            .unwrap()
            .root(),
        Error::State("quality requires loaded online track")
    ));
    assert_eq!(f.api.calls.lock().unwrap().len(), expected_calls);
    let after = f.client.player_snapshot("1").await.unwrap();
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(after.play_id.as_deref(), Some(play_id.as_str()));
    assert_eq!(after.entry_id, local.entry_id);
    assert_eq!(
        after.quality_preferences.revision.as_deref(),
        Some(revision.as_str())
    );
    assert_eq!(after.requested_quality.as_deref(), Some("standard"));
    assert_eq!(after.actual_quality.as_deref(), Some("lossless"));
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    assert_eq!(f.api.count(SEARCH_URL), 0);
}

#[tokio::test]
async fn search_row_online_quality_still_authorizes_selected_level_and_preserves_clock() {
    let f = Fixture::new();
    let online = f
        .client
        .play_selection("1", search_row(false))
        .await
        .unwrap();
    assert!(online.can_change_quality);
    assert!(!online.catalog_file_source);
    assert!(
        serde_json::to_value(&online)
            .unwrap()
            .get("catalogFileSource")
            .is_none()
    );
    assert_search_online(&f, "standard");
    let paused = f
        .client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let seek = f
        .client
        .player_action("1", PlayerAction::Seek { seconds: 15.0 })
        .await
        .unwrap();
    assert_eq!(seek.state, PlaybackStatus::Paused);
    assert_eq!(seek.position, Some(15.0));
    assert_eq!(seek.play_id, paused.play_id);

    f.api.calls.lock().unwrap().clear();
    let changed = f
        .client
        .player_action(
            "1",
            PlayerAction::QualityFor {
                play_id: seek.play_id.unwrap(),
                expected_revision: seek.quality_preferences.revision.unwrap(),
                level: Quality::ExHigh,
            },
        )
        .await
        .unwrap();
    assert_eq!(changed.state, PlaybackStatus::Paused);
    assert_eq!(changed.position, Some(15.0));
    assert_eq!(changed.entry_id, online.entry_id);
    assert!(changed.can_change_quality);
    assert!(!changed.catalog_file_source);
    assert_eq!(changed.requested_quality.as_deref(), Some("exhigh"));
    assert_eq!(changed.actual_quality.as_deref(), Some("standard"));
    assert_search_online(&f, "exhigh");
}

async fn native_search_state(f: &Fixture, want: impl Fn(&PlayerView) -> bool) -> PlayerView {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut tick = tokio::time::interval(Duration::from_millis(15));
        loop {
            tick.tick().await;
            let view = f.client.player_snapshot("1").await.unwrap();
            if want(&view) {
                return view;
            }
        }
    })
    .await
    .expect("wait for the selected native search state")
}

#[tokio::test]
async fn search_row_reads_the_held_file_even_if_the_published_name_is_replaced() {
    use cumulus_core::playback::engine::EngineEvent;

    let f = Fixture::new();
    let (_, completed) = f.completed().await;
    let path = PathBuf::from(completed.tasks[0].path.as_ref().unwrap());
    let original = fs::read(&path).unwrap();
    f.device.hold_load.store(true, Ordering::Release);
    let (selected, ()) = tokio::join!(f.client.play_selection("1", search_row(false)), async {
        tokio::time::timeout(Duration::from_secs(5), f.device.started.notified())
            .await
            .expect("the native adapter owns the checked descriptor");
        fs::rename(&path, path.with_file_name("still-open.flac")).unwrap();
        // Same size and pathname; a stat-only check cannot prove equality.
        let mut changed = original.clone();
        *changed.last_mut().unwrap() ^= 0x55;
        fs::write(&path, changed).unwrap();
        f.device.hold_load.store(false, Ordering::Release);
    });
    let playing = selected.unwrap();
    assert!(playing.catalog_file_source);
    assert_eq!(playing.state, PlaybackStatus::Playing);
    let entry = {
        let mut native = f.device.state.lock().unwrap();
        assert!(native.last_source.is_none());
        assert_eq!(native.last_pinned, Some((false, original)));
        let entry = native.view.entry_id.unwrap();
        native
            .view
            .events
            .push(EngineEvent::OutputReload { entry_id: entry });
        entry
    };
    assert!(entry > 0);
    // Explicit native reopen must hash a new descriptor from the same
    // publication. It cannot reuse the old cursor or accept the changed path.
    let failed = native_search_state(&f, |view| view.state == PlaybackStatus::Error).await;
    assert!(!failed.catalog_file_source);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    assert_eq!(f.api.count(SEARCH_URL), 0);
}

#[tokio::test]
async fn search_row_valid_native_reopen_keeps_the_same_publication_and_pause() {
    use cumulus_core::playback::engine::EngineEvent;

    let f = Fixture::new();
    f.completed().await;
    f.client
        .play_selection("1", search_row(false))
        .await
        .unwrap();
    f.client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let previous = f
        .client
        .player_action("1", PlayerAction::Seek { seconds: 12.0 })
        .await
        .unwrap();
    {
        let mut native = f.device.state.lock().unwrap();
        let entry_id = native.view.entry_id.unwrap();
        native
            .view
            .events
            .push(EngineEvent::OutputReload { entry_id });
    }
    let resumed = native_search_state(&f, |view| {
        view.state == PlaybackStatus::Paused && view.play_id != previous.play_id
    })
    .await;
    assert!(resumed.catalog_file_source);
    assert_eq!(resumed.entry_id, previous.entry_id);
    assert_eq!(resumed.position, Some(12.0));
    assert_eq!(f.device.state.lock().unwrap().loads, 2);
    assert_eq!(f.api.count(SEARCH_URL), 0);
}

#[tokio::test]
async fn search_row_ledger_or_queue_revocation_never_unpauses_the_prepared_file() {
    use cumulus_core::download::offline::OfflineLibrary;

    for revoke_queue in [false, true] {
        let f = Fixture::new();
        let (_, completed) = f.completed().await;
        let path = completed.tasks[0].path.as_ref().unwrap();
        let offline = OfflineLibrary::new(f.session.clone()).unwrap();
        let token = f.session.token().unwrap();
        let record = offline.record(&token, path).unwrap().unwrap();
        f.device.hold_load.store(true, Ordering::Release);
        let (selected, ()) = tokio::join!(f.client.play_selection("1", search_row(false)), async {
            tokio::time::timeout(Duration::from_secs(5), f.device.started.notified())
                .await
                .expect("paused native load has started");
            if revoke_queue {
                f.store
                    .put(
                        "1",
                        "playback",
                        "queue",
                        &json!({
                            "items": [], "order": [], "current": null, "mode": "order_loop"
                        }),
                    )
                    .unwrap();
            } else {
                offline.forget(&token, &record).unwrap();
            }
            f.device.hold_load.store(false, Ordering::Release);
        });
        let error = selected.err().expect("obsolete publication cannot start");
        if revoke_queue {
            assert!(matches!(
                error.root(),
                Error::QueueChanged | Error::Cancelled
            ));
        } else {
            assert!(matches!(
                error.root(),
                Error::State("published search download changed")
            ));
        }
        let view = f.client.player_snapshot("1").await.unwrap();
        assert!(!view.catalog_file_source);
        assert_ne!(view.state, PlaybackStatus::Playing);
        assert!(!f.device.state.lock().unwrap().pauses.contains(&false));
        assert_eq!(f.api.count(SEARCH_URL), 0);
    }
}

#[tokio::test]
async fn search_row_host_without_pinned_playback_uses_only_authorized_online_source() {
    let f = Fixture::new();
    f.completed().await;
    f.device
        .pinned_search_supported
        .store(false, Ordering::Release);
    let view = f
        .client
        .play_selection("1", search_row(false))
        .await
        .unwrap();
    assert!(!view.catalog_file_source);
    assert!(view.can_change_quality);
    assert_search_online(&f, "standard");
    assert!(f.device.state.lock().unwrap().last_pinned.is_none());
}

#[tokio::test]
async fn search_row_unsupported_pinned_format_is_not_sent_to_a_path_decoder() {
    use cumulus_core::{
        download::offline::{OfflineLibrary, PublishedDownload},
        media::integrity::inspect_file,
    };

    let f = Fixture::new();
    let (task_id, completed) = f.completed().await;
    let path =
        PathBuf::from(completed.tasks[0].path.as_ref().unwrap()).with_file_name("unsupported.ogg");
    fs::write(&path, b"OggS sealed non-pinned format fixture").unwrap();
    let integrity = inspect_file(&path).unwrap();
    let record = OfflineLibrary::new(f.session.clone())
        .unwrap()
        .register(
            &f.session.token().unwrap(),
            &PublishedDownload {
                task_id: task_id.clone(),
                song_id: "123".into(),
                requested_quality: Quality::Standard,
                target: path,
                song: json!({"id":123}),
                resource: json!({"id":123,"level":"standard"}),
                encrypted: false,
                integrity: integrity.clone(),
            },
        )
        .unwrap();
    let mut task = f
        .store
        .get("1", "download_tasks_v1", &task_id)
        .unwrap()
        .unwrap();
    task["target"] = json!(record.path);
    task["output_integrity"] = json!(integrity);
    f.store
        .put("1", "download_tasks_v1", &task_id, &task)
        .unwrap();

    let view = f
        .client
        .play_selection("1", search_row(false))
        .await
        .unwrap();
    assert!(!view.catalog_file_source);
    assert_search_online(&f, "standard");
    assert!(f.device.state.lock().unwrap().last_pinned.is_none());
}

#[tokio::test]
async fn search_row_missing_or_corrupt_download_uses_only_authorized_online_fallback() {
    for missing in [true, false] {
        let f = Fixture::new();
        let (_, completed) = f.completed().await;
        let path = completed.tasks[0].path.as_ref().unwrap();
        if missing {
            fs::remove_file(path).unwrap();
        } else {
            fs::write(path, b"changed since publication").unwrap();
        }
        let view = f
            .client
            .play_selection("1", search_row(false))
            .await
            .unwrap();
        assert_eq!(view.state, PlaybackStatus::Playing);
        assert_eq!(view.resource_kind, Some(PlayerResourceKind::Catalog));
        assert_eq!(view.actual_quality.as_deref(), Some("standard"));
        assert_eq!(f.device.state.lock().unwrap().loads, 1);
        assert_eq!(f.api.count(MEMBERSHIP_INFO), 0);
        assert_search_online(&f, "standard");
    }
}

#[tokio::test]
async fn search_row_never_guesses_between_two_completed_publications() {
    let f = Fixture::new();
    let (task, _) = f.completed().await;
    let mut duplicate = f
        .store
        .get("1", "download_tasks_v1", &task)
        .unwrap()
        .unwrap();
    let second = "a4a25a7c7e094fa8855cbf2fed026ec1";
    duplicate["id"] = json!(second);
    f.store
        .put("1", "download_tasks_v1", second, &duplicate)
        .unwrap();

    let view = f
        .client
        .play_selection("1", search_row(false))
        .await
        .unwrap();
    assert_eq!(view.state, PlaybackStatus::Playing);
    assert_search_online(&f, "standard");
}

#[tokio::test]
async fn search_row_encrypted_download_needs_current_offline_rights_without_renewal() {
    use cumulus_core::download::offline::OfflineLibrary;
    for (known, active) in [(false, false), (true, false), (true, true)] {
        let f = Fixture::new();
        f.api.resource.lock().unwrap()["flag"] = json!(0);
        let (task, completed) = f.completed().await;
        let original = PathBuf::from(completed.tasks[0].path.as_ref().unwrap());
        let encrypted = replace_with_protected_flac(&f, &task, &original);
        if known {
            if !active {
                f.api.membership_expiry.store(1_999_999, Ordering::Release);
            }
            OfflineLibrary::new(f.session.clone())
                .unwrap()
                .refresh_membership(&f.session.token().unwrap())
                .await
                .unwrap();
        }
        let view = f
            .client
            .play_selection("1", search_row(false))
            .await
            .unwrap();
        assert_eq!(view.state, PlaybackStatus::Playing);
        if active {
            let native = f.device.state.lock().unwrap();
            assert!(native.last_source.is_none());
            assert_eq!(
                native.last_pinned,
                Some((true, fs::read(encrypted).unwrap()))
            );
            assert_eq!(f.api.count(SEARCH_URL), 0);
        } else {
            assert_search_online(&f, "standard");
        }
        assert_eq!(f.api.count(MEMBERSHIP_INFO), usize::from(known));
    }
}

#[tokio::test]
async fn search_row_does_not_load_a_protected_download_after_online_denial() {
    let f = Fixture::new();
    f.api.resource.lock().unwrap()["flag"] = json!(0);
    let (task, completed) = f.completed().await;
    let path = PathBuf::from(completed.tasks[0].path.as_ref().unwrap());
    replace_with_protected_flac(&f, &task, &path);
    *f.api.play.lock().unwrap() = Ok(json!({"code":200,"data":[{"id":123,"code":403}]}));

    let result = f.client.play_selection("1", search_row(false)).await;
    assert!(result.err().unwrap().is_service_code(403));
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
    assert_eq!(f.api.count(SEARCH_URL), 1);
    assert_eq!(f.api.count(MEMBERSHIP_INFO), 0);
}

#[tokio::test]
async fn search_row_download_preference_is_neither_cross_account_nor_other_search_source() {
    let f = Fixture::new();
    f.completed().await;
    let hot = f
        .client
        .play_selection(
            "1",
            PlaySelection::SearchHotTrack {
                id: "123".into(),
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(hot.state, PlaybackStatus::Playing);
    assert!(matches!(
        &f.device.state.lock().unwrap().last_source,
        Some(MediaInput::Url(_))
    ));
    let before = f.session.token().unwrap();
    f.session
        .adopt(f.api.clone(), json!({"userId":2}), &before)
        .unwrap();
    let next = f.session.token().unwrap();
    let previous_urls = f.api.count(SEARCH_URL);
    assert!(matches!(
        f.client.play_selection("1", search_row(false)).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.api.count(SEARCH_URL), previous_urls);
    f.api.calls.lock().unwrap().clear();
    let view = f
        .client
        .play_selection(&next.generation.to_string(), search_row(false))
        .await
        .unwrap();
    assert_eq!(view.state, PlaybackStatus::Playing);
    assert_eq!(view.actual_quality.as_deref(), Some("standard"));
    assert_search_online(&f, "standard");
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
async fn explicit_download_quality_refresh_recovers_unavailable_membership_without_media_reads() {
    use std::sync::atomic::Ordering;
    const MEMBERSHIP: &str = "/api/music-vip-membership/client/vip/info";
    let f = Fixture::new();
    let before = f.client.download_quality_preferences("1").unwrap();
    assert!(f.api.calls.lock().unwrap().is_empty());
    f.api.membership_fails.store(true, Ordering::Release);
    assert!(matches!(
        f.client.refresh_download_quality_preferences("1").await,
        Err(Error::Transport(_))
    ));
    let still_unknown = f.client.download_quality_preferences("1").unwrap();
    assert_eq!(still_unknown.revision, before.revision);
    f.api.membership_fails.store(false, Ordering::Release);
    let recovered = f
        .client
        .refresh_download_quality_preferences("1")
        .await
        .unwrap();
    assert_ne!(recovered.revision, before.revision);
    assert_eq!(recovered.options.quality, before.options.quality);
    assert!(
        serde_json::to_value(&recovered).unwrap()["choices"]
            .as_array()
            .unwrap()
            .contains(&json!({"quality":"lossless","access":"allowed"}))
    );
    let current = f
        .client
        .refresh_download_quality_preferences("1")
        .await
        .unwrap();
    assert_eq!(current.revision, recovered.revision);
    assert!(matches!(
        f.client.refresh_download_quality_preferences("0").await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        *f.api.calls.lock().unwrap(),
        vec![(MEMBERSHIP.to_owned(), json!({"userId":"1"})); 2],
    );
}

#[tokio::test]
async fn default_download_quality_is_current_account_preference_not_media_permission() {
    use cumulus_core::membership::Quality;
    let f = Fixture::new();
    let read = || f.client.download_quality_preferences("1").unwrap();
    let access = |view: &DownloadQualityPreferences, quality: &str| {
        serde_json::to_value(view).unwrap()["choices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|choice| choice["quality"] == quality)
            .and_then(|choice| choice["access"].as_str().map(str::to_owned))
    };
    let save = |revision, options| {
        f.client
            .download_command("1", DownloadCommand::Preferences { revision, options })
    };

    let unknown = read();
    assert_eq!(access(&unknown, "lossless").as_deref(), Some("unknown"));
    assert_eq!(access(&unknown, "exhigh").as_deref(), Some("allowed"));
    assert_eq!(access(&unknown, "vivid"), None);
    let mut lossless = unknown.options.clone();
    lossless.quality = Quality::Lossless;
    assert!(matches!(
        save(unknown.revision, lossless.clone()).await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(read().options.quality, Quality::Standard);

    let saved = DownloadOptions {
        quality: Quality::Vivid,
        ..options()
    };
    f.store
        .put(
            "1",
            "download_preferences_v1",
            "options",
            &serde_json::to_value(saved).unwrap(),
        )
        .unwrap();
    let unavailable = read();
    assert_eq!(unavailable.options.quality, Quality::Vivid);
    assert_eq!(access(&unavailable, "vivid"), None);
    let mut edited = unavailable.options.clone();
    edited.tags = !edited.tags;
    save(unavailable.revision.clone(), edited.clone())
        .await
        .unwrap();
    assert_eq!(read().options.quality, Quality::Vivid);
    assert_eq!(read().options.tags, edited.tags);
    assert!(matches!(
        save(unavailable.revision, edited).await,
        Err(Error::QueueChanged)
    ));
    assert!(f.api.calls.lock().unwrap().is_empty());

    let before_membership = read().revision;
    let mut observations = f.client.subscribe_membership_observations().unwrap();
    let lifecycle = f.client.start_membership_lifecycle().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while observations.borrow_and_update().is_none() {
            observations.changed().await.unwrap();
        }
    })
    .await
    .expect("native account startup did not commit membership");
    drop(lifecycle);
    let member = read();
    assert_eq!(access(&member, "lossless").as_deref(), Some("allowed"));
    assert_eq!(access(&member, "jyeffect").as_deref(), Some("allowed"));
    assert_eq!(
        access(&member, "jymaster").as_deref(),
        Some("requires_svip")
    );
    assert!(matches!(
        save(before_membership, lossless.clone()).await,
        Err(Error::QueueChanged)
    ));
    save(member.revision, lossless).await.unwrap();
    assert_eq!(f.api.count("/api/music-vip-membership/client/vip/info"), 1);
    assert_eq!(f.api.calls.lock().unwrap().len(), 1);

    let (_, completed) = f.completed().await;
    assert_eq!(completed.tasks[0].requested_quality, "standard");
    assert_eq!(
        completed.tasks[0].actual_quality.as_deref(),
        Some("lossless")
    );
    assert_eq!(read().options.quality, Quality::Lossless);
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.download_quality_preferences("1"),
        Err(Error::StaleResponse)
    ));
    let other = f.client.download_quality_preferences("2").unwrap();
    assert_eq!(other.options.quality, Quality::Standard);
    assert_eq!(access(&other, "lossless").as_deref(), Some("unknown"));
}

#[tokio::test]
async fn native_downloads_publish_real_time_whitelist_data_and_preserve_preferences() {
    let f = Fixture::new();
    f.configure().await;
    f.client
        .download_command(
            "1",
            DownloadCommand::Preferences {
                revision: f.client.download_quality_preferences("1").unwrap().revision,
                options: options(),
            },
        )
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
async fn active_download_speed_measures_this_body_and_is_retired_on_pause_and_resume() {
    let f = Fixture::new();
    let gate = Arc::new(support::transfer::ReadGate::default());
    {
        let mut settings = f.backend.audio.settings.lock().unwrap();
        settings.gate = Some(gate.clone());
        settings.gate_after = 2048;
        settings.chunk_limit = 2048;
    }
    assert!(FLAC.len() > 2048);
    let id = f.start().await;
    tokio::time::timeout(Duration::from_secs(5), gate.reached.notified())
        .await
        .unwrap();
    let active = wait(&f.client, "1", DownloadTab::Active, |v| {
        v.tasks
            .first()
            .is_some_and(|t| t.speed_bytes_per_second.is_some_and(|n| n > 0))
    })
    .await;
    assert_eq!(active.tasks[0].status, DownloadStatus::Downloading);
    assert_eq!(active.tasks[0].id, id);
    for kind in ["media_transfers_v1", "download_tasks_v1"] {
        let raw = f.store.get("1", kind, &id).unwrap().unwrap();
        assert!(!raw.to_string().contains("speed"));
    }

    let control = |action| DownloadCommand::Control {
        task_ids: vec![id.clone()],
        action,
    };
    let pause = f
        .client
        .download_command("1", control(TaskControl::Pause))
        .await
        .unwrap();
    assert_eq!(pause.accepted.as_slice(), std::slice::from_ref(&id));
    let pending = f.client.downloads("1", Default::default()).await.unwrap();
    // The native network read is still held. A requested pause must already
    // suppress the old measured rate before the durable status catches up.
    assert_eq!(pending.tasks[0].status, DownloadStatus::Downloading);
    assert_eq!(pending.tasks[0].speed_bytes_per_second, None);
    gate.release.notify_one();
    let paused = wait(&f.client, "1", DownloadTab::Active, |v| {
        v.tasks
            .first()
            .is_some_and(|t| t.status == DownloadStatus::Paused)
    })
    .await;
    assert_eq!(paused.tasks[0].bytes, 2048);
    assert_eq!(paused.tasks[0].speed_bytes_per_second, None);

    let resumed = Arc::new(support::transfer::ReadGate::default());
    {
        let mut settings = f.backend.audio.settings.lock().unwrap();
        settings.gate = Some(resumed.clone());
        settings.gate_after = 0;
    }
    let resume = f
        .client
        .download_command("1", control(TaskControl::Resume))
        .await
        .unwrap();
    assert_eq!(resume.accepted.as_slice(), std::slice::from_ref(&id));
    tokio::time::timeout(Duration::from_secs(5), resumed.reached.notified())
        .await
        .unwrap();
    let running = f.client.downloads("1", Default::default()).await.unwrap();
    assert_eq!(running.tasks[0].speed_bytes_per_second, Some(0));
    assert_eq!(f.backend.audio.calls().last().unwrap().offset, 2048);
    resumed.release.notify_one();
    let complete = wait(&f.client, "1", DownloadTab::Completed, |v| {
        v.completed_count == 1
    })
    .await;
    assert_eq!(complete.tasks[0].speed_bytes_per_second, None);
    assert_eq!(complete.tasks[0].progress, Some(1.0));
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
async fn completed_file_reveal_checks_the_published_bytes_and_exact_leaf() {
    let f = Fixture::new();
    let (id, view) = f.completed().await;
    let path = PathBuf::from(view.tasks[0].path.as_ref().unwrap());
    let calls = f.backend.audio.calls().len();
    let cue = path.with_extension("cue");
    fs::write(&cue, b"FILE \"synthetic.flac\" WAVE\n").unwrap();
    let reveal = f
        .client
        .download_file_reveal("1", id.clone())
        .await
        .unwrap();
    assert_eq!(reveal.path(), path);
    assert_ne!(reveal.path(), cue);
    reveal.check_current().unwrap();

    let mut changed = FLAC.to_vec();
    changed[0] ^= 1;
    fs::write(&path, changed).unwrap();
    assert!(matches!(
        f.client.download_file_reveal("1", id.clone()).await,
        Err(Error::Media(_))
    ));
    fs::write(&path, FLAC).unwrap();
    let reveal = f
        .client
        .download_file_reveal("1", id.clone())
        .await
        .unwrap();
    let original = path.with_extension("original");
    fs::rename(&path, &original).unwrap();
    fs::write(&path, FLAC).unwrap();
    assert!(reveal.check_current().is_err());
    assert!(f.client.download_file_reveal("1", id).await.is_err());
    assert_eq!(f.backend.audio.calls().len(), calls);
}

#[tokio::test]
async fn queued_download_reveals_its_own_publication_without_a_same_song_fallback() {
    use cumulus_core::download::service::{
        DownloadDestination, DownloadRunOptions, DownloadService,
    };

    let f = Fixture::new();
    let (first, view) = f.completed().await;
    let first_path = PathBuf::from(view.tasks[0].path.as_ref().unwrap());
    let second_path = first_path
        .parent()
        .unwrap()
        .join("alternate same song.flac");
    let service =
        DownloadService::with_backend(f.session.clone(), f.backend.clone(), 1 << 24).unwrap();
    let second = service
        .create(
            &json!("123"),
            DownloadDestination::Path {
                path: second_path.clone(),
            },
            options(),
        )
        .unwrap();
    let completed = service
        .run(&second.id, DownloadRunOptions::default())
        .await
        .unwrap();
    assert_eq!(completed.status, DownloadStatus::Complete);

    let started = f
        .client
        .play_selection(
            "1",
            PlaySelection::Downloads {
                query: DownloadQuery {
                    tab: DownloadTab::Completed,
                    category: DownloadCategory::Song,
                    ..Default::default()
                },
                start_id: Some(first),
            },
        )
        .await
        .unwrap();
    let queue = f.client.player_queue("1").await.unwrap();
    let first_entry = queue.current.as_deref().unwrap();
    let second_entry = &queue
        .tracks
        .iter()
        .find(|entry| entry.entry_id != first_entry)
        .unwrap()
        .entry_id;
    assert!(queue.tracks.iter().all(|row| matches!(
        row.download_menu,
        Some(QueueDownloadMenuKind::Song)
    ) && !row.local_menu
        && !row.catalog_menu));
    let first_file = f
        .client
        .player_queue_download_file_reveal("1", &queue.revision, first_entry)
        .await
        .unwrap();
    let second_file = f
        .client
        .player_queue_download_file_reveal("1", &queue.revision, second_entry)
        .await
        .unwrap();
    assert_eq!(first_file.path(), first_path);
    assert_eq!(second_file.path(), second_path);
    first_file.check_current(&f.client).await.unwrap();
    second_file.check_current(&f.client).await.unwrap();
    assert!(matches!(
        f.client
            .player_queue_download_file_reveal("1", &queue.revision, &second.id)
            .await,
        Err(Error::InvalidInput(_))
    ));

    let updated = f
        .client
        .player_action(
            "1",
            PlayerAction::QueueFor {
                expected_queue_revision: queue.revision,
                expected_room_id: None,
                edit: QueueEdit::Remove {
                    entry_ids: vec![second_entry.clone()],
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.play_id, started.play_id);
    assert!(matches!(
        first_file.check_current(&f.client).await,
        Err(Error::QueueChanged)
    ));
    let mut changed = fs::read(&first_path).unwrap();
    changed[0] ^= 1;
    fs::write(&first_path, changed).unwrap();
    let request_count = f.backend.audio.calls().len();
    assert!(matches!(
        f.client
            .player_queue_download_file_reveal("1", &updated.queue_revision, first_entry)
            .await,
        Err(Error::State("queued download file is unavailable"))
    ));
    assert_eq!(fs::read(second_path).unwrap(), FLAC);
    assert_eq!(f.backend.audio.calls().len(), request_count);

    let old = f.session.token().unwrap();
    f.session
        .adopt(f.api.clone(), json!({"userId":2}), &old)
        .unwrap();
    assert!(matches!(
        second_file.check_current(&f.client).await,
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn catalog_queue_location_keeps_the_offered_source_and_rejects_ambiguous_mappings() {
    use cumulus_core::library::{AudioMetadata, LocalLibrary, probe::AudioProbe};
    use std::path::Path;

    struct Probe;
    impl AudioProbe for Probe {
        fn inspect(&self, path: &Path) -> cumulus_core::Result<AudioMetadata> {
            Ok(AudioMetadata {
                title: path.file_stem().unwrap().to_string_lossy().into(),
                duration: 45.0,
                album: String::new(),
                artists: Vec::new(),
                format: Some("flac".into()),
                container: "flac".into(),
                bitrate: None,
                sample_rate: Some(8000),
                channels: Some(1),
                cover_bytes: None,
            })
        }
    }

    let f = Fixture::new();
    let (id, view) = f.completed().await;
    let original = PathBuf::from(view.tasks[0].path.as_ref().unwrap());
    let local_root = f.root.path().join("mapped");
    fs::create_dir(&local_root).unwrap();
    for name in ["mapped original.flac", "alternate local.flac"] {
        fs::write(local_root.join(name), FLAC).unwrap();
    }
    let library = LocalLibrary::with_probe(f.store.clone(), Arc::new(Probe)).unwrap();
    library.scan(&local_root).unwrap();
    let files = library.list(Default::default()).unwrap();
    let item = |name| {
        files
            .iter()
            .find(|item| item.path.file_name().unwrap() == name)
            .unwrap()
    };
    let mapped = item("mapped original.flac");
    let alternate = item("alternate local.flac");
    library
        .map_online(&f.session, &mapped.id, &json!("123"))
        .unwrap();
    f.api.catalog_menu_enabled.store(true, Ordering::Release);
    f.client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["123".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let queue = f.client.player_queue("1").await.unwrap();
    let entry = queue.current.as_deref().unwrap();
    let menu = f
        .client
        .player_queue_catalog_menu("1", &queue.revision, entry)
        .await
        .unwrap();
    assert!(menu.downloaded);
    assert_eq!(menu.comment_count, Some(9));
    let ticket = menu.directory_receipt.unwrap();
    assert!(matches!(
        f.client
            .player_queue_catalog_file_reveal("1", &queue.revision, &id, &ticket)
            .await,
        Err(Error::QueueChanged)
    ));
    let download = f
        .client
        .player_queue_catalog_file_reveal("1", &queue.revision, entry, &ticket)
        .await
        .unwrap();
    assert_eq!(download.path(), original);
    download.check_current(&f.client).await.unwrap();
    assert!(matches!(
        f.client
            .player_queue_catalog_file_reveal("1", &queue.revision, entry, &ticket)
            .await,
        Err(Error::QueueChanged)
    ));

    let failed = f
        .client
        .player_queue_catalog_menu("1", &queue.revision, entry)
        .await
        .unwrap()
        .directory_receipt
        .unwrap();
    let mut bytes = fs::read(&original).unwrap();
    bytes[0] ^= 1;
    fs::write(&original, bytes).unwrap();
    assert!(
        f.client
            .player_queue_catalog_file_reveal("1", &queue.revision, entry, &failed)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&mapped.path).unwrap(), FLAC);
    fs::write(&original, FLAC).unwrap();

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
    let queue = f.client.player_queue("1").await.unwrap();
    let entry = queue.current.as_deref().unwrap();
    let menu = f
        .client
        .player_queue_catalog_menu("1", &queue.revision, entry)
        .await
        .unwrap();
    assert!(!menu.downloaded);
    let file = f
        .client
        .player_queue_catalog_file_reveal(
            "1",
            &queue.revision,
            entry,
            &menu.directory_receipt.unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(file.path(), mapped.path);
    file.check_current(&f.client).await.unwrap();
    library
        .map_online(&f.session, &alternate.id, &json!("123"))
        .unwrap();
    let ambiguous = f
        .client
        .player_queue_catalog_menu("1", &queue.revision, entry)
        .await
        .unwrap();
    assert!(ambiguous.directory_receipt.is_none());
    library
        .map_online(&f.session, &mapped.id, &json!("124"))
        .unwrap();
    assert!(matches!(
        file.check_current(&f.client).await,
        Err(Error::QueueChanged)
    ));
}

#[tokio::test]
async fn completed_file_reveal_refuses_a_stale_account_and_a_missing_ledger() {
    use cumulus_core::download::offline::OfflineLibrary;

    let f = Fixture::new();
    let (id, _) = f.completed().await;
    let reveal = f
        .client
        .download_file_reveal("1", id.clone())
        .await
        .unwrap();
    let token = f.session.token().unwrap();
    let offline = OfflineLibrary::new(f.session.clone()).unwrap();
    let record = offline.record(&token, reveal.path()).unwrap().unwrap();
    offline.forget(&token, &record).unwrap();
    assert!(reveal.check_current().is_err());
    assert!(
        f.client
            .download_file_reveal("1", id.clone())
            .await
            .is_err()
    );
    assert!(reveal.path().is_file());

    f.session
        .adopt(f.api.clone(), json!({"userId":2}), &token)
        .unwrap();
    assert!(matches!(reveal.check_current(), Err(Error::StaleResponse)));
    assert!(matches!(
        f.client.download_file_reveal("1", id.clone()).await,
        Err(Error::StaleResponse)
    ));
    let current = f.client.account().unwrap().epoch;
    assert!(f.client.download_file_reveal(&current, id).await.is_err());
}

#[tokio::test]
async fn completed_file_reveal_selects_the_original_ncm_without_renewing_rights() {
    use cumulus_core::download::service::OutputContainer;

    let f = Fixture::new();
    f.configure().await;
    let started = f
        .client
        .download_command(
            "1",
            DownloadCommand::Start {
                song_ids: vec!["123".into()],
                options: DownloadOptions {
                    container: OutputContainer::Ncm,
                    ..options()
                },
            },
        )
        .await
        .unwrap();
    let view = wait(&f.client, "1", DownloadTab::Completed, |v| {
        v.completed_count == 1 && !v.tasks[0].cleanup_pending
    })
    .await;
    let path = PathBuf::from(view.tasks[0].path.as_ref().unwrap());
    let requests = f.api.count(MEMBERSHIP_INFO);
    let calls = f.backend.audio.calls().len();
    let reveal = f
        .client
        .download_file_reveal("1", started.accepted[0].clone())
        .await
        .unwrap();
    reveal.check_current().unwrap();
    assert_eq!(reveal.path(), path);
    assert_eq!(path.extension().unwrap(), "ncm");
    assert!(fs::read(path).unwrap().starts_with(b"CTENFDAM"));
    assert_eq!(f.api.count(MEMBERSHIP_INFO), requests);
    assert_eq!(f.backend.audio.calls().len(), calls);
}

#[tokio::test]
async fn rename_move_directory_and_delete_run_from_native_commands_without_reauthorization() {
    let f = Fixture::new();
    let (id, before) = f.completed().await;
    let old = PathBuf::from(before.tasks[0].path.as_ref().unwrap());
    let old_reveal = f
        .client
        .download_file_reveal("1", id.clone())
        .await
        .unwrap();
    assert_eq!(old_reveal.path(), old);
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
    assert!(old_reveal.check_current().is_err());
    let moved_reveal = f
        .client
        .download_file_reveal("1", id.clone())
        .await
        .unwrap();
    assert_eq!(moved_reveal.path(), path);
    moved_reveal.check_current().unwrap();
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
    assert!(moved_reveal.check_current().is_err());
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
async fn confirmed_clear_covers_its_full_snapshot_and_does_not_cancel_a_late_task() {
    use cumulus_core::download::service::{DownloadAction, DownloadService};
    let f = Fixture::new();
    // Test setup controls each transient Queued → Paused transition directly.
    let _scheduler = cumulus_core::storage::OperationLock::acquire(
        f.store.directory(),
        "download-scheduler",
        "1",
    )
    .unwrap();
    f.configure().await;
    let service =
        DownloadService::with_backend(f.session.clone(), f.backend.clone(), 512 * 1024 * 1024)
            .unwrap();
    let paused = || {
        let task = service.create_default(&json!(123), options()).unwrap();
        service.request(&task.id, DownloadAction::Pause).unwrap()
    };
    for _ in 0..103 {
        paused();
    }
    let before = f.client.downloads("1", Default::default()).await.unwrap();
    assert_eq!(before.active_count, 103);
    assert_eq!(before.tasks.len(), 100);

    let plan = f.client.download_clear_prepare("1").await.unwrap();
    assert_eq!(plan.count, 103);
    let receipt = plan.receipt.unwrap();
    assert!(
        service
            .list()
            .unwrap()
            .iter()
            .all(|t| t.status == DownloadStatus::Paused)
    );
    let late = paused();

    // Reject the old all/cancel command even if a renderer supplies it.
    assert!(matches!(
        f.client
            .download_command(
                "1",
                DownloadCommand::All {
                    action: TaskControl::Cancel
                }
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    let result = f
        .client
        .download_command(
            "1",
            DownloadCommand::ConfirmClear {
                receipt: receipt.clone(),
            },
        )
        .await
        .unwrap();
    assert!(result.failed.is_empty());
    assert_eq!(result.accepted.len(), 103);
    assert!(!result.accepted.contains(&late.id));
    assert!(
        f.client
            .download_command("1", DownloadCommand::ConfirmClear { receipt })
            .await
            .is_err()
    );
    wait(&f.client, "1", DownloadTab::Active, |v| v.active_count == 1).await;
    let tasks = service.list().unwrap();
    assert_eq!(tasks.len(), 104);
    assert!(
        tasks
            .iter()
            .filter(|t| t.id != late.id)
            .all(|t| t.status == DownloadStatus::Cancelled && !t.work.exists())
    );
    let preserved = service.get(&late.id).unwrap();
    assert_eq!(preserved.status, DownloadStatus::Paused);
    assert!(preserved.work.is_dir());
    assert!(f.backend.audio.calls().is_empty());
}

#[tokio::test]
async fn clear_receipt_is_one_visit_and_skips_a_task_completed_before_confirmation() {
    use cumulus_core::download::service::{DownloadAction, DownloadRunOptions, DownloadService};
    let f = Fixture::new();
    // Test setup controls each transient Queued → Paused transition directly.
    let _scheduler = cumulus_core::storage::OperationLock::acquire(
        f.store.directory(),
        "download-scheduler",
        "1",
    )
    .unwrap();
    f.configure().await;
    let service =
        DownloadService::with_backend(f.session.clone(), f.backend.clone(), 512 * 1024 * 1024)
            .unwrap();
    let task = service.create_default(&json!(123), options()).unwrap();
    service.request(&task.id, DownloadAction::Pause).unwrap();
    let first = f
        .client
        .download_clear_prepare("1")
        .await
        .unwrap()
        .receipt
        .unwrap();
    let current = f
        .client
        .download_clear_prepare("1")
        .await
        .unwrap()
        .receipt
        .unwrap();
    f.client.download_clear_discard("1", &first).unwrap();
    let command = |receipt| DownloadCommand::ConfirmClear { receipt };
    assert!(
        f.client
            .download_command("1", command(first))
            .await
            .is_err()
    );
    assert!(
        f.client
            .download_command("2", command(current.clone()))
            .await
            .is_err()
    );
    assert_eq!(
        service.get(&task.id).unwrap().status,
        DownloadStatus::Paused
    );

    f.client.download_clear_discard("1", &current).unwrap();
    assert!(
        f.client
            .download_command("1", command(current))
            .await
            .is_err()
    );
    assert_eq!(
        service.get(&task.id).unwrap().status,
        DownloadStatus::Paused
    );

    let ready = f
        .client
        .download_clear_prepare("1")
        .await
        .unwrap()
        .receipt
        .unwrap();
    service.enqueue(&task.id).unwrap();
    let complete = service
        .run(&task.id, DownloadRunOptions::default())
        .await
        .unwrap();
    assert_eq!(complete.status, DownloadStatus::Complete);
    let result = f
        .client
        .download_command("1", command(ready))
        .await
        .unwrap();
    assert!(result.accepted.is_empty() && result.failed.is_empty());
    assert_eq!(
        service.get(&task.id).unwrap().status,
        DownloadStatus::Complete
    );
    assert_eq!(fs::read(complete.target.unwrap()).unwrap(), FLAC);
    let empty = f.client.download_clear_prepare("1").await.unwrap();
    assert_eq!(empty.count, 0);
    assert!(empty.receipt.is_none());
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
async fn downloaded_voice_header_comes_from_native_task_origin_not_raw_song_metadata() {
    use cumulus_core::{
        download::service::{DownloadOrigin, DownloadTask},
        playback::controller::PlaybackStatus,
        podcast::Episode,
    };

    let f = Fixture::new();
    let (task_id, _) = f.completed().await;
    let mut task: DownloadTask = serde_json::from_value(
        f.store
            .get("1", "download_tasks_v1", &task_id)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let description = "  已下载节目的简介\n\n\t第二段 🎧\r\n";
    let episode = Episode::from_official(&json!({
        "id":71,"name":"合成下载声音","mainSong":{"id":123,"duration":1000},
        "description":description,
        "coverUrl":"https://p1.music.126.net/episode.jpg?key=PRIVATE_ARTWORK#PRIVATE_FRAGMENT",
        "listenerCount":"0","playCount":700,
        "createTime":1735787045000_u64,
        "dj":{"userId":42,"nickname":"合成主播","avatarUrl":"PRIVATE_AVATAR"},
        "radio":{"id":"9007199254740993","name":"该声音所属播客","programCount":1,
            "picUrl":"PRIVATE_PODCAST_COVER"},
        "authorization":"PRIVATE_TOKEN"
    }))
    .unwrap();
    let mut metadata = episode.download_metadata().unwrap();
    // An invalid foreign voice ID must not fail mapping before typed origin
    // normalization, either in the download list or in the playback producer.
    metadata["voice_id"] = json!("not-a-decimal-id");
    task.song = Some(metadata);
    let calls = f.api.calls.lock().unwrap().len();
    let media_calls = f.backend.audio.calls().len();
    let select = |category| PlaySelection::Downloads {
        query: DownloadQuery {
            tab: DownloadTab::Completed,
            category,
            ..Default::default()
        },
        start_id: Some(task_id.clone()),
    };
    let mut previous_play_id = None;
    for origin in [
        DownloadOrigin::Song,
        DownloadOrigin::Cloud {
            file_name: "synthetic.flac".into(),
            file_size: FLAC.len() as u64,
        },
    ] {
        for raw_voice_id in [json!("71"), json!("not-a-decimal-id")] {
            // Identical foreign episode JSON in either audio task kind cannot
            // grant voice layout, with either a valid or malformed raw ID.
            task.song.as_mut().unwrap()["voice_id"] = raw_voice_id;
            task.origin = origin.clone();
            let stored = serde_json::to_value(&task).unwrap();
            f.store
                .put("1", "download_tasks_v1", &task_id, &stored)
                .unwrap();
            let downloads = f
                .client
                .downloads(
                    "1",
                    DownloadQuery {
                        tab: DownloadTab::Completed,
                        category: DownloadCategory::Song,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            assert_eq!(downloads.tasks.len(), 1);
            assert!(downloads.tasks[0].track.voice_id.is_none());
            assert!(downloads.tasks[0].voice_podcast.is_none());
            let song = f
                .client
                .play_selection("1", select(DownloadCategory::Song))
                .await
                .unwrap();
            assert_eq!(song.state, PlaybackStatus::Playing);
            assert_eq!(song.resource_kind, Some(PlayerResourceKind::Downloaded));
            assert_eq!(song.download_task_id.as_deref(), Some(task_id.as_str()));
            assert!(song.play_id.is_some());
            assert_ne!(song.play_id, previous_play_id);
            previous_play_id = song.play_id;
            assert!(song.voice_detail.is_none());
            assert!(song.track.as_ref().unwrap().voice_id.is_none());
            let queue: cumulus_core::playback::QueueSnapshot =
                serde_json::from_value(f.store.get("1", "playback", "queue").unwrap().unwrap())
                    .unwrap();
            assert!(queue.items[0].resource["download_voice_id"].is_null());
            assert!(queue.items[0].resource["song"]["voice_id"].is_null());
            assert!(queue.items[0].resource["voice"].is_null());
            assert_eq!(
                f.store.get("1", "download_tasks_v1", &task_id).unwrap(),
                Some(stored),
                "projection must not rewrite the persisted raw task"
            );
            f.client
                .player_action("1", PlayerAction::Stop)
                .await
                .unwrap();
        }
    }

    // Only the typed native origin changes; the invalid raw song voice ID is
    // deliberately retained. Both list and playback must use canonical 71.
    task.origin = DownloadOrigin::Voice {
        voice_id: "71".into(),
    };
    let stored = serde_json::to_value(&task).unwrap();
    f.store
        .put("1", "download_tasks_v1", &task_id, &stored)
        .unwrap();
    let voice_list = DownloadQuery {
        tab: DownloadTab::Completed,
        category: DownloadCategory::Voice,
        ..Default::default()
    };
    let downloads = f.client.downloads("1", voice_list.clone()).await.unwrap();
    let listed = serde_json::to_value(&downloads.tasks[0].voice_podcast).unwrap();
    assert_eq!(
        listed,
        json!({"id":"9007199254740993","name":"该声音所属播客"})
    );
    for (path, unexpected, projected) in [
        (
            "/song/voice/audio/id",
            json!(null),
            json!({"name":"该声音所属播客"}),
        ),
        (
            "/song/voice/audio/id",
            json!("https://private.invalid/"),
            json!({"name":"该声音所属播客"}),
        ),
        ("/song/voice/mainTrackId", json!("124"), json!(null)),
        ("/song/voice/id", json!("72"), json!(null)),
        ("/song/id", json!("124"), json!(null)),
    ] {
        let mut damaged = stored.clone();
        *damaged.pointer_mut(path).unwrap() = unexpected;
        f.store
            .put("1", "download_tasks_v1", &task_id, &damaged)
            .unwrap();
        let read = f.client.downloads("1", voice_list.clone()).await.unwrap();
        assert_eq!(
            serde_json::to_value(&read.tasks[0].voice_podcast).unwrap(),
            projected,
            "{path}"
        );
        assert_eq!(
            f.store.get("1", "download_tasks_v1", &task_id).unwrap(),
            Some(damaged)
        );
    }
    f.store
        .put("1", "download_tasks_v1", &task_id, &stored)
        .unwrap();
    assert_eq!(f.api.calls.lock().unwrap().len(), calls);
    let voice = f
        .client
        .play_selection("1", select(DownloadCategory::Voice))
        .await
        .unwrap();
    assert_eq!(voice.state, PlaybackStatus::Playing);
    assert_eq!(voice.resource_kind, Some(PlayerResourceKind::Downloaded));
    assert_eq!(voice.download_task_id.as_deref(), Some(task_id.as_str()));
    assert_ne!(voice.play_id, previous_play_id);
    assert_eq!(voice.track.as_ref().unwrap().id, "123");
    assert_eq!(
        voice.track.as_ref().unwrap().voice_id.as_deref(),
        Some("71")
    );
    assert_eq!(
        serde_json::to_value(&voice.voice_detail).unwrap(),
        json!({"id":"71","creator":{"id":"42","name":"合成主播"},"releaseTime":1735787045000_u64,
            "description":description,"coverUrl":"https://p1.music.126.net/episode.jpg",
            "playCount":"0","radio":{"id":"9007199254740993","name":"该声音所属播客",
                "subscriberCount":0,"ownedByViewer":false}})
    );
    let queue: cumulus_core::playback::QueueSnapshot =
        serde_json::from_value(f.store.get("1", "playback", "queue").unwrap().unwrap()).unwrap();
    assert_eq!(queue.items[0].resource["download_voice_id"], "71");
    assert_eq!(queue.items[0].resource["song"]["voice_id"], "71");
    assert_eq!(queue.items[0].resource["voice"]["description"], description);
    assert_eq!(
        f.store.get("1", "download_tasks_v1", &task_id).unwrap(),
        Some(stored)
    );
    assert_eq!(
        f.client.player_snapshot("1").await.unwrap().voice_detail,
        voice.voice_detail
    );
    let public = serde_json::to_string(&voice).unwrap();
    assert!(!public.contains("download_voice_id"));
    assert!(!public.contains("PRIVATE"));
    assert!(!public.contains(f.root.path().to_str().unwrap()));
    assert_eq!(f.api.calls.lock().unwrap().len(), calls);
    assert_eq!(f.backend.audio.calls().len(), media_calls);
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();

    assert!(
        f.client
            .player_snapshot("1")
            .await
            .unwrap()
            .voice_detail
            .is_none()
    );

    // Canonical origin is not permission to rewrite conflicting raw episode
    // identity. Playback keeps the typed episode namespace, but header details
    // remain unavailable when the preserved voice JSON names another episode.
    task.song.as_mut().unwrap()["voice"]["id"] = json!(72);
    let stored = serde_json::to_value(&task).unwrap();
    f.store
        .put("1", "download_tasks_v1", &task_id, &stored)
        .unwrap();
    let conflict = f
        .client
        .play_selection("1", select(DownloadCategory::Voice))
        .await
        .unwrap();
    assert_eq!(conflict.state, PlaybackStatus::Playing);
    assert_eq!(
        conflict.track.as_ref().unwrap().voice_id.as_deref(),
        Some("71")
    );
    assert!(conflict.voice_detail.is_none());
    let queue: cumulus_core::playback::QueueSnapshot =
        serde_json::from_value(f.store.get("1", "playback", "queue").unwrap().unwrap()).unwrap();
    assert_eq!(queue.items[0].resource["download_voice_id"], "71");
    assert_eq!(queue.items[0].resource["song"]["voice_id"], "71");
    assert_eq!(queue.items[0].resource["voice"]["id"], 72);
    assert_eq!(
        f.store.get("1", "download_tasks_v1", &task_id).unwrap(),
        Some(stored)
    );
    assert_eq!(f.api.calls.lock().unwrap().len(), calls);
    assert_eq!(f.backend.audio.calls().len(), media_calls);
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
