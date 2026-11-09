use super::{HttpResponse, ScriptHttp, gate, playback, reply, vinyl_http_with_credentials};
#[path = "voice_songs/collect.rs"]
mod collect;
#[path = "voice_songs/download.rs"]
mod download;
#[path = "voice_songs/group_download.rs"]
mod group_download;
#[path = "voice_songs/likes.rs"]
mod likes;
#[path = "voice_songs/picker.rs"]
mod picker;
#[path = "voice_songs/purchased.rs"]
mod purchased;
#[path = "voice_songs/selection.rs"]
mod selection;
use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials},
    client::{
        MusicClient,
        dto::{
            PlaySelection, PlayerAction, PlayerResourceKind, QueueClickPreferencesCommand,
            SceneFmCommand, VoiceSongAction,
        },
    },
    history::HistorySyncAction,
    membership::Quality,
    playback::{PlaybackMode, controller::PlaybackStatus},
    protocol::{
        api::Api,
        http::{ApiOrigin, MusicHttpClient},
    },
};
use serde_json::{Value, json};
use std::{
    future::poll_fn,
    sync::{Arc, atomic::Ordering},
    task::Poll,
    time::Duration,
};

const CARD: &str = "/api/v1/dj/program/tracks";
const HOLD: &str = "/api/w/nuser/account/get";
const COOKIE: &str = "MUSIC_U=synthetic-voice; Path=/; Secure";
const VOICE: &str = "71";
const SONG: &str = "424242";
const STARTUP: [&str; 4] = [
    "/api/dj/program/detail",
    "/api/dj/program/detail",
    "/api/dj/play/url/check",
    "/api/song/enhance/player/url/v1",
];

struct Fixture {
    root: tempfile::TempDir,
    native: Arc<MusicHttpClient>,
    credentials: Arc<Credentials>,
    session: Arc<AccountSession>,
    store: Arc<cumulus_core::storage::StateStore>,
    client: Arc<MusicClient>,
    device: Arc<playback::Device>,
    http: Arc<ScriptHttp>,
    epoch: String,
    play_id: String,
}

fn episode() -> Value {
    json!({
        "id":VOICE,"name":"合成节目","duration":120000,"mainTrackId":SONG,
        "mainSong":{"id":SONG,"name":"合成节目音轨","duration":120000},
        "songs":[{"id":9,"name":"DO_NOT_EXPORT_EMBEDDED","al":{"id":9}}]
    })
}

fn detail() -> HttpResponse {
    json!({"code":200,"program":episode()}).into()
}

fn authorized() -> [HttpResponse; 2] {
    [
        json!({"code":200,"data":{"voiceId":VOICE,"songId":SONG,"canPlay":true}}).into(),
        json!({"code":200,"data":[{"id":SONG,"code":200,"url":"https://m1.music.126.net/episode.mp3?auth=DO_NOT_EXPORT","md5":"abc","level":"standard","expi":120}]}).into(),
    ]
}

impl Fixture {
    fn host(cookies: &[&str], replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let (native, http, credentials) =
            vinyl_http_with_credentials(root.path(), cookies, replies);
        let registry = super::fixture_registry(&root.path().join("host"));
        let store = registry.store();
        let session = Arc::new(AccountSession::new(native.clone(), Some(store.clone())));
        session
            .adopt(
                native.clone(),
                json!({"userId":25}),
                &session.token().unwrap(),
            )
            .unwrap();
        let epoch = session.token().unwrap().generation.to_string();
        let device = Arc::new(playback::Device::default());
        let audio = device.clone();
        let client = Arc::new(
            MusicClient::with_session(session.clone(), registry)
                .with_audio_factory(move || Ok(playback::audio_engine(audio.clone()))),
        );
        // Stop is used to invalidate a held card read. Keep the independent
        // remote progress writer disabled throughout this sealed fixture.
        client
            .history_sync_preference(
                &epoch,
                HistorySyncAction::Set {
                    enabled: false,
                    confirmed_first_disable: true,
                },
            )
            .unwrap();
        Self {
            root,
            native,
            credentials,
            session,
            store,
            client,
            device,
            http,
            epoch,
            play_id: String::new(),
        }
    }

    async fn new(cookies: &[&str], replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let mut f = Self::host(
            cookies,
            [detail(), detail()]
                .into_iter()
                .chain(authorized())
                .chain(replies),
        );
        f.play_id = f
            .client
            .play_selection(&f.epoch, PlaySelection::Voice { id: VOICE.into() })
            .await
            .unwrap()
            .play_id
            .unwrap();
        assert_eq!(
            f.http
                .calls()
                .iter()
                .map(|call| call.path.as_str())
                .collect::<Vec<_>>(),
            STARTUP
        );
        f
    }
}

fn card() -> Value {
    json!({"code":200,"tracks":[
        {"id":301,"name":"标题 <字面文字>","ar":[{"id":42,"name":"甲","cookie":"DO_NOT_EXPORT"}],
         "al":{"id":501,"name":"错误专辑"},"album":{"albumId":502,"albumName":"正确专辑",
          "cover":"http://p1.music.126.net/fake/cover.jpg?token=DO_NOT_EXPORT","picUrl":"https://evil.invalid/cover"},
         "dt":0,"duration":123456,"url":"https://evil.invalid/?token=DO_NOT_EXPORT",
         "voice_id":999,"privilege":{"id":301,"token":"DO_NOT_EXPORT"}},
        {"id":9,"name":"不要带出无专辑行","album":false,"al":{"id":2}},
        {"id":101,"name":"旧接口","al":{"id":601,"name":"旧专辑"},"ar":[],"artists":[{"name":"不覆盖显式空数组"}]},
        {"id":301,"name":"同一 ID 的另一个位置","album":{}}
    ],"data":{"tracks":[{"id":9,"name":"不接受嵌套响应"}]}})
}

#[tokio::test]
async fn voice_songs_reads_only_the_exact_card_and_filters_in_returned_order() {
    let f = Fixture::new(
        &[COOKIE],
        [
            card().into(),
            json!({"code":200}).into(),
            json!({"code":"200","tracks":[]}).into(),
            json!({"code":200,"tracks":null}).into(),
            json!({"code":200,"tracks":vec![json!({}); 101]}).into(),
            json!({"code":403}).into(),
        ],
    )
    .await;
    for (epoch, play_id) in [(&*f.epoch, "stale-play"), ("stale-epoch", &*f.play_id)] {
        assert!(matches!(
            f.client.player_voice_songs(epoch, play_id).await,
            Err(Error::StaleResponse)
        ));
    }
    assert_eq!(f.http.calls().len(), STARTUP.len());
    let view = f
        .client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap();
    let value = serde_json::to_value(view).unwrap();
    assert_eq!(value["playId"], f.play_id);
    assert_eq!(value["voiceId"], VOICE);
    let tracks = value["tracks"].as_array().unwrap();
    assert_eq!(
        tracks
            .iter()
            .map(|track| track["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["301", "101", "301"]
    );
    assert_eq!(
        tracks[0],
        json!({
            "id":"301","name":"标题 <字面文字>","artists":["甲"],"artistRefs":[{"id":"42","name":"甲"}],
            "album":"正确专辑","albumId":"502","coverUrl":"https://p1.music.126.net/fake/cover.jpg?param=320y320",
            "durationMs":123456,"fee":0,"missing":false,"local":false
        })
    );
    assert_eq!(tracks[1]["albumId"], "601");
    assert_eq!(tracks[1]["artists"], json!([]));
    assert!(!value.to_string().contains("DO_NOT_EXPORT"));
    for _ in 0..2 {
        assert!(
            f.client
                .player_voice_songs(&f.epoch, &f.play_id)
                .await
                .unwrap()
                .tracks
                .is_empty()
        );
    }
    for _ in 0..2 {
        assert!(matches!(
            f.client.player_voice_songs(&f.epoch, &f.play_id).await,
            Err(Error::Response(_))
        ));
    }
    assert!(
        f.client
            .player_voice_songs(&f.epoch, &f.play_id)
            .await
            .err()
            .unwrap()
            .is_service_code(403)
    );
    let calls = f.http.calls();
    assert_eq!(calls.len(), STARTUP.len() + 6);
    for call in &calls[STARTUP.len()..] {
        assert_eq!(call.path, CARD);
        let mut payload = call.payload.clone();
        assert!(
            payload
                .as_object_mut()
                .unwrap()
                .remove("header")
                .unwrap()
                .is_string()
        );
        assert_eq!(payload, json!({"id":VOICE}));
        assert!(
            call.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-voice")
        );
        assert!(call.login_chain.is_none());
    }
}

#[tokio::test]
async fn voice_songs_fences_playback_and_credentials_before_dispatch_and_after_reply() {
    for rotate in [false, true] {
        for already_dispatched in [false, true] {
            let (held, release) = gate();
            let f = Fixture::new(&[COOKIE], [held]).await;
            let occupying = if already_dispatched {
                None
            } else {
                let native = f.native.clone();
                Some(tokio::spawn(async move {
                    native.request(HOLD, json!({}), false).await
                }))
            };
            let next_call = STARTUP.len() + 1;
            if occupying.is_some() {
                f.http.wait(next_call).await;
            }
            let mut read = Box::pin(f.client.player_voice_songs(&f.epoch, &f.play_id));
            assert!(
                poll_fn(|cx| Poll::Ready(read.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            f.http.wait(next_call).await;
            if rotate {
                let target = ApiOrigin::Pc.url();
                let revision = f.credentials.request_credentials(&target).unwrap().revision;
                f.credentials
                    .record_response(
                        &target,
                        &["MUSIC_U=synthetic-voice-rotated; Path=/; Secure".into()],
                        None,
                        &revision,
                    )
                    .unwrap();
            } else {
                let stopped = f
                    .client
                    .player_action(&f.epoch, PlayerAction::Stop)
                    .await
                    .unwrap();
                assert_ne!(stopped.play_id.as_deref(), Some(f.play_id.as_str()));
            }
            assert!(release.send(Ok(reply(card()))).is_ok());
            if let Some(occupying) = occupying {
                // The replaced revision may make even the occupier stale;
                // either outcome must release the old dispatch slot.
                let _ = occupying.await.unwrap();
            }
            assert!(matches!(read.await, Err(Error::StaleResponse)));
            let calls = f.http.calls();
            assert_eq!(calls.len(), next_call);
            assert_eq!(
                calls[next_call - 1].path,
                if already_dispatched { CARD } else { HOLD }
            );
        }
    }
}

#[tokio::test]
async fn voice_songs_cannot_send_under_a_different_scoped_cookie() {
    let f = Fixture::new(
        &[
            COOKIE,
            "MUSIC_U=synthetic-other; Path=/eapi/v1/dj/program/; Secure",
        ],
        [],
    )
    .await;
    assert!(matches!(
        f.client.player_voice_songs(&f.epoch, &f.play_id).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.http.calls().len(), STARTUP.len());
}

#[tokio::test]
async fn voice_songs_scene_requires_the_current_episode_to_finish_native_load() {
    let directory = json!({"code":200,"data":[{"creatives":[{
        "creativeId":"OP_1","creativeExtInfoVO":{"djPrograms":[episode()]},
        "uiElement":{"mainTitle":{"title":"合成随心听"}}
    }]}]});
    let f = Fixture::host(
        &[COOKIE],
        [directory.into(), detail()]
            .into_iter()
            .chain(authorized())
            .chain([card().into()]),
    );
    let scene = f.client.scene_fm_view(&f.epoch, true).await.unwrap().value;
    f.device.hold_load.store(true, Ordering::SeqCst);
    let play = {
        let client = f.client.clone();
        let epoch = f.epoch.clone();
        tokio::spawn(async move {
            client
                .scene_fm_command(
                    &epoch,
                    SceneFmCommand::Play {
                        revision: scene.revision.unwrap(),
                    },
                )
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), f.device.started.notified())
        .await
        .unwrap();
    let pending = f.client.player_snapshot(&f.epoch).await.unwrap();
    let play_id = pending.play_id.unwrap();
    assert!(
        f.client
            .player_voice_songs(&f.epoch, &play_id)
            .await
            .is_err()
    );
    assert_eq!(f.http.calls().len(), 4);
    f.device.hold_load.store(false, Ordering::SeqCst);
    assert!(play.await.unwrap().unwrap().value.playing);
    let ready = f.client.player_snapshot(&f.epoch).await.unwrap();
    assert_eq!(ready.play_id.as_deref(), Some(play_id.as_str()));
    let songs = f
        .client
        .player_voice_songs(&f.epoch, &play_id)
        .await
        .unwrap();
    assert_eq!(songs.voice_id, VOICE);
    assert_eq!(songs.tracks.len(), 3);
    let calls = f.http.calls();
    assert_eq!(calls.len(), 5);
    assert_eq!(calls[4].path, CARD);
    assert_eq!(calls[4].payload["id"], VOICE);
}

#[tokio::test]
async fn voice_songs_accepts_published_typed_download_and_refuses_conflicting_episode() {
    use cumulus_core::download::service::{
        DownloadDestination, DownloadOptions, DownloadRunOptions, DownloadService, DownloadStatus,
    };
    use md5::{Digest, Md5};
    const AUDIO: &[u8] = include_bytes!("../fixtures/tag_audio/seed.mp3");
    let permission = json!({"code":200,"data":{
        "allowDownloadVoiceIds":[VOICE],"forbidDownloadVoiceIds":[],
        "downloadCheckResultList":[{"voiceId":VOICE,"canDownload":true}]
    }});
    let source = json!({"code":200,"data":{"id":SONG,"code":200,
        "url":"https://m.test.music.126.net/media?auth=DO_NOT_EXPORT",
        "md5":hex::encode(Md5::digest(AUDIO)),"size":AUDIO.len(),"type":"mp3",
        "level":"standard","flag":0,"payed":0}});
    let f = Fixture::host(
        &[COOKIE],
        [
            detail(),
            detail(),
            permission.into(),
            source.into(),
            card().into(),
        ],
    );
    let media = Arc::new(super::support::download::Backend::new(AUDIO.into()));
    let service =
        DownloadService::with_backend(f.session.clone(), media.clone(), 1024 * 1024).unwrap();
    let task = service
        .create_voice(
            &json!(VOICE),
            DownloadDestination::Path {
                path: f.root.path().join("episode.mp3"),
            },
            DownloadOptions {
                tags: false,
                cover: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let mut done = service
        .run(&task.id, DownloadRunOptions::default())
        .await
        .unwrap();
    assert_eq!(done.status, DownloadStatus::Complete);
    let loaded = f
        .client
        .play_downloads(&f.epoch, vec![task.id.clone()], 0)
        .await
        .unwrap();
    assert_eq!(loaded.resource_kind, Some(PlayerResourceKind::Downloaded));
    let play_id = loaded.play_id.unwrap();
    assert_eq!(
        f.http.calls().len(),
        4,
        "download playback must not reauthorize online"
    );
    let songs = f
        .client
        .player_voice_songs(&f.epoch, &play_id)
        .await
        .unwrap();
    assert_eq!(songs.voice_id, VOICE);
    assert_eq!(songs.tracks.len(), 3);
    let calls = f.http.calls();
    assert_eq!(calls.len(), 5);
    assert_eq!(calls[4].path, CARD);
    assert_eq!(calls[4].payload["id"], VOICE);
    assert_eq!(media.audio.calls().len(), 1);

    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    done.song.as_mut().unwrap()["voice"]["id"] = json!(999);
    f.store
        .put(
            "25",
            "download_tasks_v1",
            &task.id,
            &serde_json::to_value(done).unwrap(),
        )
        .unwrap();
    let conflict = f
        .client
        .play_downloads(&f.epoch, vec![task.id], 0)
        .await
        .unwrap();
    assert!(conflict.voice_detail.is_none());
    assert!(
        f.client
            .player_voice_songs(&f.epoch, conflict.play_id.as_deref().unwrap())
            .await
            .is_err()
    );
    assert_eq!(f.http.calls().len(), 5);
}

fn playable_card() -> Value {
    json!({"code":200,"tracks":[
        {"id":301,"name":"第一首","album":{},"privilege":{"id":301,"pl":0,"flag":0}},
        {"id":9,"name":"无专辑未展示"},
        {"id":SONG,"name":"恰好与节目主音轨同ID的普通歌曲","album":{"id":11},
         "privilege":{"id":SONG,"pl":320000,"flag":0},"duration":120000}
    ]})
}

#[tokio::test]
async fn voice_songs_actions_respect_the_table_preference_and_never_toggle_backing_voice() {
    for (action, replace, replace_expected) in [
        (VoiceSongAction::Cover { index: 1 }, true, false),
        (VoiceSongAction::DoubleClick { index: 1 }, false, false),
        (VoiceSongAction::DoubleClick { index: 1 }, true, true),
    ] {
        let media = json!({"code":200,"data":[{"id":SONG,"code":200,
            "url":"https://m1.music.126.net/ordinary.mp3?auth=DO_NOT_EXPORT","md5":"def",
            "level":"standard","expi":120}]});
        let f = Fixture::new(&[COOKIE], [playable_card().into(), media.into()]).await;
        f.client
            .player_action(
                &f.epoch,
                PlayerAction::Mode {
                    mode: PlaybackMode::Sequential,
                },
            )
            .await
            .unwrap();
        if !replace {
            f.client
                .queue_click_preferences_command(
                    &f.epoch,
                    QueueClickPreferencesCommand::Change {
                        expected_revision: "0".into(),
                        replace_queue: false,
                    },
                )
                .unwrap();
        }
        let card = f
            .client
            .player_voice_songs(&f.epoch, &f.play_id)
            .await
            .unwrap();
        let result = f
            .client
            .player_voice_song_play(
                &f.epoch,
                &f.play_id,
                card.receipt.as_deref().unwrap(),
                action,
                Quality::Standard,
            )
            .await
            .unwrap();
        assert_ne!(result.play_id.as_deref(), Some(f.play_id.as_str()));
        assert_eq!(result.resource_kind, Some(PlayerResourceKind::Catalog));
        assert!(result.voice_detail.is_none());
        assert_eq!(result.track.unwrap().id, SONG);
        let queue = f.client.player_queue(&f.epoch).await.unwrap();
        assert_eq!(queue.tracks.len(), 2);
        assert_eq!(
            queue.current.as_deref(),
            Some(queue.tracks[1].entry_id.as_str())
        );
        assert_eq!(queue.tracks[1].track.id, SONG);
        assert!(queue.tracks[1].track.voice_id.is_none());
        if replace_expected {
            assert_eq!(queue.tracks[0].track.id, "301");
            assert!(queue.tracks[0].track.voice_id.is_none());
        } else {
            assert_eq!(queue.tracks[0].track.id, SONG);
            assert_eq!(queue.tracks[0].track.voice_id.as_deref(), Some(VOICE));
        }
        let calls = f.http.calls();
        assert_eq!(calls.len(), STARTUP.len() + 2);
        assert_eq!(calls[STARTUP.len()].path, CARD);
        assert_eq!(
            calls[STARTUP.len() + 1].path,
            "/api/song/enhance/player/url/v1"
        );
        assert!(
            f.client
                .player_voice_song_play(
                    &f.epoch,
                    &f.play_id,
                    card.receipt.as_deref().unwrap(),
                    action,
                    Quality::Standard
                )
                .await
                .is_err()
        );
        assert_eq!(f.http.calls().len(), calls.len());
    }
}

#[tokio::test]
async fn voice_songs_actions_reject_denied_stale_and_renderer_invented_rows_without_transport() {
    let f = Fixture::new(&[COOKIE], [playable_card().into(), playable_card().into()]).await;
    let before = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    let old = f
        .client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap()
        .receipt
        .unwrap();
    let current = f
        .client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap()
        .receipt
        .unwrap();
    assert_ne!(old, current);
    for (receipt, index) in [
        (&*old, 1),
        ("0123456789abcdef0123456789abcdef", 1),
        (&*current, 0),
        (&*current, 2),
    ] {
        assert!(
            f.client
                .player_voice_song_play(
                    &f.epoch,
                    &f.play_id,
                    receipt,
                    VoiceSongAction::DoubleClick { index },
                    Quality::Standard
                )
                .await
                .is_err()
        );
    }
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(
            &target,
            &["MUSIC_U=rotated-episode-songs; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    assert!(matches!(
        f.client
            .player_voice_song_play(
                &f.epoch,
                &f.play_id,
                &current,
                VoiceSongAction::Cover { index: 1 },
                Quality::Standard
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.http.calls().len(), STARTUP.len() + 2);
    let after = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    assert_eq!(before, after);
}

#[tokio::test]
async fn voice_songs_next_play_promotes_and_repeats_without_replacing_the_paused_decoder() {
    let tracks = json!({"code":200,"tracks":[
        {"id":301,"name":"另一首","album":{},"privilege":{"pl":320000}},
        {"id":SONG,"name":"与节目主音轨同ID的普通歌","album":{},"privilege":{"pl":320000}}
    ]});
    // The controller may perform its usual upcoming-track prefetch. Both
    // sealed URLs are valid fixture responses; prefetch must never load them.
    let media = json!({"code":200,"data":[
        {"id":301,"code":200,"url":"https://m1.music.126.net/other.mp3?auth=DO_NOT_EXPORT","md5":"def","level":"standard","expi":120},
        {"id":SONG,"code":200,"url":"https://m1.music.126.net/ordinary.mp3?auth=DO_NOT_EXPORT","md5":"ghi","level":"standard","expi":120}
    ]});
    let f = Fixture::new(
        &[COOKIE],
        [tracks.into()]
            .into_iter()
            .chain(std::iter::repeat_with(|| media.clone().into()).take(8)),
    )
    .await;
    let card = f
        .client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap();
    let receipt = card.receipt.as_deref().unwrap();
    f.client
        .player_action(
            &f.epoch,
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    f.client
        .player_action(&f.epoch, PlayerAction::Seek { seconds: 25.0 })
        .await
        .unwrap();
    let paused = f
        .client
        .player_action(&f.epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    assert_eq!(paused.position, Some(25.0));
    let counters = || {
        let native = f.device.state.lock().unwrap();
        (
            native.loads,
            native.stops,
            native.pauses.clone(),
            native.view.entry_id,
        )
    };
    let native_before = counters();
    let mut matched_entry = None;
    let mut final_revision = None;
    for (stage, index, expected) in [
        (0, 1, [SONG, SONG, ""]),
        (1, 0, [SONG, "301", SONG]),
        (2, 1, [SONG, SONG, "301"]),
        (3, 1, [SONG, SONG, "301"]),
    ] {
        let state = f
            .client
            .player_voice_song_play(
                &f.epoch,
                &f.play_id,
                receipt,
                VoiceSongAction::NextPlay { index },
                Quality::Standard,
            )
            .await
            .unwrap();
        assert_eq!(state.play_id, paused.play_id);
        assert_eq!(state.entry_id, paused.entry_id);
        assert_eq!(state.position, paused.position);
        assert_eq!(state.state, paused.state);
        assert_eq!(state.mode, PlaybackMode::Random);
        assert_eq!(counters(), native_before);
        let queue = f.client.player_queue(&f.epoch).await.unwrap();
        assert_eq!(queue.current, paused.entry_id);
        assert_eq!(queue.tracks[0].track.voice_id.as_deref(), Some(VOICE));
        assert_eq!(
            queue
                .tracks
                .iter()
                .map(|row| row.track.id.as_str())
                .collect::<Vec<_>>(),
            expected
                .into_iter()
                .filter(|id| !id.is_empty())
                .collect::<Vec<_>>()
        );
        let entry = queue
            .tracks
            .iter()
            .find(|row| row.track.id == SONG && row.track.voice_id.is_none())
            .unwrap();
        assert_eq!(
            matched_entry.get_or_insert_with(|| entry.entry_id.clone()),
            &entry.entry_id
        );
        let native_queue = cumulus_core::playback::PlaybackQueue::new(Some(f.store.clone()), "25")
            .unwrap()
            .snapshot();
        let current = native_queue
            .order
            .iter()
            .position(|id| Some(id) == native_queue.current.as_ref())
            .unwrap();
        assert_eq!(
            native_queue.order[current + 1],
            queue.tracks[1].entry_id,
            "explicit next is pinned in shuffle too"
        );
        if stage == 2 {
            final_revision = Some(state.queue_revision);
        } else if stage == 3 {
            assert_eq!(final_revision.as_ref(), Some(&state.queue_revision));
        }
    }
    let calls = f.http.calls();
    assert_eq!(calls[STARTUP.len()].path, CARD);
    assert!(
        calls[STARTUP.len() + 1..]
            .iter()
            .all(|call| call.path == "/api/song/enhance/player/url/v1")
    );
}

#[tokio::test]
async fn voice_songs_next_play_rejects_unavailable_and_stale_rows_before_queue_changes() {
    let card = json!({"code":200,"tracks":[
        {"id":301,"album":{},"privilege":{"pl":0,"flag":0}},
        {"id":302,"album":{},"privilege":{"pl":320000,"st":-200}},
        {"id":303,"album":{},"privilege":{"pl":320000,"st":"-100"}},
        {"id":304,"album":{},"privilege":{"pl":320000}}
    ]});
    let f = Fixture::new(&[COOKIE], [card.into()]).await;
    let initial = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    let card = f
        .client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap();
    let receipt = card.receipt.as_deref().unwrap();
    assert_eq!(
        serde_json::to_value(&card.availability).unwrap(),
        json!([
            {"onlineEligible":false,"copyrightBlocked":false,"groupDownload":"omitted"},
            {"onlineEligible":true,"copyrightBlocked":true,"groupDownload":"omitted"},
            {"onlineEligible":true,"copyrightBlocked":true,"groupDownload":"omitted"},
            {"onlineEligible":true,"copyrightBlocked":false,"groupDownload":"omitted"}
        ])
    );
    for index in 0..3 {
        assert!(matches!(
            f.client
                .player_voice_song_play(
                    &f.epoch,
                    &f.play_id,
                    receipt,
                    VoiceSongAction::NextPlay { index },
                    Quality::Standard
                )
                .await,
            Err(Error::State("vinyl episode song is unavailable online"))
        ));
    }
    assert_eq!(
        initial,
        serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap()
    );
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(matches!(
        f.client
            .player_voice_song_play(
                &f.epoch,
                &f.play_id,
                receipt,
                VoiceSongAction::NextPlay { index: 3 },
                Quality::Standard
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.http.calls().len(), STARTUP.len() + 1);
}
