use super::*;
use cumulus_core::{account::AccountSession, playlist::PlaylistService, storage::StateStore};
use std::sync::Arc;

const STAR_LIST: &str = "/api/user/playlist";
const LIKED: &str = "/api/song/like/get";

fn star(id: u64, owner: u64, special: u64) -> Value {
    json!({"id":id,"userId":owner,"creator":{"userId":owner},"privacy":10,
        "specialType":special,"name":"合成系统喜欢"})
}

fn list(rows: Vec<Value>) -> HttpResponse {
    json!({"code":200,"playlist":rows,"more":false}).into()
}

fn observation(rows: &[u64], likes: &[u64]) -> [HttpResponse; 3] {
    let mut detail = star(83, 25, 5);
    detail["tags"] = json!([]);
    detail["trackIds"] = json!(rows.iter().map(|id| json!({"id":id})).collect::<Vec<_>>());
    [
        list(vec![star(81, 25, 0), star(83, 25, 5)]),
        json!({"code":200,"playlist":detail}).into(),
        json!({"code":200,"ids":likes}).into(),
    ]
}

fn selection(f: &Fixture, card: &str) -> String {
    let plan = prepare(f, card, &[2, 1, 0, 5]);
    assert_eq!(plan.eligible_count, 2);
    assert_eq!(plan.omitted_indices, [1, 5]);
    plan.selection_receipt.unwrap()
}

fn exact_wire(f: &Fixture, paths: &[&str]) {
    calls(f, paths);
    for call in f.http.calls().into_iter().skip(STARTUP.len() + 1) {
        assert!(
            call.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-voice")
        );
        let mut body = call.payload;
        assert!(
            body.as_object_mut()
                .unwrap()
                .remove("header")
                .unwrap()
                .is_string()
        );
        let expected = match call.path.as_str() {
            STAR_LIST => json!({"uid":"25","offset":"0","limit":"1000"}),
            DETAIL => json!({"id":"83","n":"100000","s":"0"}),
            ADD => json!({"pid":"83","trackIds":"[\"303\",\"301\"]","op":"add"}),
            LIKED => json!({"uid":"25"}),
            _ => panic!("unexpected system-liked wire"),
        };
        assert_eq!(body, expected);
    }
}

#[tokio::test]
async fn system_liked_adds_once_in_selected_order_and_publishes_observed_likes() {
    let (f, card) = setup(
        &[COOKIE],
        observation(&[701], &[701])
            .into_iter()
            .chain([json!({"code":200}).into()])
            .chain(observation(&[303, 301, 701], &[701, 301, 303])),
    )
    .await;
    let selection = selection(&f, &card);
    for _ in 0..2 {
        let done = f
            .client
            .player_voice_songs_collect_liked(&f.epoch, &f.play_id, &card, &selection)
            .await
            .unwrap();
        assert_eq!(done.playlist_id, "83");
        assert_eq!(done.phase, PlaylistPhase::Confirmed);
    }
    exact_wire(
        &f,
        &[STAR_LIST, DETAIL, LIKED, ADD, STAR_LIST, DETAIL, LIKED],
    );
    assert_eq!(
        f.store.get("25", "resources", "liked").unwrap(),
        Some(json!(["701", "301", "303"]))
    );
    let records = PlaylistService::new(f.session.clone())
        .unwrap()
        .operations(&f.session.token().unwrap())
        .unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].system_liked_group);
}

#[tokio::test]
async fn system_liked_journal_restarts_read_only_and_requires_liked_ids_after_ack_loss() {
    let (f, card) = setup(
        &[COOKIE],
        observation(&[701], &[701])
            .into_iter()
            .chain([HttpResponse::Ready(Err(Error::Transport(
                "synthetic lost batch reply".into(),
            )))])
            // The playlist alone already reports both IDs, but the liked list has not converged.
            .chain(observation(&[303, 301, 701], &[701, 303]))
            .chain([list(vec![star(84, 25, 5)])])
            .chain(observation(&[303, 301, 701], &[701, 301, 303])),
    )
    .await;
    let selection = selection(&f, &card);
    let first = f
        .client
        .player_voice_songs_collect_liked(&f.epoch, &f.play_id, &card, &selection)
        .await
        .unwrap();
    assert_eq!(first.phase, PlaylistPhase::Uncertain);
    let repeat = f
        .client
        .player_voice_songs_collect_liked(&f.epoch, &f.play_id, &card, &selection)
        .await
        .unwrap();
    assert_eq!(first.operation_id, repeat.operation_id);
    let status = f
        .client
        .player_voice_songs_collect_status(&f.epoch, &f.play_id, &card, &selection, false)
        .await
        .unwrap();
    assert_eq!(status.phase, PlaylistPhase::Uncertain);
    assert_eq!(status.operation_id, first.operation_id);
    assert!(f.store.get("25", "resources", "liked").unwrap().is_none());
    let fresh_store = Arc::new(StateStore::open(f.store.directory()).unwrap());
    let fresh_session = Arc::new(AccountSession::new(
        f.native.clone(),
        Some(fresh_store.clone()),
    ));
    fresh_session
        .adopt(
            f.native.clone(),
            json!({"userId":25}),
            &fresh_session.token().unwrap(),
        )
        .unwrap();
    let journal = PlaylistService::new(fresh_session.clone()).unwrap();
    let token = fresh_session.token().unwrap();
    assert!(journal.operations(&token).unwrap()[0].system_liked_group);
    // A changed star cannot confirm the old intent against a different playlist.
    assert!(
        journal
            .reconcile(&token, &first.operation_id)
            .await
            .is_err()
    );
    assert_eq!(
        journal.operations(&token).unwrap()[0].phase,
        PlaylistPhase::Uncertain
    );
    let after = journal
        .reconcile(&token, &first.operation_id)
        .await
        .unwrap();
    assert_eq!(after.phase, PlaylistPhase::Confirmed);
    assert_eq!(
        fresh_store.get("25", "resources", "liked").unwrap(),
        Some(json!(["701", "301", "303"]))
    );
    exact_wire(
        &f,
        &[
            STAR_LIST, DETAIL, LIKED, ADD, STAR_LIST, DETAIL, LIKED, STAR_LIST, STAR_LIST, DETAIL,
            LIKED,
        ],
    );
}

#[tokio::test]
async fn system_liked_refuses_regular_foreign_ambiguous_or_inconsistent_star() {
    let mismatches = [
        vec![list(vec![star(83, 25, 0)])],
        vec![list(vec![star(83, 76, 5), star(84, 25, 5)])],
        vec![list(vec![star(83, 25, 5), star(84, 25, 5)])],
        vec![
            list(vec![star(83, 25, 5)]),
            json!({"code":200,"playlist":star(84, 25, 5)}).into(),
        ],
        vec![
            list(vec![star(83, 25, 5)]),
            json!({"code":200,"playlist":star(83, 25, 0)}).into(),
        ],
        vec![
            list(vec![star(83, 25, 5)]),
            json!({"code":200,"playlist":{
            "id":83,"specialType":5,"userId":25,"creator":{"userId":76}}})
            .into(),
        ],
        observation(&[303], &[]).into(),
    ];
    for responses in mismatches {
        let n = responses.len();
        let (f, card) = setup(&[COOKIE], responses).await;
        let selection = selection(&f, &card);
        assert!(
            f.client
                .player_voice_songs_collect_liked(&f.epoch, &f.play_id, &card, &selection)
                .await
                .is_err()
        );
        calls(&f, &[STAR_LIST, DETAIL, LIKED][..n]);
        assert!(
            f.store
                .values("25", "playlist-mutations")
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn system_liked_scope_rejects_foreign_cookie_before_membership_or_write() {
    let (f, card) = setup(
        &[COOKIE, "MUSIC_U=foreign; Path=/eapi/song/like/; Secure"],
        [],
    )
    .await;
    let selection = selection(&f, &card);
    assert!(matches!(
        f.client
            .player_voice_songs_collect_liked(&f.epoch, &f.play_id, &card, &selection)
            .await,
        Err(Error::StaleResponse)
    ));
    calls(&f, &[]);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn system_liked_does_not_send_after_current_episode_leaves_during_liked_read() {
    let [list, detail, _] = observation(&[], &[]);
    let (held, release) = gate();
    let (f, card) = setup(&[COOKIE], [list, detail, held]).await;
    let selection = selection(&f, &card);
    let (client, epoch, play_id) = (f.client.clone(), f.epoch.clone(), f.play_id.clone());
    let pending = tokio::spawn(async move {
        client
            .player_voice_songs_collect_liked(&epoch, &play_id, &card, &selection)
            .await
    });
    f.http.wait(STARTUP.len() + 4).await;
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(
        release
            .send(Ok(reply(json!({"code":200,"ids":[]}))))
            .is_ok()
    );
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    calls(&f, &[STAR_LIST, DETAIL, LIKED]);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
}
