use super::{HttpResponse, ScriptHttp, gate, playback, reply, vinyl_http_with_credentials};
use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{PlaySelection, PlayerAction},
    },
    history::HistorySyncAction,
    protocol::{
        api::Api,
        http::{ApiOrigin, MusicHttpClient},
    },
};
use serde_json::{Value, json};
use std::{future::poll_fn, sync::Arc, task::Poll};

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
    _root: tempfile::TempDir,
    native: Arc<MusicHttpClient>,
    credentials: Arc<Credentials>,
    client: MusicClient,
    http: Arc<ScriptHttp>,
    epoch: String,
    play_id: String,
}

impl Fixture {
    async fn new(cookies: &[&str], replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let episode = json!({"code":200,"program":{
            "id":VOICE,"name":"合成节目","duration":120000,"mainTrackId":SONG,
            "mainSong":{"id":SONG,"name":"合成节目音轨","duration":120000},
            "songs":[{"id":9,"name":"DO_NOT_EXPORT_EMBEDDED","al":{"id":9}}]
        }});
        let (native, http, credentials) = vinyl_http_with_credentials(root.path(), cookies, [
            episode.clone().into(), episode.into(),
            json!({"code":200,"data":{"voiceId":VOICE,"songId":SONG,"canPlay":true}}).into(),
            json!({"code":200,"data":[{"id":SONG,"code":200,"url":"https://m1.music.126.net/episode.mp3?auth=DO_NOT_EXPORT","md5":"abc","level":"standard","expi":120}]}).into(),
        ].into_iter().chain(replies));
        let registry = ProfileRegistry::open(&root.path().join("host"), ApiOrigin::Pc).unwrap();
        let session = Arc::new(AccountSession::new(native.clone(), Some(registry.store())));
        session
            .adopt(
                native.clone(),
                json!({"userId":25}),
                &session.token().unwrap(),
            )
            .unwrap();
        let epoch = session.token().unwrap().generation.to_string();
        let client = MusicClient::with_session(session, registry).with_audio_factory(|| {
            Ok(playback::audio_engine(
                Arc::new(playback::Device::default()),
            ))
        });
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
        let play_id = client
            .play_selection(&epoch, PlaySelection::Voice { id: VOICE.into() })
            .await
            .unwrap()
            .play_id
            .unwrap();
        assert_eq!(
            http.calls()
                .iter()
                .map(|call| call.path.as_str())
                .collect::<Vec<_>>(),
            STARTUP
        );
        Self {
            _root: root,
            native,
            credentials,
            client,
            http,
            epoch,
            play_id,
        }
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
