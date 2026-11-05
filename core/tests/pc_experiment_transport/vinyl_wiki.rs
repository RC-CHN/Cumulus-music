use super::{HttpResponse, ScriptHttp, gate, playback, reply, vinyl_http};
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{PlaySelection, PlayerAction},
    },
    membership::Quality,
    protocol::{
        api::Api,
        http::{ApiOrigin, MusicHttpClient},
    },
};
use serde_json::{Value, json};
use std::{future::poll_fn, sync::Arc, task::Poll};

const WIKI: &str = "/api/music/wiki/home/song/desktop/get";
const HOLD: &str = "/api/w/nuser/account/get";
const SONG: &str = "424242";

struct Fixture {
    _root: tempfile::TempDir,
    native: Arc<MusicHttpClient>,
    client: MusicClient,
    http: Arc<ScriptHttp>,
    epoch: String,
    play_id: String,
}

impl Fixture {
    async fn new(cookies: &[&str], wiki_replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let (native, http) = vinyl_http(root.path(), cookies, [
            json!({"code":200,"songs":[{"id":SONG,"name":"合成曲目","dt":120000}]}).into(),
            json!({"code":200,"data":[{"id":SONG,"code":200,"url":"https://m1.music.126.net/test.mp3?auth=SYNTHETIC","md5":"abc","level":"standard","expi":120}]}).into(),
        ].into_iter().chain(wiki_replies));
        let registry = ProfileRegistry::open(&root.path().join("host"), ApiOrigin::Pc).unwrap();
        let session = Arc::new(AccountSession::new(native.clone(), Some(registry.store())));
        session
            .adopt(
                native.clone(),
                json!({"userId":71}),
                &session.token().unwrap(),
            )
            .unwrap();
        let epoch = session.token().unwrap().generation.to_string();
        let client = MusicClient::with_session(session, registry).with_audio_factory(|| {
            Ok(playback::audio_engine(
                Arc::new(playback::Device::default()),
            ))
        });
        let play_id = client
            .play_selection(
                &epoch,
                PlaySelection::Songs {
                    ids: vec![SONG.into()],
                    index: 0,
                    quality: Quality::Standard,
                },
            )
            .await
            .unwrap()
            .play_id
            .unwrap();
        Self {
            _root: root,
            native,
            client,
            http,
            epoch,
            play_id,
        }
    }
}

fn metadata() -> Value {
    json!({"code":200,"data":{"wikiUrl":"https://private.invalid/path?token=SHOULD_NOT_CROSS_IPC",
    "wikiSubBlockBaseInfoVo":{"wikiSubElementVos":[
        {"title":"发行日期","pattern":0,"content":"2024年1月1日","credential":"SHOULD_NOT_CROSS_IPC"},
        {"title":"风格","pattern":1,"content":"忽略这一栏","wikiSubMetaVos":[{"text":"摇滚"},{"text":"独立"}]},
        {"title":"空内容","content":""},{"content":"空标题"},
        {"title":"普通字符串","content":"<em>保留文字</em>"},
        {"title":"字符串模式不当作数字","pattern":"1","content":"原值"}
    ]}}})
}

#[tokio::test]
async fn vinyl_wiki_uses_one_exact_pc_signed_read_and_keeps_only_plain_rows() {
    let mut oversized = metadata();
    oversized["data"]["wikiSubBlockBaseInfoVo"]["wikiSubElementVos"][0]["content"] =
        json!("a".repeat(4097));
    let f = Fixture::new(
        &["MUSIC_U=synthetic-vinyl-wiki; Path=/; Secure"],
        [
            metadata().into(),
            json!({"code":"200","data":metadata()["data"]}).into(),
            json!({"code":200,"data":{"wikiSubBlockBaseInfoVo":{"wikiSubElementVos":[]}}}).into(),
            oversized.into(),
            json!({"code":403}).into(),
        ],
    )
    .await;
    assert!(matches!(
        f.client.player_vinyl_wiki(&f.epoch, "expired-play").await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        f.client.player_vinyl_wiki("other-epoch", &f.play_id).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.http.calls().len(), 2);
    let view = f
        .client
        .player_vinyl_wiki(&f.epoch, &f.play_id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(view).unwrap(),
        json!({"playId":f.play_id,"songId":SONG,"card":{"metas":[
            {"title":"发行日期","content":"2024年1月1日"},{"title":"风格","content":"摇滚、独立"},
            {"title":"普通字符串","content":"<em>保留文字</em>"},{"title":"字符串模式不当作数字","content":"原值"}
        ]}})
    );
    for _ in 0..2 {
        assert!(
            f.client
                .player_vinyl_wiki(&f.epoch, &f.play_id)
                .await
                .unwrap()
                .card
                .is_none()
        );
    }
    assert!(matches!(
        f.client.player_vinyl_wiki(&f.epoch, &f.play_id).await,
        Err(Error::Response(_))
    ));
    assert!(
        f.client
            .player_vinyl_wiki(&f.epoch, &f.play_id)
            .await
            .err()
            .unwrap()
            .is_service_code(403)
    );
    let calls = f.http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/v3/song/detail",
            "/api/song/enhance/player/url/v1",
            WIKI,
            WIKI,
            WIKI,
            WIKI,
            WIKI,
        ]
    );
    for call in &calls[2..] {
        let mut payload = call.payload.clone();
        assert!(
            payload
                .as_object_mut()
                .unwrap()
                .remove("header")
                .unwrap()
                .is_string()
        );
        assert_eq!(
            payload,
            json!({"songId":SONG,"_scver":"1","_sign":"332083521190d35a96829d59aac97a3e"})
        );
        assert!(
            call.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-vinyl-wiki")
        );
        assert!(call.login_chain.is_none());
    }
}

#[tokio::test]
async fn vinyl_wiki_fences_the_current_play_before_send_and_after_reply() {
    for already_dispatched in [false, true] {
        let (held, release) = gate();
        let f = Fixture::new(&["MUSIC_U=synthetic-vinyl-wiki; Path=/; Secure"], [held]).await;
        let occupying = if already_dispatched {
            None
        } else {
            let native = f.native.clone();
            let occupying =
                tokio::spawn(async move { native.request(HOLD, json!({}), false).await });
            f.http.wait(3).await;
            Some(occupying)
        };
        let mut queued = Box::pin(f.client.player_vinyl_wiki(&f.epoch, &f.play_id));
        assert!(
            poll_fn(|cx| Poll::Ready(queued.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        f.http.wait(3).await;
        let stopped = f
            .client
            .player_action(&f.epoch, PlayerAction::Stop)
            .await
            .unwrap();
        assert_ne!(stopped.play_id.as_deref(), Some(f.play_id.as_str()));
        assert!(release.send(Ok(reply(metadata()))).is_ok());
        if let Some(occupying) = occupying {
            occupying.await.unwrap().unwrap();
        }
        assert!(matches!(queued.await, Err(Error::StaleResponse)));
        let calls = f.http.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[2].path, if already_dispatched { WIKI } else { HOLD });
    }
}

#[tokio::test]
async fn vinyl_wiki_rejects_same_revision_other_cookie_scope_without_sending() {
    let f = Fixture::new(
        &[
            "MUSIC_U=synthetic-account; Path=/; Secure",
            "MUSIC_U=synthetic-other-scope; Path=/eapi/music/wiki/; Secure",
        ],
        [],
    )
    .await;
    assert!(matches!(
        f.client.player_vinyl_wiki(&f.epoch, &f.play_id).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.http.calls().len(), 2);
}
