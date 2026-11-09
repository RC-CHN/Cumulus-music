#[path = "support/discovery_home.rs"]
mod support;
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};
use support::{BATCH, BOOKS, CUSTOM, Fixture, GENERAL, MIX, RECENT, SETTING, VOICES};

fn fixture() -> (
    tempfile::TempDir,
    Arc<Fixture>,
    Arc<AccountSession>,
    MusicClient,
) {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Fixture::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":42}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry);
    (root, api, session, client)
}

#[tokio::test]
async fn official_batch_order_parameters_and_bounded_public_sections() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let view = serde_json::to_value(client.discovery_home(&epoch).await.unwrap()).unwrap();
    assert_eq!(view["epoch"], epoch);
    let view = &view["value"];
    assert_eq!(view["recommendationEnabled"], true);
    let sections = view["sections"].as_array().unwrap();
    let keys = sections
        .iter()
        .map(|row| row["kind"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "playlists",
            "featured",
            "all_listen",
            "recent",
            "heartbeat",
            "radar",
            "vip",
            "style",
            "daily_voice",
            "personal_voices",
            "books"
        ]
    );
    assert_eq!(sections[1]["cards"][0]["target"], json!({"kind":"daily"}));
    assert_eq!(
        sections[1]["cards"][4]["target"],
        json!({"kind":"podcast","id":"744"})
    );
    assert_eq!(sections[2]["cards"].as_array().unwrap().len(), 2);
    assert_eq!(
        sections[3]["cards"][0]["target"],
        json!({"kind":"album","id":"851"})
    );
    let recent = sections[3]["cards"].as_array().unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[1]["title"], "合成最近风格");
    assert_eq!(recent[1]["target"]["kind"], "recent_style");
    assert!(
        recent[1]["target"]["id"]
            .as_str()
            .is_some_and(|id| id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
    );
    assert_eq!(sections[9]["cards"].as_array().unwrap().len(), 3);
    assert!(!view.to_string().contains("PRIVATE"));
    assert!(!view.to_string().contains("错配歌曲"));

    let calls = api.calls.lock().unwrap();
    let paths = calls
        .iter()
        .map(|(path, _, _)| path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths[0], SETTING);
    assert!(
        paths.iter().position(|path| *path == BATCH).unwrap()
            < paths.iter().position(|path| *path == MIX).unwrap()
    );
    assert!(paths.contains(&RECENT));
    assert_eq!(paths.len(), 4);
    assert!(calls.iter().all(|(_, _, sensitive)| !sensitive));
    let body = &calls.iter().find(|(path, _, _)| path == BATCH).unwrap().1;
    let child = |path: &str| serde_json::from_str::<Value>(body[path].as_str().unwrap()).unwrap();
    assert_eq!(body.as_object().unwrap().len(), 6);
    assert_eq!(child("/api/personalized/taste"), json!({}));
    assert_eq!(child("/api/playlist/tags"), json!({}));
    assert_eq!(child(CUSTOM), json!({"newStyle":true}));
    assert_eq!(child(VOICES), json!({"limit":9}));
    assert_eq!(
        child(BOOKS),
        json!({"pageCode":"PC_HOMEPAGE_PODCAST","blockCode":"PC_HOMEPAGE_VOICEBOOK_RCMD","extInfo":r#"{"position":"homePage"}"#})
    );
    let general = child(GENERAL);
    assert_eq!(general["newStyle"], true);
    assert_eq!(
        general["extInfo"],
        r#"{"abInfo":{"hp-new-homepageV3.1":"t3"}}"#
    );
    assert_eq!(
        serde_json::from_str::<Value>(general["cursor"].as_str().unwrap()).unwrap(),
        json!({
            "offset":0,"blockCodeOrderList":["HOMPAGE_BLOCK_VIP_RCMD","HOMEPAGE_BLOCK_RED_SIMILAR_SONG",
                "HOMEPAGE_BLOCK_STYLE_RCMD","HOMEPAGE_MUSIC_PODCAST_RCMD_BLOCK","HOMEPAGE_BLOCK_PLAYLIST_RCMD","HOMEPAGE_BLOCK_ALL_LISTEN"]
        })
    );
    let mix = &calls.iter().find(|(path, _, _)| path == MIX).unwrap().1;
    assert!(
        chrono::NaiveDateTime::parse_from_str(
            mix["clientTime"].as_str().unwrap(),
            "%Y-%m-%d %H:%M:%S"
        )
        .is_ok()
    );
}

#[tokio::test]
async fn home_style_dispatch_keeps_native_seed_and_dialog_revisions_independent() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let home = serde_json::to_value(client.discovery_home(&epoch).await.unwrap().value).unwrap();
    let cards = home["sections"][1]["cards"].as_array().unwrap();
    let style = cards
        .iter()
        .filter(|card| card["target"]["kind"] == "style")
        .collect::<Vec<_>>();
    assert_eq!(
        style
            .iter()
            .map(|card| card["title"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["合成首页风格", "合成降级风格"]
    );
    assert!(!home.to_string().contains("PRIVATE_SEED"));
    assert!(!home.to_string().contains("songId"));
    let seed =
        |index: usize| serde_json::from_value(style[index]["target"]["seed"].clone()).unwrap();

    // The direct daily-style drawer reads the same endpoint separately. It
    // must not invalidate cached discovery cards.
    let dialog =
        serde_json::to_value(client.style_home_seeds(&epoch).await.unwrap().value).unwrap();
    let dialog_seed = || {
        serde_json::from_value(json!({
            "revision":dialog["revision"], "id":dialog["cards"][0]["id"]
        }))
        .unwrap()
    };
    for (position, id) in [(0, "81"), (1, "83")] {
        let value = serde_json::to_value(
            client
                .seeded_style_recommendations(&epoch, seed(position))
                .await
                .unwrap()
                .value,
        )
        .unwrap();
        assert_eq!(value["tracks"][0]["id"], id);
    }
    api.mode.store(1, Ordering::SeqCst);
    assert!(client.discovery_home(&epoch).await.is_err());
    assert!(
        client
            .seeded_style_recommendations(&epoch, seed(0))
            .await
            .is_ok()
    );
    api.mode.store(2, Ordering::SeqCst);
    client.discovery_home(&epoch).await.unwrap();
    assert!(matches!(
        client.seeded_style_recommendations(&epoch, seed(0)).await,
        Err(cumulus_core::Error::QueueChanged)
    ));
    assert!(
        client
            .seeded_style_recommendations(&epoch, dialog_seed())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn privacy_short_circuits_and_secondary_failure_does_not_break_the_feed() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    assert!(client.discovery_home("another-epoch").await.is_err());
    assert!(api.calls.lock().unwrap().is_empty());
    api.mode.store(2, Ordering::SeqCst);
    let view = serde_json::to_value(client.discovery_home(&epoch).await.unwrap().value).unwrap();
    assert_eq!(view, json!({"recommendationEnabled":false,"sections":[]}));
    assert_eq!(
        api.calls.lock().unwrap().as_slice(),
        &[(SETTING.into(), json!({}), false)]
    );

    api.mode.store(1, Ordering::SeqCst);
    assert!(client.discovery_home(&epoch).await.is_err());

    api.mode.store(3, Ordering::SeqCst);
    let view = serde_json::to_value(client.discovery_home(&epoch).await.unwrap().value).unwrap();
    let sections = view["sections"].as_array().unwrap();
    assert_eq!(sections[0]["cards"][0]["target"], json!({"kind":"daily"}));
    assert!(
        !sections
            .iter()
            .any(|row| matches!(row["kind"].as_str(), Some("featured" | "recent")))
    );
}

#[tokio::test]
async fn replacing_the_account_drops_a_feed_that_was_already_in_flight() {
    let (_root, api, session, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    api.hold_batch.store(true, Ordering::SeqCst);
    let replace = async {
        api.batch_started.notified().await;
        session
            .adopt(api.clone(), json!({"userId":43}), &session.token().unwrap())
            .unwrap();
        api.release_batch.notify_one();
    };
    let (result, ()) = tokio::join!(client.discovery_home(&epoch), replace);
    assert!(matches!(result, Err(cumulus_core::Error::StaleResponse)));
    assert_ne!(client.account().unwrap().epoch, epoch);
}

#[tokio::test]
async fn rankings_honor_both_official_groups_and_reuse_the_independent_rank_cache() {
    use support::{AB, RANK_ALBUM, RANK_MAIN, RANK_SONG};
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;

    // An explicit tag exclusion overrides rank t1. Ambiguous assignment is hidden.
    for mode in [4, 6] {
        api.mode.store(mode, Ordering::SeqCst);
        assert!(
            client
                .discovery_home_rankings(&epoch)
                .await
                .unwrap()
                .value
                .is_none()
        );
    }
    {
        let calls = api.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|(path, args, sensitive)| path == AB
            && *args == json!({"expNames":"PH-PC-tagT,PH-PC-Home-Ranklist"})
            && !sensitive));
    }

    // Both absent rows use the two values in the official defaults.
    api.mode.store(5, Ordering::SeqCst);
    let value =
        serde_json::to_value(client.discovery_home_rankings(&epoch).await.unwrap().value).unwrap();
    assert_eq!(value["name"], "官方榜");
    assert_eq!(value["displayType"], "TOP_3");
    assert_eq!(value["cards"][0]["tracks"].as_array().unwrap().len(), 3);
    assert_eq!(value["cards"][0]["tracks"][0]["name"], "合成榜首");
    assert_eq!(value["cards"][1]["target"], "album_sales");
    assert_eq!(value["cards"][2]["target"], "song_sales");
    assert!(!value.to_string().contains("PRIVATE"));
    assert!(!value.to_string().contains("全球榜"));

    // Explicit allowed assignments are accepted too, but the rank data is fresh.
    api.mode.store(0, Ordering::SeqCst);
    assert!(
        client
            .discovery_home_rankings(&epoch)
            .await
            .unwrap()
            .value
            .is_some()
    );
    let calls = api.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _, _)| path == RANK_MAIN)
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _, _)| path == RANK_SONG)
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(
                |(path, args, _)| path == RANK_ALBUM && *args == json!({"limit":3,"albumType":0})
            )
            .count(),
        1
    );
}

#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;

const TEST_CLOCK: u64 = 1_780_000_000_000;
const TEST_MEDIA: &str = "/api/song/enhance/player/url/v1";
fn make_recall_player(client: MusicClient) -> (MusicClient, Arc<engine::Device>) {
    let device = Arc::new(engine::Device::default());
    let engine = device.clone();
    (
        client.with_audio_factory(move || Ok(engine::audio_engine(engine.clone()))),
        device,
    )
}
fn recall_rows(queue: &cumulus_core::client::dto::QueueView) -> Vec<String> {
    queue
        .tracks
        .iter()
        .map(|row| row.track.id.clone())
        .collect()
}

#[tokio::test(start_paused = true)]
async fn recall_caps_hot_rows_persists_cooldown_and_only_explicit_play_opens_media() {
    use cumulus_core::{client::dto::PlayerAction, playback::controller::PlaybackStatus};
    let (_root, api, _, client) = fixture();
    let (client, device) = make_recall_player(client);
    let epoch = client.account().unwrap().epoch;
    api.recall.store(1, Ordering::SeqCst);
    api.now.store(TEST_CLOCK, Ordering::SeqCst);
    client.refresh_pc_presentation().await.unwrap();
    let page = serde_json::to_value(client.discovery_home(&epoch).await.unwrap().value).unwrap();
    let guide = page["recallGuide"]["receipt"].as_str().unwrap();
    let queue = client.player_queue(&epoch).await.unwrap();
    assert_eq!(
        recall_rows(&queue),
        (911..923).map(|id| id.to_string()).collect::<Vec<_>>()
    );
    assert_eq!(queue.current.as_ref(), Some(&queue.tracks[0].entry_id));
    assert_eq!(queue.revision, page["recallGuide"]["queueRevision"]);
    assert!(!serde_json::to_string(&queue).unwrap().contains("PRIVATE"));
    let ready = client
        .player_action(&epoch, PlayerAction::Startup)
        .await
        .unwrap();
    assert_eq!(ready.state, PlaybackStatus::Idle);
    assert_eq!(ready.track.unwrap().id, "911");
    assert!(ready.play_id.is_none() && ready.position.is_none());
    assert_eq!(device.state.lock().unwrap().loads, 0);
    let batch = api.calls.lock().unwrap();
    let args = &batch.iter().find(|(path, _, _)| path == BATCH).unwrap().1;
    let body: Value = serde_json::from_str(args[GENERAL].as_str().unwrap()).unwrap();
    let cursor: Value = serde_json::from_str(body["cursor"].as_str().unwrap()).unwrap();
    assert_eq!(
        cursor["blockCodeOrderList"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        "HOMEPAGE_BLOCK_HOT_RESOURCE"
    );
    assert_eq!(cursor["blockCodeOrderList"].as_array().unwrap().len(), 7);
    assert!(!batch.iter().any(|(path, _, _)| path == TEST_MEDIA));
    drop(batch);

    assert!(client.acknowledge_home_recall_guide(&epoch, guide).is_err());
    tokio::time::advance(std::time::Duration::from_secs(3)).await;
    client.acknowledge_home_recall_guide(&epoch, guide).unwrap();
    assert!(client.acknowledge_home_recall_guide(&epoch, guide).is_err());
    let playing = client
        .player_action(&epoch, PlayerAction::Play)
        .await
        .unwrap();
    assert_eq!(playing.state, PlaybackStatus::Playing);
    assert_eq!(playing.entry_id, queue.current);
    assert_eq!(playing.track.unwrap().id, "911");
    assert!(playing.play_id.is_some());
    assert_eq!(device.state.lock().unwrap().loads, 1);
}

#[tokio::test]
async fn recall_uses_all_listen_fallback_but_never_enqueues_in_control_or_opt_out() {
    for (recall, opt_out, wanted) in [
        (2, false, true),
        (3, false, false),
        (4, false, true),
        (5, false, false), // A present malformed HOT list cannot grant ALL_LISTEN instead.
        (1, true, false),
    ] {
        let (_root, api, _, client) = fixture();
        let (client, device) = make_recall_player(client);
        let epoch = client.account().unwrap().epoch;
        api.recall.store(recall, Ordering::SeqCst);
        api.mode
            .store(if opt_out { 2 } else { 0 }, Ordering::SeqCst);
        client.refresh_pc_presentation().await.unwrap();
        let home =
            serde_json::to_value(client.discovery_home(&epoch).await.unwrap().value).unwrap();
        assert_eq!(!home["recallGuide"].is_null(), wanted);
        let queue = client.player_queue(&epoch).await.unwrap();
        if wanted {
            assert_eq!(recall_rows(&queue), ["811", "812"]);
        } else {
            assert!(queue.tracks.is_empty());
        }
        let calls = api.calls.lock().unwrap();
        assert_eq!(calls.iter().any(|(path, _, _)| path == BATCH), !opt_out);
        assert!(!calls.iter().any(|(path, _, _)| path == TEST_MEDIA));
        assert_eq!(device.state.lock().unwrap().loads, 0);
    }
}

#[tokio::test]
async fn held_home_does_not_replace_a_newly_playing_queue() {
    use cumulus_core::{
        client::dto::PlaySelection, membership::Quality, playback::controller::PlaybackStatus,
    };
    let (_root, api, _, client) = fixture();
    let (client, device) = make_recall_player(client);
    let client = Arc::new(client);
    let epoch = client.account().unwrap().epoch;
    api.recall.store(1, Ordering::SeqCst);
    client.refresh_pc_presentation().await.unwrap();
    api.hold_batch.store(true, Ordering::SeqCst);
    let held = tokio::spawn({
        let client = client.clone();
        let epoch = epoch.clone();
        async move { client.discovery_home(&epoch).await.unwrap() }
    });
    api.batch_started.notified().await;
    let active = client
        .play_selection(
            &epoch,
            PlaySelection::Songs {
                ids: vec!["901".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    api.release_batch.notify_one();
    let home = serde_json::to_value(held.await.unwrap().value).unwrap();
    assert!(home["recallGuide"].is_null());
    let after = client.player_snapshot(&epoch).await.unwrap();
    assert_eq!(after.state, PlaybackStatus::Playing);
    assert_eq!(after.play_id, active.play_id);
    assert_eq!(after.entry_id, active.entry_id);
    assert_eq!(
        recall_rows(&client.player_queue(&epoch).await.unwrap()),
        ["901"]
    );
    assert_eq!(device.state.lock().unwrap().loads, 1);
}

#[tokio::test(start_paused = true)]
async fn recall_cooldown_uses_server_day_boundary_and_never_repeats_the_shown_guide() {
    use cumulus_core::client::dto::{PlayerAction, QueueEdit};
    let (_root, api, _, client) = fixture();
    let (client, device) = make_recall_player(client);
    let epoch = client.account().unwrap().epoch;
    api.recall.store(2, Ordering::SeqCst);
    api.now.store(TEST_CLOCK, Ordering::SeqCst);
    client.refresh_pc_presentation().await.unwrap();
    let home = serde_json::to_value(client.discovery_home(&epoch).await.unwrap().value).unwrap();
    let guide = home["recallGuide"]["receipt"].as_str().unwrap();
    tokio::time::advance(std::time::Duration::from_secs(3)).await;
    client.acknowledge_home_recall_guide(&epoch, guide).unwrap();

    let queue = client.player_queue(&epoch).await.unwrap();
    client
        .player_action(
            &epoch,
            PlayerAction::QueueFor {
                expected_queue_revision: queue.revision,
                expected_room_id: None,
                edit: QueueEdit::Clear,
            },
        )
        .await
        .unwrap();
    let empty = client.player_queue(&epoch).await.unwrap();
    assert!(empty.tracks.is_empty());
    for earlier in [TEST_CLOCK - 1000, TEST_CLOCK + 86_399_999] {
        api.now.store(earlier, Ordering::SeqCst);
        let page =
            serde_json::to_value(client.discovery_home(&epoch).await.unwrap().value).unwrap();
        assert!(page["recallGuide"].is_null());
        let after = client.player_queue(&epoch).await.unwrap();
        assert!(after.tracks.is_empty());
        assert_eq!(after.revision, empty.revision);
    }
    api.now.store(TEST_CLOCK + 86_400_000, Ordering::SeqCst);
    let page = serde_json::to_value(client.discovery_home(&epoch).await.unwrap().value).unwrap();
    assert!(page["recallGuide"].is_null());
    assert_eq!(
        recall_rows(&client.player_queue(&epoch).await.unwrap()),
        ["811", "812"]
    );
    assert_eq!(device.state.lock().unwrap().loads, 0);
}

#[tokio::test]
async fn newer_home_does_not_prefill_while_the_personalization_opt_out_is_sending() {
    use cumulus_core::client::dto::{
        AccountSettingsCommand, SettingChange, SettingKey, SettingValue,
    };
    let (_root, api, _, client) = fixture();
    let (client, device) = make_recall_player(client);
    let client = Arc::new(client);
    let epoch = client.account().unwrap().epoch;
    api.recall.store(1, Ordering::SeqCst);
    client.refresh_pc_presentation().await.unwrap();
    let settings = client.account_settings_page(&epoch, true).await.unwrap();
    api.hold_opt_out.store(true, Ordering::SeqCst);
    let change = tokio::spawn({
        let client = client.clone();
        let epoch = epoch.clone();
        async move {
            client
                .account_settings_command(
                    &epoch,
                    AccountSettingsCommand::Change {
                        id: "00000000000000000000000000000017".into(),
                        expected_revision: settings.value.revision.unwrap(),
                        change: SettingChange {
                            key: SettingKey::PersonalServiceSetting,
                            value: SettingValue::Number(2),
                        },
                    },
                )
                .await
                .unwrap()
        }
    });
    api.opt_out_started.notified().await;
    let home = serde_json::to_value(client.discovery_home(&epoch).await.unwrap().value).unwrap();
    assert_eq!(
        home["recommendationEnabled"], true,
        "the held opt-out has not committed remotely yet"
    );
    assert!(home["recallGuide"].is_null());
    assert!(client.player_queue(&epoch).await.unwrap().tracks.is_empty());
    api.release_opt_out.notify_one();
    change.await.unwrap();
    let home = serde_json::to_value(client.discovery_home(&epoch).await.unwrap().value).unwrap();
    assert_eq!(home["recommendationEnabled"], false);
    assert_eq!(device.state.lock().unwrap().loads, 0);
}
