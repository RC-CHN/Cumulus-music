use super::{HttpResponse, ScriptHttp, gate, playback, reply, vinyl_http_with_credentials};
use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials},
    client::{
        MusicClient,
        dto::{PlaySelection, PlayerAction, VinylVoiceRecommendationsView},
    },
    history::HistorySyncAction,
    protocol::{
        api::Api,
        http::{ApiOrigin, MusicHttpClient},
    },
};
use serde_json::{Value, json};
use std::{future::poll_fn, sync::Arc, task::Poll};

const CARD: &str = "/api/voice/play/page/rcmd";
pub(super) const HOLD: &str = "/api/w/nuser/account/get";
pub(super) const COOKIE: &str = "MUSIC_U=synthetic-voice-rcmd; Path=/; Secure";
pub(super) const VOICE: &str = "71";
pub(super) const SONG: &str = "424242";
pub(super) const STARTUP: [&str; 4] = [
    "/api/dj/program/detail",
    "/api/dj/program/detail",
    "/api/dj/play/url/check",
    "/api/song/enhance/player/url/v1",
];

pub(super) struct Fixture {
    _root: tempfile::TempDir,
    pub(super) native: Arc<MusicHttpClient>,
    pub(super) session: Arc<AccountSession>,
    pub(super) credentials: Arc<Credentials>,
    pub(super) client: MusicClient,
    pub(super) http: Arc<ScriptHttp>,
    pub(super) epoch: String,
    pub(super) play_id: String,
}

impl Fixture {
    pub(super) async fn new(
        cookies: &[&str],
        replies: impl IntoIterator<Item = HttpResponse>,
    ) -> Self {
        Self::build(cookies, replies, false).await
    }

    pub(super) async fn with_existing_voice_targets(
        cookies: &[&str],
        replies: impl IntoIterator<Item = HttpResponse>,
    ) -> Self {
        Self::build(cookies, replies, true).await
    }

    async fn build(
        cookies: &[&str],
        replies: impl IntoIterator<Item = HttpResponse>,
        existing_targets: bool,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut episode = json!({"code":200,"program":{
            "id":VOICE,"name":"合成节目","duration":120000,"mainTrackId":SONG,
            "coverUrl":"http://p1.music.126.net/current.png?auth=DO_NOT_EXPORT",
            "mainSong":{"id":SONG,"name":"合成音轨","duration":120000}
        }});
        let mut extra = Vec::<HttpResponse>::new();
        if existing_targets {
            episode["program"]["creator"] = json!({"userId":"54","nickname":"原播放主播"});
            episode["program"]["radio"] = json!({"id":"800","name":"原播放播单"});
            extra.push(json!({"code":200,"data":{"resumePlayValue":0}}).into());
        }
        let (native, http, credentials) = vinyl_http_with_credentials(
            root.path(), cookies, [
                episode.clone().into(), episode.into(),
                json!({"code":200,"data":{"voiceId":VOICE,"songId":SONG,"canPlay":true}}).into(),
                json!({"code":200,"data":[{"id":SONG,"code":200,"url":"https://m1.music.126.net/episode.mp3?auth=DO_NOT_EXPORT","md5":"abc","level":"standard","expi":120}]}).into(),
            ].into_iter().chain(extra).chain(replies),
        );
        let registry = super::fixture_registry(&root.path().join("host"));
        let session = Arc::new(AccountSession::new(native.clone(), Some(registry.store())));
        session
            .adopt(
                native.clone(),
                json!({"userId":25}),
                &session.token().unwrap(),
            )
            .unwrap();
        let epoch = session.token().unwrap().generation.to_string();
        let client =
            MusicClient::with_session(session.clone(), registry).with_audio_factory(|| {
                Ok(playback::audio_engine(
                    Arc::new(playback::Device::default()),
                ))
            });
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
        let mut startup = STARTUP.to_vec();
        if existing_targets {
            startup.push("/api/dj/user/setting/resumeplay/get");
        }
        assert_eq!(
            http.calls()
                .iter()
                .map(|call| call.path.as_str())
                .collect::<Vec<_>>(),
            startup
        );
        Self {
            _root: root,
            native,
            session,
            credentials,
            client,
            http,
            epoch,
            play_id,
        }
    }

    async fn read(&self) -> cumulus_core::Result<VinylVoiceRecommendationsView> {
        self.client
            .player_vinyl_voice_recommendations(&self.epoch, &self.play_id)
            .await
    }

    fn next(&self, receipt: &str) -> cumulus_core::Result<VinylVoiceRecommendationsView> {
        self.client
            .player_vinyl_voice_recommendations_next(&self.epoch, &self.play_id, receipt)
    }

    pub(super) fn rotate(&self) {
        let target = ApiOrigin::Pc.url();
        let revision = self
            .credentials
            .request_credentials(&target)
            .unwrap()
            .revision;
        self.credentials
            .record_response(
                &target,
                &["MUSIC_U=synthetic-voice-rcmd-rotated; Path=/; Secure".into()],
                None,
                &revision,
            )
            .unwrap();
    }

    fn wire(&self) {
        for call in &self.http.calls()[STARTUP.len()..] {
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
            assert_eq!(payload, json!({"voiceId":VOICE}));
            assert!(
                call.cookie
                    .as_deref()
                    .unwrap()
                    .contains("MUSIC_U=synthetic-voice-rcmd")
            );
            assert!(call.login_chain.is_none());
        }
    }
}

fn data(count: u32) -> Value {
    let rows: Vec<_> = (1..=count)
        .map(|n| json!({"id":n,"radioId":800+n,"programId":900+n,
            "name":format!("合成播单 {n}"),"playCount":n.to_string(),
            "picUrl":"http://p1.music.126.net/test/art.jpg?token=DO_NOT_EXPORT",
            "coverUrl":"https://evil.invalid/DO_NOT_EXPORT","url":"https://evil.invalid/DO_NOT_EXPORT"}))
        .collect();
    json!({"code":200,"data":rows})
}

fn ids(view: &VinylVoiceRecommendationsView) -> Vec<u32> {
    view.cards
        .iter()
        .map(|card| card.radio_id.parse().unwrap())
        .collect()
}

#[tokio::test]
async fn voice_recommendations_uses_row_id_directly_and_cycles_locally_without_any_wire() {
    let f = Fixture::new(
        &[COOKIE],
        [
            data(10).into(),
            data(9).into(),
            json!({"code":403}).into(),
            data(3).into(),
        ],
    )
    .await;
    let initial = f.read().await.unwrap();
    assert_eq!(
        (
            &*initial.play_id,
            &*initial.voice_id,
            initial.page,
            initial.page_count
        ),
        (&*f.play_id, VOICE, 1, 2)
    );
    assert_eq!(ids(&initial), [1, 2, 3, 4, 5, 6]);
    let json = serde_json::to_value(&initial).unwrap();
    assert_eq!(
        json["cards"][0],
        json!({
            "radioId":"1","name":"合成播单 1","coverUrl":"https://p1.music.126.net/test/art.jpg?param=320y320","playCount":"1"
        })
    );
    assert!(!json.to_string().contains("DO_NOT_EXPORT"));
    let paused = f
        .client
        .player_action(&f.epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let first = initial.receipt;
    let target = |page, id| {
        f.client
            .player_vinyl_voice_recommendation_target(&f.epoch, &f.play_id, &first, page, id)
    };
    assert_eq!(target(1, "1").unwrap(), "1");
    assert!(matches!(target(1, "7"), Err(Error::StaleResponse)));
    let second = f.next(&first).unwrap();
    assert_eq!((second.page, second.page_count), (2, 2));
    assert_eq!(ids(&second), [7, 8, 9, 10]);
    assert!(matches!(target(1, "1"), Err(Error::StaleResponse)));
    assert_eq!(target(2, "7").unwrap(), "7");
    let wrapped = f.next(&first).unwrap();
    assert_eq!(
        (wrapped.page, wrapped.receipt.as_str()),
        (1, first.as_str())
    );
    assert_eq!(ids(&wrapped), [1, 2, 3, 4, 5, 6]);
    assert_eq!(f.http.calls().len(), STARTUP.len() + 1);
    let after = f.client.player_snapshot(&f.epoch).await.unwrap();
    assert_eq!(
        (
            &after.play_id,
            &after.entry_id,
            &after.queue_revision,
            after.position,
            after.state
        ),
        (
            &paused.play_id,
            &paused.entry_id,
            &paused.queue_revision,
            paused.position,
            paused.state
        )
    );
    let only = f.read().await.unwrap();
    assert_eq!((only.page, only.page_count), (1, 1));
    assert_eq!(ids(&only), [1, 2, 3, 4, 5, 6]);
    assert_eq!(ids(&f.next(&only.receipt).unwrap()), ids(&only));
    assert!(matches!(f.next(&first), Err(Error::StaleResponse)));
    let denied = f.read().await.unwrap_err();
    assert!(denied.is_service_code(403));
    assert_eq!(ids(&f.next(&only.receipt).unwrap()), ids(&only));
    let empty = f.read().await.unwrap();
    assert_eq!((empty.page, empty.page_count, empty.cards.len()), (0, 0, 0));
    assert_eq!(f.next(&empty.receipt).unwrap().page, 0);
    assert_eq!(f.http.calls().len(), STARTUP.len() + 4);
    f.wire();
}

#[tokio::test]
async fn voice_recommendations_rejects_old_receipt_and_late_read_after_play_or_credentials_change()
{
    for change in ["stop", "cookie", "account"] {
        let (held, release) = gate();
        let f = Fixture::new(&[COOKIE], [data(12).into(), held]).await;
        let before = f.read().await.unwrap();
        let mut pending = Box::pin(f.read());
        assert!(
            poll_fn(|cx| Poll::Ready(pending.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        f.http.wait(STARTUP.len() + 2).await;
        match change {
            "cookie" => f.rotate(),
            "account" => {
                f.session
                    .adopt(
                        f.native.clone(),
                        json!({"userId":26}),
                        &f.session.token().unwrap(),
                    )
                    .unwrap();
            }
            "stop" => {
                f.client
                    .player_action(&f.epoch, PlayerAction::Stop)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(release.send(Ok(reply(data(10)))).is_ok());
        assert!(matches!(pending.await, Err(Error::StaleResponse)));
        assert!(matches!(f.next(&before.receipt), Err(Error::StaleResponse)));
        assert!(matches!(
            f.client.player_vinyl_voice_recommendation_target(
                &f.epoch,
                &f.play_id,
                &before.receipt,
                1,
                "1",
            ),
            Err(Error::StaleResponse)
        ));
        if change == "account" {
            let fresh = f.session.token().unwrap().generation.to_string();
            assert!(matches!(
                f.client.player_vinyl_voice_recommendations_next(
                    &fresh,
                    &f.play_id,
                    &before.receipt
                ),
                Err(Error::StaleResponse)
            ));
        }
        assert_eq!(f.http.calls().len(), STARTUP.len() + 2);
        f.wire();
    }
}

#[tokio::test]
async fn voice_recommendations_cannot_dispatch_under_a_different_scoped_cookie_or_stale_play() {
    let f = Fixture::new(
        &[
            COOKIE,
            "MUSIC_U=synthetic-other; Path=/eapi/voice/play/page/; Secure",
        ],
        [],
    )
    .await;
    assert!(matches!(
        f.client
            .player_vinyl_voice_recommendations(&f.epoch, "stale-play")
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(f.read().await, Err(Error::StaleResponse)));
    assert_eq!(f.http.calls().len(), STARTUP.len());
}

#[tokio::test]
async fn voice_recommendations_guard_rechecks_live_play_before_transport_dispatch() {
    let (held, release) = gate();
    let f = Fixture::new(&[COOKIE], [held]).await;
    let native = f.native.clone();
    let occupying = tokio::spawn(async move { native.request(HOLD, json!({}), false).await });
    f.http.wait(STARTUP.len() + 1).await;
    let mut pending = Box::pin(f.read());
    assert!(
        poll_fn(|cx| Poll::Ready(pending.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    occupying.await.unwrap().unwrap();
    assert!(matches!(pending.await, Err(Error::StaleResponse)));
    let calls = f.http.calls();
    assert_eq!(calls.len(), STARTUP.len() + 1);
    assert_eq!(calls.last().unwrap().path, HOLD);
}
