#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod playback;

use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    membership::Quality,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

struct Backend {
    group: &'static str,
    calls: Mutex<Vec<(String, Value)>>,
}
impl Api for Backend {
    fn queue_more_experiment_groups(&self) -> ApiFuture<'_, PcExperimentGroups> {
        Box::pin(async {
            PcExperimentGroups::refreshed(&json!({"data":[
                {"expName":"PH-PC-tagT","expGroupName":"c"},
                {"expName":"PH-PC-recommendMore","expGroupName":"t1"},
                {"expName":"PH-PC-recommendMore-songList","expGroupName":self.group}
            ]}))
        })
    }

    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            Ok(match path {
                "/api/user/playlist" => {
                    assert_eq!(payload["uid"], "7");
                    json!({"code":200,"more":false,"playlist":[
                        {"id":"24","userId":"9"},
                        {"id":"22","userId":"7","specialType":5},
                        {"id":"25","userId":"8"}
                    ]})
                }
                "/api/v6/playlist/detail" => {
                    let id = payload["id"].as_str().unwrap();
                    let mut detail = match id {
                        "21" => json!({"userId":7,"specialType":0}),
                        "22" => json!({"creator":{"userId":"7"}}),
                        "23" => json!({"creator":{"userId":8},"subscribed":true}),
                        "24" => json!({"creator":{"userId":9},"subscribed":false}),
                        "25" | "26" => json!({}),
                        "27" => json!({"id":"77","userId":8,"subscribed":false}),
                        _ => panic!("unexpected playlist {id}"),
                    };
                    if id != "27" {
                        detail["id"] = json!(id);
                    }
                    detail["trackIds"] = json!([{"id":"101"},{"id":"102"}]);
                    json!({"code":200,"playlist":detail})
                }
                "/api/v3/song/detail" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["c"].as_str().unwrap()).unwrap();
                    json!({"code":200,"songs":ids.iter().map(|v|json!({
                        "id":v["id"],"name":"Fixture song","dt":120000
                    })).collect::<Vec<_>>()})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,
                        "url":"https://m1.music.126.net/fixture.mp3","md5":"fixture",
                        "expi":120,"level":"standard"}]})
                }
                "/api/homepage/more/rcmd/song" => {
                    assert_eq!(
                        payload,
                        json!({
                            "songIds":["101","102"],"currentPlaySongId":"101",
                            "source":"curlist_more_rcmd"
                        })
                    );
                    json!({"code":200,"data":{"songList":[{"id":103,"name":"Fixture recommendation"}]}})
                }
                "/api/song/enhance/privilege" => {
                    assert_eq!(payload, json!({"ids":"[\"103\"]"}));
                    json!({"code":200,"data":[{"id":103,"st":0,"pl":128000}]})
                }
                _ => panic!("unexpected request {path}"),
            })
        })
    }
}

async fn fixture(
    group: &'static str,
) -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    MusicClient,
) {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend {
        group,
        calls: Mutex::default(),
    });
    let profiles = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(profiles.store())));
    session
        .adopt(api.clone(), json!({"userId":7}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), profiles)
        .with_queue_recommendations()
        .with_audio_factory(|| Ok(playback::audio_engine(Default::default())));
    client.playlists().await.unwrap();
    (root, api, session, client)
}

async fn playlist(client: &MusicClient, epoch: &str, id: &str) -> QueueView {
    let selected = client
        .play_selection(
            epoch,
            PlaySelection::Playlist {
                id: id.into(),
                start_id: None,
                revision: None,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(selected.track.unwrap().id, "101");
    let queue = client.player_queue(epoch).await.unwrap();
    assert_eq!(queue.tracks.len(), 2);
    queue
}

async fn wait_for_assignment(client: &MusicClient, epoch: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !client
            .player_snapshot(epoch)
            .await
            .unwrap()
            .queue_more_available
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn clearing_playlist_uses_native_memberships_and_exact_subgroup_without_extra_reads() {
    for (group, expected) in [
        ("", [false, false, false, true, false, false, false]),
        ("t2", [false, false, true, true, true, false, false]),
        ("t3", [false, true, true, true, true, false, false]),
        ("t4", [true, true, true, true, true, true, false]),
    ] {
        let (_root, api, _session, client) = fixture(group).await;
        playlist(&client, "1", "24").await;
        wait_for_assignment(&client, "1").await;
        for (id, expected) in (21..=27).zip(expected) {
            let queue = playlist(&client, "1", &id.to_string()).await;
            assert_eq!(
                queue.more_recommendations.is_some(),
                expected,
                "playlist {id} in subgroup {group:?}"
            );
            assert_eq!(
                client
                    .player_snapshot("1")
                    .await
                    .unwrap()
                    .queue_more_available,
                expected
            );
            let public = serde_json::to_string(&queue).unwrap();
            assert!(!public.contains("queue_more_playlist_membership"));
        }
        let calls = api.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|(path, _)| path == "/api/user/playlist")
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|(path, _)| path == "/api/v6/playlist/detail")
                .count(),
            8
        );
        assert!(
            !calls
                .iter()
                .any(|(path, _)| path == "/api/homepage/more/rcmd/song")
        );
    }
}

#[tokio::test]
async fn known_subscription_can_open_recommendations_but_cannot_cross_account_generation() {
    let (_root, api, session, client) = fixture("t2").await;
    playlist(&client, "1", "25").await;
    wait_for_assignment(&client, "1").await;
    let original = client.player_queue("1").await.unwrap();
    assert!(original.more_recommendations.unwrap().prefetch_on_open);
    client
        .player_action(
            "1",
            PlayerAction::QueueMoreOpened {
                expected_revision: original.revision,
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let queue = client.player_queue("1").await.unwrap();
            if queue.tracks.len() == 3 {
                let added = &queue.tracks[2];
                assert!(added.more_recommendation);
                assert_eq!(
                    queue.more_recommendations.unwrap().separator_before,
                    Some(added.entry_id.clone())
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    for uid in [8, 7] {
        session
            .adopt(
                api.clone(),
                json!({"userId":uid}),
                &session.token().unwrap(),
            )
            .unwrap();
        let epoch = session.token().unwrap().generation.to_string();
        playlist(&client, &epoch, "24").await;
        wait_for_assignment(&client, &epoch).await;
        playlist(&client, &epoch, "25").await;
        assert!(
            client
                .player_queue(&epoch)
                .await
                .unwrap()
                .more_recommendations
                .is_none()
        );
    }
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| matches!(
                path.as_str(),
                "/api/homepage/more/rcmd/song" | "/api/song/enhance/privilege"
            ))
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/homepage/more/rcmd/song",
            "/api/song/enhance/privilege"
        ]
    );
}
