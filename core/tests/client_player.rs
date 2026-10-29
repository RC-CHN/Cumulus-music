#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    membership::Quality,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Notify;

#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<String>>,
    hold_id: AtomicU64,
    started: Notify,
    release: Notify,
    trial: Mutex<Option<Value>>,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(path.into());
            Ok(match path {
                "/api/v2/artist/songs" => {
                    let offset = payload["offset"].as_u64().unwrap();
                    let limit = payload["limit"].as_u64().unwrap();
                    json!({"code":200,"total":55,"more":offset+limit<55,"songs":((offset+1)..=(offset+limit).min(55)).map(|id|json!({"id":id,"name":"曲目"})).collect::<Vec<_>>()})
                }
                "/api/album/detail/dynamic" => json!({"code":200,"isSub":false}),
                "/api/album/privilege" => json!({"code":200,"data":[]}),
                "/api/resource-exposure/config" => {
                    json!({"code":200,"data":{"resourceType":"other"}})
                }
                "/api/album/v3/detail" => {
                    json!({"code":200,"album":{"id":7,"name":"长专辑"},"songs":(1..=501).map(|id|json!({"id":id,"name":"曲目","dt":120000})).collect::<Vec<_>>()})
                }
                "/api/v6/playlist/detail" => {
                    json!({"code":200,"playlist":{"trackIds":(1..=205).map(|id|json!({"id":id})).collect::<Vec<_>>()}})
                }
                "/api/v3/song/detail" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["c"].as_str().unwrap()).unwrap();
                    let id = ids[0]["id"].as_str().unwrap().parse::<u64>().unwrap();
                    if self.hold_id.load(Ordering::SeqCst) == id {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    json!({"code":200,"songs":ids.iter().map(|v|json!({"id":v["id"],"name":"曲目","ar":[{"id":12,"name":"歌手"}],"al":{"id":7,"name":"专辑","picUrl":"http://p1.music.126.net/image.jpg?token=SECRET"},"dt":120000,"url":"https://private/?secret=SECRET","cookie":"SECRET"})).collect::<Vec<_>>()})
                }
                "/api/song/enhance/player/url/v1" => {
                    *self.trial.lock().unwrap() = Some(payload["trialMode"].clone());
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/a.mp3?key=SECRET","md5":"abc","expi":120,"level":"standard"}]})
                }
                "/api/song/lyric/v1" => {
                    json!({"code":200,"lrc":{"lyric":"[00:01]第一行\n[00:03]第二行","version":1},"yrc":{"lyric":"[1000,1000](1000,500,0)你(1500,500,0)好","version":1},"ytlrc":{"lyric":"[1000,1000]hello"},"yromalrc":{"lyric":"[1000,1000]ni hao"},"lyricContributor":{"cookie":"SECRET"}})
                }
                _ => panic!("unexpected {path}"),
            })
        })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    Arc<MusicClient>,
    Arc<AtomicU64>,
) {
    let dir = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let count = Arc::new(AtomicU64::new(0));
    let calls = count.clone();
    let client =
        MusicClient::with_session(session.clone(), registry).with_audio_factory(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(support::audio_engine(Arc::new(support::Device::default())))
        });
    (dir, api, session, Arc::new(client), count)
}
fn songs(id: &str) -> PlaySelection {
    PlaySelection::Songs {
        ids: vec![id.into()],
        index: 0,
        quality: Quality::Lossless,
    }
}

#[tokio::test]
async fn local_startup_preferences_need_no_engine_or_http_and_reject_old_account_writes() {
    use cumulus_core::client::dto::StartupPreferencesCommand as Change;
    use cumulus_core::playback::preferences::StartupSettings;
    let (_dir, api, session, client, count) = fixture();
    let initial = client.startup_preferences("1").unwrap();
    assert_eq!(initial.value.settings, StartupSettings::default());
    let saved = client
        .startup_preferences_command(
            "1",
            Change::Change {
                expected_revision: initial.value.revision.unwrap(),
                settings: StartupSettings {
                    auto_play: true,
                    remember_position: false,
                },
            },
        )
        .unwrap();
    assert_eq!(saved.value.revision.as_deref(), Some("1"));
    assert!(
        client
            .startup_preferences_command(
                "1",
                Change::Change {
                    expected_revision: "0".into(),
                    settings: StartupSettings::default()
                }
            )
            .is_err()
    );
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.startup_preferences_command("1", Change::Repair),
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        client.startup_preferences("2").unwrap().value.settings,
        saved.value.settings
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn public_startup_claims_audio_once_and_damaged_preferences_allow_manual_play_and_explicit_repair()
 {
    use cumulus_core::client::dto::StartupPreferencesCommand as Change;
    use cumulus_core::{
        playback::{PlaybackQueue, preferences::StartupSettings},
        storage::StateStore,
    };
    let (dir, _api, session, client, _count) = fixture();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let mut queue = PlaybackQueue::new(Some(store.clone()), "9").unwrap();
    queue
        .replace(vec![json!({"id":1,"dt":120000})], 0, json!({"kind":"test"}))
        .unwrap();
    store.put("9", "playback", "position", &json!({"entry_id":queue.selected().unwrap().entry_id,"seconds":17.0,"state":"paused","timeline_version":1})).unwrap();
    client
        .startup_preferences_command(
            "1",
            Change::Change {
                expected_revision: "0".into(),
                settings: StartupSettings {
                    auto_play: true,
                    remember_position: true,
                },
            },
        )
        .unwrap();
    let playing = client
        .player_action("1", PlayerAction::Startup)
        .await
        .unwrap();
    assert_eq!(
        playing.state,
        cumulus_core::playback::controller::PlaybackStatus::Playing
    );
    assert_eq!(playing.position, Some(17.0));
    assert_eq!(
        client
            .player_action("1", PlayerAction::Startup)
            .await
            .unwrap()
            .play_id,
        playing.play_id
    );
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    assert_eq!(
        client
            .player_action("1", PlayerAction::Startup)
            .await
            .unwrap()
            .state,
        cumulus_core::playback::controller::PlaybackStatus::Idle
    );
    store
        .put(
            "local-device",
            "playback-startup-preferences-v1",
            "current",
            &json!({"damaged":true}),
        )
        .unwrap();
    let reopened = MusicClient::with_session(
        session,
        ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap(),
    )
    .with_audio_factory(|| Ok(support::audio_engine(Arc::new(support::Device::default()))));
    let damaged = reopened.startup_preferences("1").unwrap().value;
    assert!(damaged.damaged && damaged.revision.is_none());
    assert!(matches!(
        reopened.player_action("1", PlayerAction::Startup).await,
        Err(Error::State("playback settings damaged"))
    ));
    let repaired = reopened
        .startup_preferences_command("1", Change::Repair)
        .unwrap()
        .value;
    assert!(!repaired.damaged);
    assert_eq!(repaired.settings, StartupSettings::default());
    reopened
        .startup_preferences_command(
            "1",
            Change::Change {
                expected_revision: repaired.revision.unwrap(),
                settings: StartupSettings {
                    auto_play: true,
                    remember_position: true,
                },
            },
        )
        .unwrap();
    // Changing preferences after a failed initial attempt still takes effect on
    // the next process launch, not a page reload in this process.
    assert_eq!(
        reopened
            .player_action("1", PlayerAction::Startup)
            .await
            .unwrap()
            .state,
        cumulus_core::playback::controller::PlaybackStatus::Idle
    );
    assert_eq!(
        reopened
            .player_action("1", PlayerAction::Restore)
            .await
            .unwrap()
            .state,
        cumulus_core::playback::controller::PlaybackStatus::Paused
    );
}

#[tokio::test]
async fn album_selection_uses_complete_native_order_and_rejects_changed_revision() {
    let (_dir, _api, session, client, _count) = fixture();
    let page = client
        .ranking_page("1", RankingQuery::Album { id: "7".into() }, true)
        .await
        .unwrap();
    let RankingPageView::Album {
        revision, tracks, ..
    } = page.value.page
    else {
        panic!()
    };
    assert_eq!(tracks.len(), 501);
    let state = client
        .play_selection(
            "1",
            PlaySelection::Album {
                id: "7".into(),
                start_id: Some("501".into()),
                revision,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.queue_len, 501);
    assert!(state.history_warning.is_none());
    let history = cumulus_core::history::HistoryService::new(session.clone()).unwrap();
    let token = session.token().unwrap();
    assert_eq!(
        history
            .snapshot_for(&token, cumulus_core::history::HistoryKind::Song)
            .unwrap()
            .local[0]
            .id,
        "501"
    );
    assert_eq!(
        history
            .snapshot_for(&token, cumulus_core::history::HistoryKind::Album)
            .unwrap()
            .local[0]
            .id,
        "7"
    );
    assert_eq!(state.track.unwrap().id, "501");
    assert!(matches!(
        client
            .play_selection(
                "1",
                PlaySelection::Album {
                    id: "7".into(),
                    start_id: None,
                    revision: "changed".into(),
                    quality: Quality::Standard
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 501);
    assert_eq!(queue.tracks.last().unwrap().track.id, "501");
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn complete_playlist_is_native_and_public_player_queue_lyrics_are_whitelisted() {
    let (dir, api, session, client, count) = fixture();
    let mut subscription = client.subscribe_playback("1").await.unwrap();
    let view = client
        .play_selection(
            "1",
            PlaySelection::Playlist {
                id: "7".into(),
                start_id: Some("151".into()),
                revision: None,
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.queue_len, 205);
    assert!(view.history_warning.is_none());
    let history = cumulus_core::history::HistoryService::new(session.clone()).unwrap();
    let playlist = history
        .snapshot_for(
            &session.token().unwrap(),
            cumulus_core::history::HistoryKind::Playlist,
        )
        .unwrap();
    assert_eq!(playlist.local[0].id, "7");
    assert_eq!(view.track.as_ref().unwrap().id, "151");
    assert_eq!(view.requested_quality, Some("lossless".into()));
    assert_eq!(view.actual_quality, Some("standard".into()));
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|s| *s == "/api/v3/song/detail")
            .count(),
        3
    );
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 205);
    assert_eq!(queue.tracks[204].track.id, "205");
    let id = view.play_id.as_ref().unwrap();
    let lyric = client.player_lyrics("1", id).await.unwrap();
    assert_eq!(lyric.lines.len(), 2);
    assert_eq!(lyric.word_lines[0].words.len(), 2);
    assert_eq!(lyric.word_translation[0].lyric.trim(), "hello");
    assert_eq!(lyric.word_romanization[0].lyric.trim(), "ni hao");
    let changed = client.lyric_offset("1", id, 1.0).await.unwrap();
    assert_eq!(changed.offset, 0.5);
    assert_eq!(changed.lines[0].time, Some(0.5));
    for value in [
        serde_json::to_value(view).unwrap(),
        serde_json::to_value(queue).unwrap(),
        serde_json::to_value(lyric).unwrap(),
    ] {
        let text = value.to_string();
        assert!(!text.contains("SECRET"));
        assert!(!text.contains("cookie"));
        assert!(!text.contains("resource_info"));
    }
    let stored = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    assert!(!stored.to_string().contains("SECRET"));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let updated = subscription.changed().await.unwrap();
    assert_eq!(
        updated.state,
        cumulus_core::playback::controller::PlaybackStatus::Paused
    );
}

#[tokio::test]
async fn stale_inputs_and_unknown_native_fields_never_initialize_audio_or_access_network() {
    let (_dir, api, session, client, count) = fixture();
    assert!(matches!(
        client.play_selection("0", songs("1")).await.err().unwrap(),
        Error::StaleResponse
    ));
    assert!(matches!(
        client
            .player_action("0", PlayerAction::Play)
            .await
            .err()
            .unwrap(),
        Error::StaleResponse
    ));
    assert!(serde_json::from_value::<PlaySelection>(json!({"kind":"songs","ids":["1"],"index":0,"quality":"standard","url":"https://private"})).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(api.calls.lock().unwrap().is_empty());
    let stream = client.subscribe_playback("1").await.unwrap();
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        stream.current().err().unwrap(),
        Error::StaleResponse
    ));
}

#[tokio::test]
async fn slow_metadata_cannot_replace_new_selection_and_old_lyrics_cannot_adjust_new_song() {
    let (_dir, api, _session, client, _) = fixture();
    api.hold_id.store(1, Ordering::SeqCst);
    let old_client = client.clone();
    let old = tokio::spawn(async move { old_client.play_selection("1", songs("1")).await });
    api.started.notified().await;
    let next = client.play_selection("1", songs("2")).await.unwrap();
    api.release.notify_waiters();
    assert!(matches!(
        old.await.unwrap().err().unwrap(),
        Error::StaleResponse
    ));
    let play_id = next.play_id.unwrap();
    client.player_lyrics("1", &play_id).await.unwrap();
    client.play_selection("1", songs("3")).await.unwrap();
    assert!(matches!(
        client.lyric_offset("1", &play_id, 1.0).await.err().unwrap(),
        Error::StaleResponse
    ));
    assert_eq!(
        client.player_queue("1").await.unwrap().tracks[0].track.id,
        "3"
    );
}

#[tokio::test]
async fn catalog_selection_keeps_loaded_artist_order_source_and_navigation_references() {
    let (dir, api, _session, client, _count) = fixture();
    let mut pages = Vec::new();
    for offset in [0, 20] {
        let query = CatalogQuery::ArtistSongs {
            id: "12".into(),
            offset,
            limit: 20,
        };
        let saved = client
            .catalog_page("1", query.clone(), true)
            .await
            .unwrap()
            .value
            .unwrap();
        pages.push(CatalogSelectionPage {
            query,
            revision: saved.revision,
        });
    }
    let state = client
        .play_selection(
            "1",
            PlaySelection::Catalog {
                pages: pages.clone(),
                index: 23,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.queue_len, 40);
    let track = state.track.unwrap();
    assert_eq!(track.id, "24");
    assert_eq!(track.artist_refs[0].id.as_deref(), Some("12"));
    assert_eq!(track.album_id.as_deref(), Some("7"));
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(-1)));
    let stored = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    assert_eq!(
        stored["items"][0]["source"],
        json!({"kind":"artist","id":"12","scene":"artist","trial_mode":-1})
    );
    pages[1].revision = "old".into();
    assert!(matches!(
        client
            .play_selection(
                "1",
                PlaySelection::Catalog {
                    pages: pages.clone(),
                    index: 0,
                    quality: Quality::Standard
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    pages[1].query = CatalogQuery::ArtistSongs {
        id: "12".into(),
        offset: 40,
        limit: 20,
    };
    assert!(matches!(
        client
            .play_selection(
                "1",
                PlaySelection::Catalog {
                    pages,
                    index: 0,
                    quality: Quality::Standard
                }
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 40);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn catalog_album_selection_revalidates_the_album_and_keeps_all_tracks() {
    let (_dir, api, _session, client, _count) = fixture();
    let query = CatalogQuery::Album { id: "7".into() };
    let saved = client
        .catalog_page("1", query.clone(), true)
        .await
        .unwrap()
        .value
        .unwrap();
    api.calls.lock().unwrap().clear();
    let state = client
        .play_selection(
            "1",
            PlaySelection::Catalog {
                pages: vec![CatalogSelectionPage {
                    query,
                    revision: saved.revision,
                }],
                index: 500,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.queue_len, 501);
    assert_eq!(state.track.unwrap().id, "501");
    assert_eq!(api.calls.lock().unwrap()[0], "/api/album/v3/detail");
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn output_settings_are_local_lazy_and_reject_old_account_commands() {
    let (_dir, api, session, client, count) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    let initial = client.output_settings(&epoch, false).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(initial.value.supported, None);
    let inspected = client.output_settings(&epoch, true).await.unwrap();
    assert_eq!(inspected.value.supported, Some(false));
    client.output_settings(&epoch, true).await.unwrap();
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "one never-loaded inspector"
    );
    let saved = client
        .output_command(
            &epoch,
            OutputCommand::Select {
                expected_revision: "0".into(),
                id: "auto".into(),
            },
        )
        .unwrap();
    assert_eq!(saved.value.revision.as_deref(), Some("1"));
    assert!(
        client
            .output_command(
                &epoch,
                OutputCommand::Retry {
                    expected_revision: "0".into()
                }
            )
            .is_err()
    );
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.output_command(
            &epoch,
            OutputCommand::Retry {
                expected_revision: "1".into()
            }
        ),
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.output_settings(&epoch, true).await,
        Err(Error::StaleResponse)
    ));
    let next = session.token().unwrap().generation.to_string();
    assert_eq!(
        client
            .output_settings(&next, false)
            .await
            .unwrap()
            .value
            .revision
            .as_deref(),
        Some("1")
    );
    assert!(
        api.calls.lock().unwrap().is_empty(),
        "device settings have no music API"
    );
}

#[tokio::test]
async fn desktop_lyric_document_shares_detail_cache_offset_and_native_clock_without_retries() {
    use cumulus_core::preferences::desktop_lyrics::{Appearance, Auxiliary};
    let (_dir, api, session, client, count) = fixture();
    assert!(matches!(
        client.desktop_lyric_document("1", "missing").await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(api.calls.lock().unwrap().is_empty());
    let current = client.play_selection("1", songs("1")).await.unwrap();
    let id = current.play_id.unwrap();
    let mut document = client.desktop_lyric_document("1", &id).await.unwrap();
    let requests = api.calls.lock().unwrap().len();
    let style = Appearance::default();
    let initial = client.desktop_lyric_frame(&document, &style).unwrap();
    assert_eq!(initial.play_id, id);
    assert!(initial.frame.timed_words);
    assert_eq!(initial.frame.lines[0].auxiliary, "hello ");
    for _ in 0..50 {
        assert!(
            !client
                .refresh_desktop_lyric_document(&mut document)
                .await
                .unwrap()
        );
        client.desktop_lyric_frame(&document, &style).unwrap();
    }
    assert_eq!(api.calls.lock().unwrap().len(), requests);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    client.lyric_offset("1", &id, 1.0).await.unwrap();
    assert!(
        client
            .refresh_desktop_lyric_document(&mut document)
            .await
            .unwrap()
    );
    let updated = client.desktop_lyric_frame(&document, &style).unwrap();
    assert_ne!(updated.revision, initial.revision);
    assert_eq!(updated.frame.offset, 0.5);
    client
        .desktop_lyric_offset(&mut document, -1.0)
        .await
        .unwrap();
    assert_eq!(client.player_lyrics("1", &id).await.unwrap().offset, 0.0);
    let translated = client
        .desktop_lyric_frame(
            &document,
            &Appearance {
                auxiliary: Auxiliary::Roman,
                ..style.clone()
            },
        )
        .unwrap();
    assert_eq!(translated.frame.lines[0].auxiliary.trim(), "ni hao");
    assert_eq!(api.calls.lock().unwrap().len(), requests);
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = client.desktop_lyric_frame(&document, &style).unwrap();
    assert!(!paused.playing);
    client.play_selection("1", songs("2")).await.unwrap();
    assert!(client.desktop_lyric_frame(&document, &style).is_err());
    assert!(
        client
            .desktop_lyric_offset(&mut document, 1.0)
            .await
            .is_err()
    );
    session
        .adopt(api, json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(
        client
            .refresh_desktop_lyric_document(&mut document)
            .await
            .is_err()
    );
}
