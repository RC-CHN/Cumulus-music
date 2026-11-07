use super::*;
use cumulus_core::{
    client::dto::{
        DownloadResult, PlaylistCollectSort, VoiceSongGroupDownloadConfirmation as Confirm,
    },
    playlist::PlaylistPhase,
};
use serde_json::Value;

const LIST: &str = "/api/user/playlist/v2";
const DETAIL: &str = "/api/v6/playlist/detail";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";
const CREATE: &str = "/api/playlist/create";
const OWNED: &str = "/api/user/playlist";
const NAME: &str = "合成下载目的地";
const ROWS: &[usize] = &[1, 6, 0, 2];

fn summary(id: u64, owner: u64) -> Value {
    json!({"id":id,"userId":owner,"privacy":10,"name":NAME})
}
fn owned_list(rows: Vec<Value>) -> HttpResponse {
    json!({"code":200,"playlist":rows,"more":false}).into()
}
fn listed() -> HttpResponse {
    let mut special = summary(83, 25);
    special["specialType"] = json!(5);
    owned_list(vec![summary(81, 25), summary(82, 76), special])
}
fn detail(id: u64, songs: &[u64]) -> HttpResponse {
    let mut row = summary(id, 25);
    row["tags"] = json!([]);
    row["trackIds"] = json!(songs.iter().map(|id| json!({"id":id})).collect::<Vec<_>>());
    json!({"code":200,"playlist":row}).into()
}
fn likes() -> HttpResponse {
    json!({"code":200,"ids":[301]}).into()
}
fn deny() -> HttpResponse {
    json!({"code":200,"data":{"code":403}}).into()
}

async fn prepare_picker(f: &Fixture, card: &str) -> String {
    f.client.liked_ids().await.unwrap();
    let plan = f
        .client
        .player_voice_songs_download_prepare_with_destination(&f.epoch, &f.play_id, card, 2, ROWS)
        .await
        .unwrap();
    assert_eq!(plan.decision, D::PlaylistRequired);
    assert_eq!(plan.eligible_indices, [1, 0]);
    assert_eq!(plan.omitted_indices, [6, 2]);
    plan.selection_receipt.unwrap()
}

async fn list(f: &Fixture, card: &str, selection: &str) -> cumulus_core::Result<()> {
    let page = f
        .client
        .player_voice_songs_download_destinations(
            &f.epoch,
            &f.play_id,
            card,
            selection,
            PlaylistCollectSort::Default,
            0,
        )
        .await?;
    assert_eq!(
        page.value
            .playlists
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["81"]
    );
    Ok(())
}

fn trace(f: &Fixture) -> Vec<String> {
    f.http.calls()[STARTUP.len()..]
        .iter()
        .map(|c| c.path.clone())
        .collect()
}

async fn ordered_tasks(f: &Fixture, result: &DownloadResult, expected: &[&str], quality: Quality) {
    let all = DownloadService::new(f.session.clone())
        .unwrap()
        .list()
        .unwrap();
    let new = all
        .iter()
        .filter(|task| result.accepted.contains(&task.id))
        .collect::<Vec<_>>();
    assert_eq!(
        new.iter()
            .map(|task| task.song_id.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(new.iter().all(|task| task.options.quality == quality));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if f.http
                .calls()
                .iter()
                .filter(|call| call.path == AUTH)
                .count()
                == expected.len()
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let calls = f.http.calls();
    let requested = calls
        .iter()
        .filter(|call| call.path == AUTH)
        .map(|call| {
            assert_eq!(call.payload["level"], quality.as_str());
            call.payload["id"].as_str().unwrap()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(requested, expected.iter().copied().collect());
}

#[tokio::test]
async fn existing_negative_callback_separates_native_collect_and_download_orders() {
    let (f, card) = fixture([
        likes(),
        listed(),
        detail(81, &[]),
        json!({"code":403}).into(),
        detail(81, &[]),
        deny(),
        deny(),
    ])
    .await;
    configure(&f).await;
    let before = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    let plan = prepare_picker(&f, &card).await;
    assert!(
        f.client
            .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &plan)
            .await
            .is_err()
    );
    let choose = |id| {
        f.client
            .player_voice_songs_download_choose_playlist(&f.epoch, &f.play_id, &card, &plan, id)
    };
    assert!(matches!(choose("81").await, Err(Error::StaleResponse)));
    list(&f, &card, &plan).await.unwrap();
    assert!(matches!(choose("82").await, Err(Error::StaleResponse)));
    assert!(matches!(choose("83").await, Err(Error::StaleResponse)));
    let picked = choose("81").await.unwrap();
    assert_eq!(picked.phase, PlaylistPhase::Rejected);
    assert!(picked.add_failed);
    let permit = picked.continuation.unwrap();
    assert_eq!(
        choose("81").await.unwrap().operation_id,
        picked.operation_id
    );
    assert!(f.store.values("25", TASKS).unwrap().is_empty());
    let default = f
        .client
        .download_quality_preferences(&f.epoch)
        .unwrap()
        .options
        .quality;
    let start = |permit| {
        f.client.player_voice_songs_download_start_from_playlist(
            &f.epoch, &f.play_id, &card, &plan, permit,
        )
    };
    assert!(matches!(
        start(&picked.operation_id).await,
        Err(Error::StaleResponse)
    ));
    let result = start(&permit).await.unwrap();
    assert_eq!(start(&permit).await.unwrap().accepted, result.accepted);
    ordered_tasks(&f, &result, &["301", "302"], default).await;
    let paths = trace(&f);
    assert_eq!(paths, [CARD, LIKED, LIST, DETAIL, ADD, DETAIL, AUTH, AUTH]);
    let calls = f.http.calls();
    let write = calls.iter().find(|c| c.path == ADD).unwrap();
    assert_eq!(write.payload["trackIds"], r#"["307","301"]"#);
    assert_eq!(write.payload["pid"], "81");
    assert_eq!(
        before,
        serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap()
    );
}

#[tokio::test]
async fn quality_precedes_picker_and_ambiguous_add_only_reads_back() {
    let (f, card) = fixture([
        likes(),
        listed(),
        detail(81, &[]),
        HttpResponse::Ready(Err(Error::Transport("synthetic lost add".into()))),
        detail(81, &[]),
        detail(81, &[301, 310, 307]),
        deny(),
        deny(),
        deny(),
    ])
    .await;
    configure(&f).await;
    f.client.liked_ids().await.unwrap();
    let plan = f
        .client
        .player_voice_songs_download_prepare_with_destination(
            &f.epoch,
            &f.play_id,
            &card,
            2,
            &[1, 6, 10, 0, 2],
        )
        .await
        .unwrap();
    assert_eq!(plan.decision, D::PurchasedQualityRequired);
    assert!(plan.purchased_qualities.contains(&Quality::HiRes));
    let selection = plan.selection_receipt.unwrap();
    assert!(matches!(
        list(&f, &card, &selection).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(trace(&f), [CARD, LIKED]);
    let confirm = |tier| {
        f.client.player_voice_songs_download_confirm_quality(
            &f.epoch, &f.play_id, &card, &selection, tier,
        )
    };
    assert!(matches!(
        confirm(Quality::HiRes).await.unwrap(),
        Confirm::PlaylistRequired
    ));
    assert!(matches!(
        confirm(Quality::Standard).await,
        Err(Error::InvalidInput(_))
    ));
    list(&f, &card, &selection).await.unwrap();
    let choose = || {
        f.client.player_voice_songs_download_choose_playlist(
            &f.epoch, &f.play_id, &card, &selection, "81",
        )
    };
    let first = choose().await.unwrap();
    assert_eq!(first.phase, PlaylistPhase::Uncertain);
    assert!(first.continuation.is_none());
    assert!(choose().await.unwrap().continuation.is_none());
    assert!(f.store.values("25", TASKS).unwrap().is_empty());
    let recovered = f
        .client
        .player_voice_songs_download_playlist_status(&f.epoch, &f.play_id, &card, &selection, true)
        .await
        .unwrap();
    assert_eq!(recovered.operation_id, first.operation_id);
    assert_eq!(recovered.phase, PlaylistPhase::Confirmed);
    let result = f
        .client
        .player_voice_songs_download_start_from_playlist(
            &f.epoch,
            &f.play_id,
            &card,
            &selection,
            &recovered.continuation.unwrap(),
        )
        .await
        .unwrap();
    ordered_tasks(&f, &result, &["301", "310", "302"], Quality::HiRes).await;
    assert_eq!(
        trace(&f),
        [
            CARD, LIKED, LIST, DETAIL, ADD, DETAIL, DETAIL, AUTH, AUTH, AUTH
        ]
    );
    assert_ne!(
        f.client
            .download_quality_preferences(&f.epoch)
            .unwrap()
            .options
            .quality,
        Quality::HiRes
    );
}

#[tokio::test]
async fn newly_created_target_needs_a_successful_add_before_any_download() {
    for success in [false, true] {
        let mut responses = vec![
            likes(),
            owned_list(vec![]),
            json!({"code":200,"id":91}).into(),
            owned_list(vec![summary(91, 25)]),
            detail(91, &[]),
            json!({"code":if success {200} else {403}}).into(),
            detail(91, if success { &[307, 301] } else { &[] }),
        ];
        if success {
            responses.extend([deny(), deny()]);
        }
        let (f, card) = fixture(responses).await;
        configure(&f).await;
        let plan = prepare_picker(&f, &card).await;
        let create = || {
            f.client.player_voice_songs_download_create_playlist(
                &f.epoch, &f.play_id, &card, &plan, NAME, true,
            )
        };
        let picked = create().await.unwrap();
        assert_eq!(picked.create_phase, PlaylistPhase::Confirmed);
        assert_eq!(picked.playlist_id.as_deref(), Some("91"));
        assert!(!picked.can_add);
        assert_eq!(
            create().await.unwrap().create_operation_id,
            picked.create_operation_id
        );
        let add = picked.add.unwrap();
        assert_eq!(add.add_failed, !success);
        assert_eq!(add.continuation.is_some(), success);
        assert!(f.store.values("25", TASKS).unwrap().is_empty());
        let calls = f.http.calls();
        assert_eq!(
            calls
                .iter()
                .find(|call| call.path == CREATE)
                .unwrap()
                .payload["name"],
            NAME
        );
        let write = calls.iter().find(|call| call.path == ADD).unwrap();
        assert_eq!(write.payload["trackIds"], r#"["307","301"]"#);
        let mut expected = vec![CARD, LIKED, OWNED, CREATE, OWNED, DETAIL, ADD, DETAIL];
        if let Some(permit) = add.continuation {
            let result = f
                .client
                .player_voice_songs_download_start_from_playlist(
                    &f.epoch, &f.play_id, &card, &plan, &permit,
                )
                .await
                .unwrap();
            let default = f
                .client
                .download_quality_preferences(&f.epoch)
                .unwrap()
                .options
                .quality;
            ordered_tasks(&f, &result, &["301", "302"], default).await;
            expected.extend([AUTH, AUTH]);
        }
        assert_eq!(trace(&f), expected);
    }
}

#[tokio::test]
async fn retired_episode_cannot_add_after_the_observed_destination_detail_returns() {
    let (held, release) = super::super::gate();
    let (f, card) = fixture([likes(), listed(), held]).await;
    configure(&f).await;
    let plan = prepare_picker(&f, &card).await;
    list(&f, &card, &plan).await.unwrap();
    let choose = f
        .client
        .player_voice_songs_download_choose_playlist(&f.epoch, &f.play_id, &card, &plan, "81");
    tokio::pin!(choose);
    tokio::select! {
        result = &mut choose => panic!("unexpected picker completion: {}", result.err().unwrap()),
        () = f.http.wait(STARTUP.len() + 4) => {}
    }
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    let mut row = summary(81, 25);
    row["tags"] = json!([]);
    row["trackIds"] = json!([]);
    assert!(
        release
            .send(Ok(super::super::reply(json!({"code":200,"playlist":row}))))
            .is_ok()
    );
    assert!(matches!(choose.await, Err(Error::StaleResponse)));
    assert_eq!(trace(&f), [CARD, LIKED, LIST, DETAIL]);
    assert!(f.store.values("25", TASKS).unwrap().is_empty());
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_lost_create_never_replays_or_promotes_an_inferred_new_target_to_write_authority() {
    let (f, card) = fixture([
        likes(),
        owned_list(vec![]),
        HttpResponse::Ready(Err(Error::Transport("synthetic lost create".into()))),
        owned_list(vec![]),
        owned_list(vec![summary(91, 25)]),
    ])
    .await;
    configure(&f).await;
    let plan = prepare_picker(&f, &card).await;
    let create = || {
        f.client.player_voice_songs_download_create_playlist(
            &f.epoch, &f.play_id, &card, &plan, NAME, true,
        )
    };
    let first = create().await.unwrap();
    assert_eq!(first.create_phase, PlaylistPhase::Uncertain);
    assert!(!first.can_add && first.add.is_none());
    assert_eq!(
        create().await.unwrap().create_operation_id,
        first.create_operation_id
    );
    let checked = f
        .client
        .player_voice_songs_download_creation_status(&f.epoch, &f.play_id, &card, &plan, true)
        .await
        .unwrap();
    assert_eq!(checked.create_phase, PlaylistPhase::Confirmed);
    assert!(!checked.can_add && checked.add.is_none());
    assert!(matches!(
        f.client
            .player_voice_songs_download_add_created_playlist(&f.epoch, &f.play_id, &card, &plan)
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(trace(&f), [CARD, LIKED, OWNED, CREATE, OWNED, OWNED]);
    assert!(f.store.values("25", TASKS).unwrap().is_empty());
}
