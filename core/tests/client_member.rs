#[allow(dead_code)]
#[path = "support/fm.rs"]
mod backend;
#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    membership::Quality,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::sync::Arc;
const ROOT: &str = "/api/vipnewcenter/app/";
fn fixture() -> (
    tempfile::TempDir,
    Arc<backend::Backend>,
    Arc<AccountSession>,
    Arc<MusicClient>,
) {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(backend::Backend::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = Arc::new(
        MusicClient::with_session(session.clone(), registry)
            .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default())))),
    );
    let put = |suffix: &str, value: Value| {
        api.responses
            .lock()
            .unwrap()
            .insert(format!("{ROOT}{suffix}"), json!({"code":200,"data":value}));
    };
    put(
        "viptab/module/get",
        json!([{"moduleType":1,"showName":"banner"},{"moduleType":4,"showName":"VIP每日推荐"},{"moduleType":9,"showName":"榜单"},{"moduleType":5,"showName":"有声书"},{"moduleType":6,"showName":"专辑"},{"moduleType":7,"showName":"banner"},{"moduleType":12,"showName":"歌单"}]),
    );
    put(
        "resource/centertop/carousel/list",
        json!({"carousels":[{"content":"会员权益","private":"PRIVATE"}],"visitorId":"PRIVATE"}),
    );
    put(
        "resource/common/list",
        json!({"contentList":[
          {"title":"fallback","imgUrl":"https://p1.music.126.net/art?auth=PRIVATE","resourceType":"PLAYLIST","resourceId":11},
          {"title":"official activity","imgUrl":"https://p1.music.126.net/art","jumpUrl":"https://music.163.com/g/vip-portal?scene=pc"},
          {"title":"invalid activity","imgUrl":"https://evil.test/PRIVATE","jumpUrl":"javascript:alert('PRIVATE')"}
        ]}),
    );
    put(
        "viptab/recommend/song/list",
        json!({"songInfoList":[song(1),song(2),song(3)]}),
    );
    put(
        "viptab/ranking/list",
        json!([{"rankingListId":11,"rankingListName":"VIP榜","contentList":[{"songId":1},{"songId":4,"name":"missing"}],"songIds":[1,2,3,4]}]),
    );
    put(
        "resource/plus/recomend/album",
        json!([{"albumId":22,"albumName":"专辑","artistName":"歌手","cover":"https://p1.music.126.net/art?secret=PRIVATE"}]),
    );
    put(
        "viptab/recommend/playList/list",
        json!([{"id":11,"title":"歌单","coverUrl":"https://p1.music.126.net/art"}]),
    );
    let mut map = api.responses.lock().unwrap();
    map.insert(
        "/api/v3/song/detail".into(),
        json!({"code":200,"songs":[song(1),song(2),song(3)]}),
    );
    map.insert("/api/podcast/common/single/block/get".into(),json!({"code":200,"data":{"blockVOS":[{"creatives":[{"uiElement":{"mainTitle":{"title":"有声书"},"subTitle":{"title":"作者"},"image":{"imageUrl":"https://p1.music.126.net/art"}},"creativeExtInfoVO":{"radio":{"id":33,"private":"PRIVATE"}},"source":"PRIVATE"}]}],"context":{"token":"PRIVATE"}}}));
    map.insert(
        "/api/discovery/recommend/songs/history/recent".into(),
        json!({"code":200,"data":{"dates":["2026-10-26","2026-10-25"]}}),
    );
    map.insert(
        "/api/discovery/recommend/songs/history/detail".into(),
        json!({"code":200,"data":{"songs":[song(2),song(1)]}}),
    );
    drop(map);
    (dir, api, session, client)
}
fn song(id: u64) -> Value {
    json!({"id":id,"name":format!("歌曲{id}"),"ar":[{"name":"歌手"}],"al":{"name":"专辑","picUrl":"https://p1.music.126.net/art?private=PRIVATE"},"dt":180000,"alg":"PRIVATE","url":"https://private/PRIVATE"})
}

#[tokio::test]
async fn member_card_offer_uses_exact_delivery_slot_and_invalidates_old_actions() {
    const SLOT: &str = "/api/delivery/deliver";
    const PORTAL: &str = "https://music.163.com/g/vip-portal?nm_style=sbt&scene=pc_viptab_scan";
    let (_dir, api, session, client) = fixture();
    let response = |link: &str| {
        json!({"code":200,"data":{"resourceData":{
            "text":"查看合成优惠", "pcSideDisplay":"4", "jumpUrl":link
        },"logContext":"PRIVATE"}})
    };
    api.responses
        .lock()
        .unwrap()
        .insert(SLOT.into(), response(PORTAL));
    let offer = serde_json::to_value(client.member_card_offer("1", false).await.unwrap()).unwrap();
    let revision = offer["value"]["revision"].as_str().unwrap();
    assert_eq!(offer["value"]["mode"], "browser");
    assert_eq!(offer["value"]["label"], "查看合成优惠");
    assert!(!offer.to_string().contains("PRIVATE"));
    assert!(!offer.to_string().contains(PORTAL));
    assert_eq!(
        serde_json::to_value(client.member_card_offer("1", false).await.unwrap()).unwrap(),
        offer
    );
    assert_eq!(
        client.member_card_external_url("1", revision).unwrap(),
        format!("{PORTAL}&source=pc")
    );
    assert_eq!(
        *api.calls.lock().unwrap(),
        [(SLOT.into(), json!({"positionId":65}))]
    );
    api.responses
        .lock()
        .unwrap()
        .insert(SLOT.into(), response(&format!("{PORTAL}&sso=PRIVATE")));
    let blocked = serde_json::to_value(client.member_card_offer("1", true).await.unwrap()).unwrap();
    assert_eq!(blocked["value"]["mode"], "app");
    assert!(blocked["value"]["label"].is_null());
    assert!(matches!(
        client.member_card_external_url("1", revision),
        Err(Error::StaleResponse)
    ));
    *api.hold.lock().unwrap() = Some(SLOT.into());
    let pending = tokio::spawn(async move { client.member_card_offer("1", true).await });
    api.started.notified().await;
    session.detach().unwrap();
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(api.count(SLOT), 3);
}

#[tokio::test]
async fn member_offer_qr_requires_a_current_mode_three_receipt() {
    const SLOT: &str = "/api/delivery/deliver";
    const PORTAL: &str = "https://music.163.com/g/vip-portal?nm_style=sbt&scene=pc_viptab_scan";
    let (_dir, api, session, client) = fixture();
    let respond = |mode: &str, url: &str| {
        api.responses.lock().unwrap().insert(
            SLOT.into(),
            json!({"code":200,"data":{"resourceData":{
                "text":"查看合成优惠","pcSideDisplay":mode,"jumpUrl":url
            },"logContext":"PRIVATE"}}),
        );
    };
    respond("3", PORTAL);
    let offer = serde_json::to_value(client.member_card_offer("1", false).await.unwrap()).unwrap();
    let revision = offer["value"]["revision"].as_str().unwrap();
    let qr = client.member_card_offer_qr("1", revision).unwrap();
    assert_eq!(qr.epoch, "1");
    assert_eq!(
        qr.value.modules,
        cumulus_core::membership::portal::member_portal_qr()
            .unwrap()
            .modules
    );
    assert!(matches!(
        client.member_card_offer_qr("1", "different"),
        Err(Error::StaleResponse)
    ));
    assert!(client.member_card_offer_qr("2", revision).is_err());
    assert!(matches!(
        client.member_card_external_url("1", revision),
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.count(SLOT), 1, "rendering never reloads delivery");

    respond("4", PORTAL);
    let next = serde_json::to_value(client.member_card_offer("1", true).await.unwrap()).unwrap();
    let next_revision = next["value"]["revision"].as_str().unwrap();
    assert!(matches!(
        client.member_card_offer_qr("1", revision),
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.member_card_offer_qr("1", next_revision),
        Err(Error::StaleResponse)
    ));
    respond("3", PORTAL);
    let fresh = serde_json::to_value(client.member_card_offer("1", true).await.unwrap()).unwrap();
    let fresh_revision = fresh["value"]["revision"].as_str().unwrap();
    session.detach().unwrap();
    assert!(client.member_card_offer_qr("1", fresh_revision).is_err());
}

#[tokio::test]
async fn member_sign_uses_recorded_ids_and_rejects_late_account_history() {
    const CARD: &str = "/api/vipnewcenter/app/minidesk/music/sign/pc";
    const HISTORY: &str = "/api/vipnewcenter/app/level/user/checkin/history/list";
    const DETAIL: &str = "/api/vipnewcenter/app/level/user/checkin/history/detail";
    const DATE: u64 = 1_793_318_400_000;
    let (_dir, api, session, client) = fixture();
    api.responses.lock().unwrap().extend([
        (CARD.into(), json!({"code":200,"data":{"signInfoList":[
            {"dayText":"30日","today":false,"sign":true,"signTime":DATE,"songCoverUrl":"https://p1.music.126.net/cover?PRIVATE"},
            {"dayText":"31日","today":true,"sign":false,"signTime":DATE,"songCoverUrl":"https://bad.invalid/PRIVATE"}
        ]}})),
        (HISTORY.into(), json!({"code":200,"data":[{"dayTime":DATE,"recordId":"009007199254740993"}]})),
        (DETAIL.into(), json!({"code":200,"data":{"songInfo":{"songId":"9007199254740993","songName":"合成乐签"},"private":"PRIVATE"}})),
    ]);
    let card = serde_json::to_value(client.member_sign_card("1").await.unwrap()).unwrap();
    assert_eq!(card["value"]["days"][0]["signTime"], DATE);
    assert!(card["value"]["days"][1]["signTime"].is_null());
    assert!(!card.to_string().contains("PRIVATE"));
    assert!(matches!(
        client.member_sign_detail("1", DATE + 1).await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(api.count(DETAIL), 0);
    let detail = serde_json::to_value(client.member_sign_detail("1", DATE).await.unwrap()).unwrap();
    assert_eq!(detail["value"]["songId"], "9007199254740993");
    assert!(!detail.to_string().contains("PRIVATE"));
    assert!(!detail.to_string().contains("009007199254740993"));
    assert_eq!(
        api.calls.lock().unwrap().as_slice(),
        &[
            (CARD.into(), json!({"type":1})),
            (HISTORY.into(), json!({})),
            (HISTORY.into(), json!({})),
            (
                DETAIL.into(),
                json!({"type":2,"recordId":"009007199254740993"})
            ),
        ]
    );
    *api.hold.lock().unwrap() = Some(HISTORY.into());
    let pending = tokio::spawn(async move { client.member_sign_detail("1", DATE).await });
    api.started.notified().await;
    session.detach().unwrap();
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(api.count(DETAIL), 1);
}

#[tokio::test]
async fn all_official_module_shapes_become_safe_actions_and_cache_survives_reopen() {
    let (dir, api, _session, client) = fixture();
    let page = client
        .member_page("1", MemberQuery::Home, true)
        .await
        .unwrap();
    let serialized = serde_json::to_value(&page).unwrap();
    assert!(!serialized.to_string().contains("PRIVATE"));
    let modules = serialized["value"]["page"]["modules"].as_array().unwrap();
    assert_eq!(modules.len(), 7);
    assert_eq!(modules[1]["content"]["tracks"].as_array().unwrap().len(), 3);
    assert_eq!(
        modules[2]["content"]["groups"][0]["tracks"][1]["missing"],
        true
    );
    assert_eq!(
        modules[3]["content"]["cards"][0]["action"],
        json!({"kind":"radio","id":"33"})
    );
    assert_eq!(
        modules[4]["content"]["cards"][0]["action"],
        json!({"kind":"album","id":"22"})
    );
    assert_eq!(modules[6]["content"]["cards"][0]["title"], "歌单");
    let key = modules[0]["content"]["cards"][1]["action"]["key"]
        .as_str()
        .unwrap();
    assert_eq!(
        client
            .member_external_url("1", &page.value.revision, key)
            .unwrap(),
        "https://music.163.com/g/vip-portal?scene=pc"
    );
    assert!(
        client
            .member_external_url("1", &page.value.revision, "0:1:2")
            .is_err()
    );
    let count = api.calls.lock().unwrap().len();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let reopened = MusicClient::with_session(session, registry);
    assert_eq!(
        reopened
            .member_page("1", MemberQuery::Home, false)
            .await
            .unwrap()
            .value
            .revision,
        page.value.revision
    );
    assert_eq!(api.calls.lock().unwrap().len(), count);
}

#[tokio::test]
async fn rejected_or_malformed_refresh_preserves_last_good_content_and_links_are_revision_bound() {
    let (_dir, api, _session, client) = fixture();
    let album_reply =
        api.responses.lock().unwrap()[&format!("{ROOT}resource/plus/recomend/album")].clone();
    let good = client
        .member_page("1", MemberQuery::Home, true)
        .await
        .unwrap()
        .value;
    api.fault(
        &format!("{ROOT}viptab/module/get"),
        backend::Fault::Reject(403),
    );
    assert!(
        client
            .member_page("1", MemberQuery::Home, true)
            .await
            .is_err()
    );
    let cached = client
        .member_page("1", MemberQuery::Home, false)
        .await
        .unwrap()
        .value;
    assert_eq!(cached.revision, good.revision);
    assert!(cached.warning.is_some());
    api.responses.lock().unwrap().insert(
        format!("{ROOT}resource/plus/recomend/album"),
        json!({"code":200,"data":[{"albumId":false}]}),
    );
    assert!(
        client
            .member_page("1", MemberQuery::Home, true)
            .await
            .is_err()
    );
    assert_eq!(
        client
            .member_page("1", MemberQuery::Home, false)
            .await
            .unwrap()
            .value
            .revision,
        good.revision
    );
    api.responses
        .lock()
        .unwrap()
        .insert(format!("{ROOT}resource/plus/recomend/album"), album_reply);
    api.responses
        .lock()
        .unwrap()
        .get_mut(&format!("{ROOT}resource/common/list"))
        .unwrap()["data"]["contentList"][1]["jumpUrl"] = json!("https://music.163.com/g/different");
    let changed = client
        .member_page("1", MemberQuery::Home, true)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&changed.value.page).unwrap(),
        serde_json::to_value(&good.page).unwrap()
    );
    assert_ne!(changed.value.revision, good.revision);
    assert!(matches!(
        client.member_external_url("1", &good.revision, "0:1:1"),
        Err(Error::QueueChanged)
    ));
}

#[tokio::test]
async fn member_playback_uses_entire_native_list_and_official_trial_mode() {
    let (_dir, api, _session, client) = fixture();
    let view = client
        .member_page("1", MemberQuery::Home, true)
        .await
        .unwrap()
        .value;
    let selection = |revision: String| PlaySelection::Member {
        query: MemberQuery::Home,
        revision,
        section: Some("1:4".into()),
        index: 1,
        quality: Quality::Lossless,
    };
    let player = client
        .play_selection("1", selection(view.revision.clone()))
        .await
        .unwrap();
    assert_eq!(player.track.unwrap().id, "2");
    assert_eq!(player.queue_len, 3);
    assert!(!player.fm);
    let calls = api.calls.lock().unwrap().clone();
    let auth = calls
        .iter()
        .find(|(p, _)| p == "/api/song/enhance/player/url/v1")
        .unwrap();
    assert_eq!(auth.1["trialMode"], 41);
    assert_eq!(auth.1["level"], "lossless");
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|t| t.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
    assert!(matches!(
        client.play_selection("1", selection("wrong".into())).await,
        Err(Error::QueueChanged)
    ));
    let history = client
        .member_page("1", MemberQuery::History { date: None }, true)
        .await
        .unwrap()
        .value;
    client
        .play_selection(
            "1",
            PlaySelection::Member {
                query: MemberQuery::History {
                    date: Some("2026-10-26".into()),
                },
                revision: history.revision,
                section: None,
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 2);
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(p, _)| p == "/api/song/enhance/player/url/v1")
            .unwrap()
            .1["trialMode"],
        -1
    );
}

#[tokio::test]
async fn history_validates_offered_dates_and_keeps_cache_after_bad_response() {
    let (_dir, api, _session, client) = fixture();
    let query = MemberQuery::History { date: None };
    let first = client
        .member_page("1", query.clone(), true)
        .await
        .unwrap()
        .value;
    let count = api.count("/api/discovery/recommend/songs/history/detail");
    assert!(
        client
            .member_page(
                "1",
                MemberQuery::History {
                    date: Some("dates".into())
                },
                false
            )
            .await
            .is_err()
    );
    assert_eq!(
        api.count("/api/discovery/recommend/songs/history/detail"),
        count
    );
    api.responses.lock().unwrap().insert(
        "/api/discovery/recommend/songs/history/recent".into(),
        json!({"code":200,"data":{"dates":["2026-02-30"]}}),
    );
    assert!(client.member_page("1", query.clone(), true).await.is_err());
    assert_eq!(
        client
            .member_page("1", query.clone(), false)
            .await
            .unwrap()
            .value
            .revision,
        first.revision
    );
    api.responses.lock().unwrap().insert(
        "/api/discovery/recommend/songs/history/recent".into(),
        json!({"code":200,"data":{"dates":["2026-10-26","2026-10-25"]}}),
    );
    api.responses.lock().unwrap().insert(
        "/api/discovery/recommend/songs/history/detail".into(),
        json!({"code":200,"data":{"songs":[{"id":false}]}}),
    );
    assert!(client.member_page("1", query.clone(), true).await.is_err());
    assert_eq!(
        client
            .member_page("1", query, false)
            .await
            .unwrap()
            .value
            .revision,
        first.revision
    );
}

#[tokio::test]
async fn stale_epoch_and_inflight_account_change_cannot_read_or_save_another_account() {
    let (_dir, api, session, client) = fixture();
    assert!(matches!(
        client.member_page("0", MemberQuery::Home, true).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.member_rights("0", true).await,
        Err(Error::StaleResponse)
    ));
    assert!(api.calls.lock().unwrap().is_empty());
    *api.hold.lock().unwrap() = Some(format!("{ROOT}viptab/module/get"));
    let task = {
        let client = client.clone();
        tokio::spawn(async move { client.member_page("1", MemberQuery::Home, true).await })
    };
    api.started.notified().await;
    session.detach().unwrap();
    api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
}

#[tokio::test]
async fn rights_are_server_derived_and_do_not_expose_account_identity() {
    let (_dir, _api, _session, client) = fixture();
    let rights = client.member_rights("1", true).await.unwrap();
    assert_eq!(rights.value.entitlements.len(), 5);
    assert!(rights.value.entitlements.iter().all(|e| !e.active));
    let safe = serde_json::to_value(rights).unwrap();
    assert!(safe["value"].get("userId").is_none());
    let cached = client.member_rights("1", false).await.unwrap();
    assert!(cached.value.server_now > 0.0);
}
