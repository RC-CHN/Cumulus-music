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
