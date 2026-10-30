#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    library::{AudioMetadata, LocalLibrary, monitor::WatchStatus, probe::AudioProbe},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
struct Offline;
impl Api for Offline {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move { panic!("local operation attempted network: {path}") })
    }
}
#[derive(Default)]
struct Probe {
    hold: AtomicBool,
    entered: AtomicBool,
}
impl AudioProbe for Probe {
    fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
        self.entered.store(true, Ordering::SeqCst);
        while self.hold.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(AudioMetadata {
            title: path.file_stem().unwrap().to_string_lossy().into_owned(),
            album: "Album".into(),
            artists: vec!["Straße".into(), "Guest".into(), "Guest".into()],
            duration: 120.0,
            format: Some("pcm_s16le".into()),
            container: "wav".into(),
            bitrate: None,
            sample_rate: Some(8000),
            channels: Some(1),
            cover_bytes: None,
        })
    }
}
fn client(root: &Path, probe: Arc<Probe>) -> (Arc<AccountSession>, MusicClient, LocalLibrary) {
    let registry = ProfileRegistry::open(root, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(
        Arc::new(Offline),
        Some(registry.store()),
    ));
    let library = LocalLibrary::with_probe(registry.store(), probe.clone()).unwrap();
    let client = MusicClient::with_session(session.clone(), registry)
        .with_local_probe(probe)
        .with_audio_factory(|| Ok(support::audio_engine(Arc::new(support::Device::default()))));
    (session, client, library)
}

#[tokio::test]
async fn matching_commands_use_account_epochs_and_opaque_file_revisions() {
    use cumulus_core::{
        client::dto::LocalMatchCommand,
        library::matching::MatchPhase,
        media::tags::{track_plan, write_tagged_copy},
    };
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let seed = dir.path().join("seed.flac");
    std::fs::write(&seed, include_bytes!("fixtures/tag_audio/seed.flac")).unwrap();
    let plan = track_plan(
        &json!({"id":123,"name":"匹配曲目"}),
        &json!({"type":"flac","md5":"ab".repeat(16),"br":128000}),
        false,
    )
    .unwrap();
    write_tagged_copy(seed, media.join("track.flac"), &plan, None).unwrap();
    let (session, client, library) = client(&dir.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    assert!(client.local_match_view("0", vec![]).await.is_err());
    session
        .adopt(
            Arc::new(Offline),
            json!({"userId":9}),
            &session.token().unwrap(),
        )
        .unwrap();
    let page = client.local_page(LocalQuery::default()).await.unwrap();
    let row = &page.tracks[0];
    assert!(matches!(
        client.local_match_view("0", vec![]).await,
        Err(Error::StaleResponse)
    ));
    client
        .local_match_command(
            "1",
            LocalMatchCommand::Start {
                query: LocalQuery::default(),
            },
        )
        .await
        .unwrap();
    let mut finished = false;
    // Completion is asynchronous and includes blocking filesystem work. There
    // is no two-second product SLA; use a bounded deadline and report the job
    // state, failing immediately on a durable worker error.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let view = match client
            .local_match_view("1", vec![row.track.id.clone()])
            .await
        {
            Ok(view) => view,
            // The worker may hold the library operation lock while updating it.
            // A status read may race that lock; only this transient response is
            // retried within the same deadline. Durable worker failures below
            // and every other read error still fail the test.
            Err(Error::MutationBusy) => {
                tokio::time::sleep(Duration::from_millis(20)).await;
                continue;
            }
            Err(error) => panic!("matching status read failed: {error}"),
        };
        assert!(
            view.value
                .job
                .as_ref()
                .is_none_or(|job| job.phase != MatchPhase::Failed),
            "matching worker failed: {:?}",
            view.value.job,
        );
        if view
            .value
            .job
            .as_ref()
            .is_some_and(|j| j.phase == MatchPhase::Complete)
        {
            assert_eq!(view.value.links.len(), 1);
            assert_eq!(view.value.links[0].song_id, "123");
            assert_eq!(view.value.links[0].revision, row.revision);
            let text = serde_json::to_string(&view).unwrap();
            assert!(
                !text.contains("local_revision")
                    && !text.contains(&media.to_string_lossy().to_string())
            );
            finished = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        finished,
        "matching did not complete: {:?}",
        client
            .local_match_view("1", vec![row.track.id.clone()])
            .await
            .unwrap()
            .value
            .job,
    );
    client
        .local_match_command(
            "1",
            LocalMatchCommand::Clear {
                id: row.track.id.clone(),
                revision: row.revision.clone(),
            },
        )
        .await
        .unwrap();
    assert!(
        client
            .local_match_view("1", vec![row.track.id.clone()])
            .await
            .unwrap()
            .value
            .links
            .is_empty()
    );
    std::fs::write(media.join("track.flac"), "replaced audio").unwrap();
    library.scan(&media).unwrap();
    assert!(matches!(
        client
            .local_match_command(
                "1",
                LocalMatchCommand::Bind {
                    id: row.track.id.clone(),
                    revision: row.revision.clone(),
                    song_id: "123".into()
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    session.detach().unwrap();
    assert!(matches!(
        client.local_match_view("1", vec![]).await,
        Err(Error::StaleResponse)
    ));
}
async fn settled(client: &MusicClient) -> LocalPageView {
    for _ in 0..300 {
        let view = client.local_page(LocalQuery::default()).await.unwrap();
        if !view
            .roots
            .iter()
            .any(|r| matches!(r.status, WatchStatus::Pending | WatchStatus::Scanning))
        {
            return view;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("library did not settle")
}
#[tokio::test]
async fn guest_full_queue_pagination_search_groups_and_account_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media");
    std::fs::create_dir(&media).unwrap();
    for i in 0..205 {
        std::fs::write(media.join(format!("Track {i:03}.wav")), "fixture").unwrap();
    }
    let (session, client, library) = client(&dir.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let page = client
        .local_page(LocalQuery {
            limit: 20,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        (page.total, page.playable, page.tracks.len()),
        (205, 205, 20)
    );
    assert!(page.more);
    assert!(page.tracks.iter().all(|r| r.track.local));
    assert!(page.tracks.iter().all(|r| r.format == "wav"));
    let query = LocalQuery {
        keyword: "STRASSE".into(),
        group: LocalGroup::Artist,
        limit: 20,
        offset: 200,
        ..Default::default()
    };
    let grouped = client.local_page(query.clone()).await.unwrap();
    assert_eq!(
        (grouped.total, grouped.tracks.len(), grouped.groups.len()),
        (205, 5, 2)
    );
    assert_eq!(grouped.groups[0].count, 205); // Repeated artist tags count once per track.
    let selected = grouped.tracks[3].track.id.clone();
    let state = client
        .play_selection(
            "0",
            PlaySelection::Local {
                query,
                revision: page.revision.clone(),
                start_id: Some(selected.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(state.queue_len, 205);
    assert_eq!(state.track.unwrap().id, selected);
    let queue = client.player_queue("0").await.unwrap();
    assert_eq!(queue.tracks.len(), 205);
    assert_eq!(queue.tracks[203].track.id, selected);
    let public = serde_json::to_string(&queue).unwrap();
    assert!(
        !public.contains("local_revision")
            && !public.contains(&media.to_string_lossy().to_string())
    );
    let sub = client.subscribe_playback("0").await.unwrap();
    assert_eq!(sub.current().unwrap().epoch, "0");
    assert!(
        client
            .play_selection(
                "0",
                PlaySelection::Songs {
                    ids: vec!["1".into()],
                    index: 0,
                    quality: cumulus_core::membership::Quality::Standard
                }
            )
            .await
            .is_err()
    );
    session
        .adopt(
            Arc::new(Offline),
            json!({"userId":9}),
            &session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        client.player_queue("0").await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(sub.current(), Err(Error::StaleResponse)));
    assert!(client.player_queue("1").await.unwrap().tracks.is_empty());
    assert_eq!(
        client
            .local_page(LocalQuery::default())
            .await
            .unwrap()
            .total,
        205
    );
    session.detach().unwrap();
    assert_eq!(client.player_queue("2").await.unwrap().tracks.len(), 205);
}

#[tokio::test]
async fn cue_selection_native_bounds_sidecar_clock_offset_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("album.wav"), "fixture").unwrap();
    std::fs::write(
        media.join("album.lrc"),
        "[00:00]Intro\n[00:35]Part two\n[00:55]Later\n[01:30]Outro",
    )
    .unwrap();
    std::fs::write(media.join("album.cue"),"FILE \"album.wav\" WAVE\nTRACK 01 AUDIO\nTITLE \"First\"\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nTITLE \"Second\"\nINDEX 01 00:30:00\n").unwrap();
    let state = dir.path().join("state");
    let (_, client, library) = client(&state, Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let page = client.local_page(LocalQuery::default()).await.unwrap();
    assert_eq!(page.total, 2);
    let second = page
        .tracks
        .iter()
        .find(|r| r.track.name == "Second")
        .unwrap();
    assert!(second.cue);
    let view = client
        .play_selection(
            "0",
            PlaySelection::Local {
                query: LocalQuery::default(),
                revision: page.revision,
                start_id: Some(second.track.id.clone()),
            },
        )
        .await
        .unwrap();
    let play_id = view.play_id.unwrap();
    let timeline = view.timeline.unwrap();
    assert_eq!((timeline.media_start, timeline.total), (30.0, Some(90.0)));
    let lyrics = client.player_lyrics("0", &play_id).await.unwrap();
    assert_eq!(lyrics.lines[0].time, Some(0.0));
    assert_eq!(lyrics.lines[1].time, Some(5.0));
    assert_eq!(
        client
            .lyric_offset("0", &play_id, 1.0)
            .await
            .unwrap()
            .offset,
        0.5
    );
    client
        .player_action("0", PlayerAction::Seek { seconds: 23.0 })
        .await
        .unwrap();
    client
        .player_action("0", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let original = client.player_queue("0").await.unwrap().current;
    drop(client);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let (_, reopened, _) = self::client(&state, Arc::new(Probe::default()));
    let queue = reopened.player_queue("0").await.unwrap();
    assert_eq!(queue.current, original);
    let restored = reopened
        .player_action("0", PlayerAction::Startup)
        .await
        .unwrap();
    assert_eq!(restored.timeline.unwrap().media_start, 30.0);
    assert!((restored.position.unwrap() - 23.0).abs() < 0.1);
    let lyrics = reopened
        .player_lyrics("0", restored.play_id.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(lyrics.offset, 0.5);
    std::fs::write(media.join("album.lrc"), "[00:31]Changed sidecar").unwrap();
    let refreshed = reopened
        .player_lyrics("0", restored.play_id.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(refreshed.offset, 0.0);
    assert_eq!(refreshed.lines[0].time, Some(1.0));
    assert_eq!(refreshed.lines[0].lyric, "Changed sidecar");
    reopened
        .startup_preferences_command(
            "0",
            StartupPreferencesCommand::Change {
                expected_revision: "0".into(),
                settings: cumulus_core::playback::preferences::StartupSettings {
                    auto_play: true,
                    remember_position: false,
                },
            },
        )
        .unwrap();
    reopened
        .player_action("0", PlayerAction::Stop)
        .await
        .unwrap();
    drop(reopened);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (_, reopened, _) = self::client(&state, Arc::new(Probe::default()));
    let reset = reopened
        .player_action("0", PlayerAction::Startup)
        .await
        .unwrap();
    assert_eq!(reset.timeline.unwrap().media_start, 30.0);
    assert_eq!(reset.position, Some(0.0));
    assert_eq!(
        reset.state,
        cumulus_core::playback::controller::PlaybackStatus::Playing
    );
    std::fs::write(media.join("album.cue"), "changed").unwrap();
    assert!(
        reopened
            .player_action("0", PlayerAction::Refresh)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn snapshot_changes_never_mix_pages_or_replace_queue_and_missing_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let file = media.join("One.wav");
    std::fs::write(&file, "fixture").unwrap();
    let (_, client, library) = client(&dir.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let before = client.local_page(LocalQuery::default()).await.unwrap();
    std::fs::write(media.join("Two.wav"), "fixture").unwrap();
    library.scan(&media).unwrap();
    let query = LocalQuery {
        revision: Some(before.revision.clone()),
        ..Default::default()
    };
    assert!(matches!(
        client.local_page(query).await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        client
            .play_selection(
                "0",
                PlaySelection::Local {
                    query: LocalQuery::default(),
                    revision: before.revision,
                    start_id: None
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(client.player_queue("0").await.unwrap().tracks.is_empty());
    std::fs::remove_file(&file).unwrap();
    library.scan(&media).unwrap();
    let page = client.local_page(LocalQuery::default()).await.unwrap();
    assert_eq!((page.total, page.playable), (2, 1));
    let hidden = client
        .local_page(LocalQuery {
            include_missing: false,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(hidden.total, 1);
    let filtered = LocalQuery {
        group_key: Some("invalid".into()),
        ..Default::default()
    };
    assert!(client.local_page(filtered).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monitor_progress_disable_cancel_no_partial_publish_and_reopen_watch() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("One.wav"), "fixture").unwrap();
    let probe = Arc::new(Probe::default());
    probe.hold.store(true, Ordering::SeqCst);
    let state = dir.path().join("state");
    let (_, client, _) = client(&state, probe.clone());
    client
        .local_command(LocalCommand::Watch {
            path: media.to_string_lossy().into_owned(),
        })
        .await
        .unwrap();
    for _ in 0..300 {
        if probe.entered.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let scanning = client.local_page(LocalQuery::default()).await.unwrap();
    assert_eq!(scanning.total, 0);
    assert_eq!(scanning.roots[0].progress.as_ref().unwrap().total, 1);
    let id = scanning.roots[0].id.clone();
    client
        .local_command(LocalCommand::Disable { id: id.clone() })
        .await
        .unwrap();
    probe.hold.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let disabled = settled(&client).await;
    assert_eq!(disabled.total, 0);
    assert!(!disabled.roots[0].enabled);
    assert!(disabled.roots[0].progress.is_none());
    client
        .local_command(LocalCommand::Enable { id })
        .await
        .unwrap();
    assert_eq!(settled(&client).await.total, 1);
    drop(client);
    tokio::time::sleep(Duration::from_millis(100)).await;
    std::fs::write(media.join("Two.wav"), "fixture").unwrap();
    let (_, reopened, _) = self::client(&state, probe);
    reopened.start_local_library().await.unwrap();
    for _ in 0..300 {
        if reopened
            .local_page(LocalQuery::default())
            .await
            .unwrap()
            .total
            == 2
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("saved root did not resume")
}

#[tokio::test]
async fn guest_history_replays_current_files_and_rejects_changed_fingerprints() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let path = media.join("one.wav");
    std::fs::write(&path, "original synthetic audio").unwrap();
    let (_, client, library) = client(&root.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let page = client.local_page(LocalQuery::default()).await.unwrap();
    client
        .play_selection(
            "0",
            PlaySelection::Local {
                query: LocalQuery::default(),
                revision: page.revision,
                start_id: None,
            },
        )
        .await
        .unwrap();
    let history = client
        .history_page("0", HistoryKind::Song, true)
        .await
        .unwrap()
        .value;
    let encoded = serde_json::to_string(&history).unwrap();
    assert!(
        !encoded.contains(&media.to_string_lossy().to_string())
            && !encoded.contains("local_revision")
    );
    assert!(!history.can_sync);
    assert_eq!(history.entries.len(), 1);
    let selection = || PlaySelection::History {
        history_kind: HistoryKind::Song,
        revision: history.revision.clone(),
        ids: vec![history.entries[0].id.clone()],
        index: 0,
        quality: cumulus_core::membership::Quality::Standard,
    };
    let replay = client.play_selection("0", selection()).await.unwrap();
    assert_eq!(replay.queue_len, 1);
    assert!(replay.track.unwrap().local);
    let current = client
        .history_page("0", HistoryKind::Song, false)
        .await
        .unwrap()
        .value;
    std::fs::write(path, "different and longer synthetic file").unwrap();
    let changed = client
        .history_page("0", HistoryKind::Song, false)
        .await
        .unwrap()
        .value;
    assert!(matches!(&changed.entries[0].resource,HistoryResource::Song{track} if track.missing));
    assert!(
        client
            .play_selection(
                "0",
                PlaySelection::History {
                    history_kind: HistoryKind::Song,
                    revision: current.revision,
                    ids: vec![current.entries[0].id.clone()],
                    index: 0,
                    quality: cumulus_core::membership::Quality::Standard
                }
            )
            .await
            .is_err()
    );
}
