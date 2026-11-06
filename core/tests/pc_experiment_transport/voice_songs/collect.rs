use super::{
    CARD, COOKIE, Error, Fixture, HttpResponse, PlayerAction, STARTUP, gate, json, picker, reply,
};
use cumulus_core::{
    client::dto::{PlaylistCollectSort, VoiceSongCollectPlan},
    playlist::PlaylistPhase,
};
use serde_json::Value;
use std::{
    future::{Future, poll_fn},
    task::Poll,
};

const LIST: &str = "/api/user/playlist/v2";
const DETAIL: &str = "/api/v6/playlist/detail";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";

fn card() -> HttpResponse {
    json!({"code":200,"tracks":[
        {"id":301,"name":"允许收藏但无播放权","album":{},"privilege":{"id":301,"subp":1,"st":-200,"pl":0}},
        {"id":302,"name":"明确为零","album":{},"privilege":{"id":302,"subp":0,"pl":320000}},
        {"id":303,"name":"无收藏字段","album":{},"privilege":{"id":303,"pl":320000}},
        {"id":304,"name":"明确拒绝","album":{},"privilege":{"id":304,"subp":-1,"pl":320000}},
        {"id":305,"name":"归属冲突","album":{},"privilege":{"id":999,"subp":1}},
        {"id":303,"name":"同一歌曲的另一行","album":{}},
        {"id":306,"name":"明确为空","album":{},"privilege":{"id":306,"subp":null}},
        {"id":307,"name":"云盘身份不翻译","album":{},"onlineTrackId":308,"privilege":{"id":307,"cs":true}}
    ]}).into()
}

fn list(id: u64) -> HttpResponse {
    json!({"code":200,"playlist":[
        {"id":id,"userId":25,"privacy":10,"name":"合成自建歌单"},
        {"id":82,"userId":76,"privacy":0,"name":"别人的歌单"},
        {"id":83,"userId":25,"privacy":10,"specialType":5,"name":"我喜欢的音乐"}
    ],"more":false})
    .into()
}

fn detail(owner: u64, ids: &[u64]) -> Value {
    json!({"code":200,"playlist":{"id":81,"userId":owner,"privacy":10,"name":"合成自建歌单",
        "tags":[],"trackIds":ids.iter().map(|id| json!({"id":id})).collect::<Vec<_>>()}})
}

async fn setup(
    cookies: &[&str],
    replies: impl IntoIterator<Item = HttpResponse>,
) -> (Fixture, String) {
    let f = Fixture::new(cookies, [card()].into_iter().chain(replies)).await;
    picker::config(&f);
    let receipt = f
        .client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap()
        .receipt
        .unwrap();
    (f, receipt)
}

fn prepare(f: &Fixture, card: &str, rows: &[usize]) -> VoiceSongCollectPlan {
    f.client
        .player_voice_songs_collect_prepare(&f.epoch, &f.play_id, card, rows)
        .unwrap()
}

async fn destinations(f: &Fixture, card: &str, selection: &str, expected: &str) {
    let reply = f
        .client
        .player_voice_songs_collect_destinations(
            &f.epoch,
            &f.play_id,
            card,
            selection,
            PlaylistCollectSort::Default,
            0,
        )
        .await
        .unwrap();
    assert_eq!(reply.epoch, f.epoch);
    assert!(reply.value.next_page.is_none());
    assert_eq!(
        reply
            .value
            .playlists
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        [expected]
    );
}

fn calls(f: &Fixture, paths: &[&str]) {
    assert_eq!(
        f.http
            .calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        STARTUP
            .into_iter()
            .chain([CARD])
            .chain(paths.iter().copied())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn exact_filtered_group_order_to_owned_destination_once() {
    let (f, card) = setup(
        &[COOKIE],
        [
            list(81),
            detail(25, &[701]).into(),
            json!({"code":200}).into(),
            detail(25, &[303, 301, 701]).into(),
        ],
    )
    .await;
    let plan = prepare(&f, &card, &[2, 1, 0, 4, 3, 5, 7]);
    assert_eq!((plan.selected_count, plan.eligible_count), (7, 2));
    assert_eq!(plan.omitted_indices, [1, 4, 3, 5, 7]);
    assert!(!plan.used_first_fallback);
    let selection = plan.selection_receipt.unwrap();
    calls(&f, &[]);
    for (receipt, id) in [(selection.as_str(), "81"), ("unknown-selection", "81")] {
        assert!(matches!(
            f.client
                .player_voice_songs_collect_existing(&f.epoch, &f.play_id, &card, receipt, id)
                .await,
            Err(Error::StaleResponse)
        ));
    }
    destinations(&f, &card, &selection, "81").await;
    for id in ["82", "83", "899"] {
        assert!(matches!(
            f.client
                .player_voice_songs_collect_existing(&f.epoch, &f.play_id, &card, &selection, id)
                .await,
            Err(Error::StaleResponse)
        ));
    }
    let confirmed = f
        .client
        .player_voice_songs_collect_existing(&f.epoch, &f.play_id, &card, &selection, "81")
        .await
        .unwrap();
    assert_eq!(confirmed.phase, PlaylistPhase::Confirmed);
    assert_eq!(confirmed.selection_receipt, selection);
    let again = f
        .client
        .player_voice_songs_collect_existing(&f.epoch, &f.play_id, &card, &selection, "81")
        .await
        .unwrap();
    assert_eq!(confirmed.operation_id, again.operation_id);
    let trace = f.http.calls();
    let query = &trace[STARTUP.len() + 1].payload;
    assert_eq!(query["uid"], "25");
    assert_eq!(query["limit"], "10");
    assert_eq!(query["offset"], "0");
    assert_eq!(query["sortType"], "default");
    let write = &trace[STARTUP.len() + 3].payload;
    assert_eq!(write["pid"], "81");
    assert_eq!(write["trackIds"], r#"["303","301"]"#);
    assert_eq!(write["op"], "add");
    assert!(write.get("imme").is_none());
    calls(&f, &[LIST, DETAIL, ADD, DETAIL]);
    assert_eq!(f.store.values("25", "playlist-mutations").unwrap().len(), 1);
}

#[tokio::test]
async fn official_first_fallback_is_not_first_eligible_or_playability() {
    let (f, card) = setup(&[COOKIE], []).await;
    for rows in [&[0][..], &[0, 0][..], &[0, 90][..]] {
        assert!(matches!(
            f.client
                .player_voice_songs_collect_prepare(&f.epoch, &f.play_id, &card, rows),
            Err(Error::InvalidInput(_))
        ));
    }
    for rows in [[1, 3], [6, 1]] {
        let plan = prepare(&f, &card, &rows);
        assert_eq!(plan.eligible_count, 1);
        assert_eq!(plan.omitted_indices, [rows[1]]);
        assert!(plan.used_first_fallback);
        assert!(plan.selection_receipt.is_some());
    }
    let none = prepare(&f, &card, &[3, 1, 6]);
    assert_eq!(none.eligible_count, 0);
    assert_eq!(none.omitted_indices, [3, 1, 6]);
    assert!(!none.used_first_fallback);
    assert!(none.selection_receipt.is_none());
    for rows in [[1, 4], [1, 7]] {
        let unsupported_preferred = prepare(&f, &card, &rows);
        assert_eq!(unsupported_preferred.eligible_count, 0);
        assert_eq!(unsupported_preferred.omitted_indices, rows);
        assert!(!unsupported_preferred.used_first_fallback);
        assert!(unsupported_preferred.selection_receipt.is_none());
    }
    calls(&f, &[]);
}

#[tokio::test]
async fn lost_reply_is_read_back_without_replay_or_retarget() {
    let (f, card) = setup(
        &[COOKIE],
        [
            list(81),
            detail(25, &[]).into(),
            HttpResponse::Ready(Err(Error::Transport("synthetic lost group add ack".into()))),
            detail(25, &[]).into(),
            detail(25, &[301, 303]).into(),
        ],
    )
    .await;
    let selection = prepare(&f, &card, &[0, 2]).selection_receipt.unwrap();
    destinations(&f, &card, &selection, "81").await;
    for _ in 0..2 {
        assert_eq!(
            f.client
                .player_voice_songs_collect_existing(&f.epoch, &f.play_id, &card, &selection, "81")
                .await
                .unwrap()
                .phase,
            PlaylistPhase::Uncertain
        );
    }
    assert!(matches!(
        f.client
            .player_voice_songs_collect_prepare(&f.epoch, &f.play_id, &card, &[2, 0]),
        Err(Error::MutationUncertain)
    ));
    let checked = f
        .client
        .player_voice_songs_collect_status(&f.epoch, &f.play_id, &card, &selection, true)
        .await
        .unwrap();
    assert_eq!(checked.phase, PlaylistPhase::Confirmed);
    calls(&f, &[LIST, DETAIL, ADD, DETAIL, DETAIL]);
}

#[tokio::test]
async fn replaced_destination_page_or_fresh_foreign_detail_cannot_authorize_add() {
    let (f, card) = setup(&[COOKIE], [list(84), list(81), detail(76, &[]).into()]).await;
    let selection = prepare(&f, &card, &[0, 2]).selection_receipt.unwrap();
    destinations(&f, &card, &selection, "84").await;
    destinations(&f, &card, &selection, "81").await;
    assert!(matches!(
        f.client
            .player_voice_songs_collect_existing(&f.epoch, &f.play_id, &card, &selection, "84")
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        f.client
            .player_voice_songs_collect_existing(&f.epoch, &f.play_id, &card, &selection, "81")
            .await,
        Err(Error::InvalidInput(_))
    ));
    calls(&f, &[LIST, LIST, DETAIL]);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_replaced_inflight_destination_page_cannot_restore_its_ids() {
    let (held, release) = gate();
    let (f, card) = setup(&[COOKIE], [list(81), held, list(85)]).await;
    let selection = prepare(&f, &card, &[0, 2]).selection_receipt.unwrap();
    destinations(&f, &card, &selection, "81").await;
    let read = || {
        f.client.player_voice_songs_collect_destinations(
            &f.epoch,
            &f.play_id,
            &card,
            &selection,
            PlaylistCollectSort::Default,
            0,
        )
    };
    let mut old = Box::pin(read());
    assert!(
        poll_fn(|cx| Poll::Ready(old.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.http.wait(STARTUP.len() + 3).await;
    let mut newest = Box::pin(read());
    assert!(
        poll_fn(|cx| Poll::Ready(newest.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(matches!(
        f.client
            .player_voice_songs_collect_existing(&f.epoch, &f.play_id, &card, &selection, "81")
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(
        release
            .send(Ok(reply(json!({"code":200,"playlist":[
        {"id":84,"userId":25,"privacy":10,"name":"已替换的旧响应"}
    ],"more":false}))))
            .is_ok()
    );
    assert!(matches!(old.await, Err(Error::StaleResponse)));
    let current = newest.await.unwrap();
    assert_eq!(current.value.playlists[0].id, "85");
    assert!(matches!(
        f.client
            .player_voice_songs_collect_existing(&f.epoch, &f.play_id, &card, &selection, "84")
            .await,
        Err(Error::StaleResponse)
    ));
    calls(&f, &[LIST, LIST, LIST]);
}

#[tokio::test]
async fn stale_episode_while_checking_destination_cannot_journal_or_send() {
    let (held, release) = gate();
    let (f, card) = setup(&[COOKIE], [list(81), held]).await;
    let selection = prepare(&f, &card, &[0, 2]).selection_receipt.unwrap();
    destinations(&f, &card, &selection, "81").await;
    let (client, epoch, play_id) = (f.client.clone(), f.epoch.clone(), f.play_id.clone());
    let pending = tokio::spawn(async move {
        client
            .player_voice_songs_collect_existing(&epoch, &play_id, &card, &selection, "81")
            .await
    });
    f.http.wait(STARTUP.len() + 3).await;
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(release.send(Ok(reply(detail(25, &[])))).is_ok());
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    calls(&f, &[LIST, DETAIL]);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn collection_needs_its_own_exact_scoped_cookie_and_not_download_authority() {
    let scoped = [
        "MUSIC_U=card; Path=/eapi/dj; Secure",
        "MUSIC_U=card; Path=/eapi/song/enhance/player; Secure",
        "MUSIC_U=card; Path=/eapi/v1/dj/program; Secure",
        "MUSIC_U=card; Path=/eapi/rtrs/abt/front; Secure",
        "MUSIC_U=card; Path=/eapi/w/nuser/account; Secure",
        "MUSIC_U=card; Path=/eapi/v6/playlist/detail; Secure",
        "MUSIC_U=card; Path=/eapi/v1/playlist/manipulate/tracks; Secure",
    ];
    for (cookie, expected) in [
        ("MUSIC_U=card; Path=/eapi/user/playlist/v2; Secure", true),
        ("MUSIC_U=other; Path=/eapi/user/playlist/v2; Secure", false),
    ] {
        let cookies: Vec<_> = scoped.into_iter().chain([cookie]).collect();
        let (f, card) = setup(&cookies, []).await;
        let selected =
            f.client
                .player_voice_songs_collect_prepare(&f.epoch, &f.play_id, &card, &[0, 2]);
        if expected {
            assert_eq!(selected.unwrap().eligible_count, 2);
            assert!(
                f.client
                    .player_voice_song_download_prepare(&f.epoch, &f.play_id, &card, 0)
                    .await
                    .is_err()
            );
        } else {
            assert!(matches!(selected, Err(Error::StaleResponse)));
        }
        calls(&f, &[]);
    }
}
