#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    playback::controller::PlaybackStatus,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

#[derive(Default)]
struct Fixture {
    calls: Mutex<Vec<(String, Value)>>,
    mode: AtomicU8,
    credential: AtomicU64,
    hold_around: AtomicBool,
    hold_permission: AtomicBool,
    entered: Notify,
    released: Notify,
}

fn episode(id: u64) -> Value {
    json!({"id":id,"name":format!("Episode {id}"),"mainSong":{"id":id+100,"duration":120000},
        "duration":120000,"radio":{"id":7,"name":"Radio"},"djPlayRecordVo":{"listenLocation":17000,"isListened":false}})
}
impl Api for Fixture {
    fn playback_source_credential_revision(&self) -> Result<Option<String>> {
        Ok(Some(self.credential.load(Ordering::SeqCst).to_string()))
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, cached: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), args.clone()));
            Ok(match path {
                "/api/user/setting" => json!({"code":200,"setting":{"personalServiceSetting":0}}),
                "/api/voice/homepage/block/page" => {
                    let first = args["cursor"] == "";
                    let cursor = if first {
                        json!("{\"fixture\":\"next\"}")
                    } else {
                        Value::Null
                    };
                    json!({"code":200,"data":{"hasMore":first,"cursor":cursor,
                    "blocks":[{"showType":"VOICE_HOMEPAGE_WEEKNEW_VOICE",
                    "uiElement":{"mainTitle":{"title":"Week"}},"creatives":[{"resources":[
                        {"resourceId":82,"resourceType":"voice","resourceExtInfo":{"djProgram":episode(82)}}]}]}]}})
                }
                "/api/charts/voice" => json!({"code":200,"data":[{"voiceMetaVO":episode(82)}]}),
                "/api/dj/around/program/v1" => {
                    assert_eq!(args, json!({"programId":"82","roundNum":250}));
                    assert!(!cached);
                    if self.hold_around.swap(false, Ordering::SeqCst) {
                        self.entered.notify_one();
                        self.released.notified().await;
                    }
                    match self.mode.load(Ordering::SeqCst) {
                        0 => {
                            json!({"code":200,"data":{"asc":true,"list":[episode(81),episode(82),episode(83)]}})
                        }
                        1 => json!({"code":200,"data":{}}),
                        2 => return Err(Error::Transport("sealed around failure".into())),
                        3 => json!({"code":200,"data":{"list":[episode(81)]}}),
                        4 => {
                            let mut wrong = episode(82);
                            wrong["mainSong"]["id"] = json!(999);
                            json!({"code":200,"data":{"list":[wrong]}})
                        }
                        5 => json!({"code":200,"data":{"list":[]}}),
                        6 => json!({"data":{"list":[episode(82)]}}),
                        _ => panic!("invalid test mode"),
                    }
                }
                "/api/dj/program/detail" => {
                    let id = args["id"].as_str().unwrap().parse().unwrap();
                    json!({"code":200,"program":episode(id)})
                }
                "/api/dj/play/url/check" => {
                    let id: u64 = args["voiceId"].as_str().unwrap().parse().unwrap();
                    if id == 82 && self.hold_permission.swap(false, Ordering::SeqCst) {
                        self.entered.notify_one();
                        self.released.notified().await;
                    }
                    json!({"code":200,"data":{"voiceId":id,"songId":id+100,"canPlay":true}})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":1}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(args["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/synthetic.mp3","md5":"test","expi":300,"level":"standard"}]})
                }
                _ => panic!("unexpected synthetic route {path}"),
            })
        })
    }
}

fn fixture() -> (
    tempfile::TempDir,
    Arc<Fixture>,
    Arc<AccountSession>,
    Arc<MusicClient>,
) {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Fixture::default());
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let device = Arc::new(engine::Device::default());
    let client = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(move || Ok(engine::audio_engine(device.clone())));
    (root, api, session, Arc::new(client))
}

async fn rank(client: &MusicClient, preview: bool) -> String {
    let response = serde_json::to_value(
        client
            .podcast_rankings(
                "1",
                PodcastRankQuery::Entries {
                    group: PodcastRankGroup::Voice,
                    id: "daily".into(),
                    chart_type: "3".into(),
                    preview,
                },
            )
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    response["rows"][0]["resource"]["voice"]["browsePlaybackReceipt"]
        .as_str()
        .unwrap()
        .into()
}
async fn weekly(client: &MusicClient) -> String {
    let response =
        serde_json::to_value(client.podcast_home("1", None).await.unwrap().value).unwrap();
    response["blocks"][0]["groups"][0][0]["browsePlaybackReceipt"]
        .as_str()
        .unwrap()
        .into()
}
async fn select(client: &MusicClient, receipt: String) -> Result<PlayerView> {
    let state = client.player_snapshot("1").await?;
    let ticket =
        client.prepare_play_selection("1", &state.queue_revision, state.play_id.as_deref())?;
    client
        .commit_prepared_selection("1", &ticket, PlaySelection::PodcastBrowseVoice { receipt })
        .await
}
async fn queue(client: &MusicClient) -> Vec<String> {
    client
        .player_queue("1")
        .await
        .unwrap()
        .tracks
        .into_iter()
        .map(|row| row.track.voice_id.unwrap())
        .collect()
}
async fn old_selection(client: &MusicClient) -> PlayerView {
    client
        .play_selection("1", PlaySelection::Voice { id: "80".into() })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap()
}

#[tokio::test]
async fn preview_and_full_rank_insert_the_window_select_exact_episode_and_toggle_without_rereading()
{
    let (_root, api, _session, client) = fixture();
    let old = old_selection(&client).await;
    let preview = rank(&client, true).await;
    let full = rank(&client, false).await;
    api.calls.lock().unwrap().clear();
    let playing = select(&client, preview.clone()).await.unwrap();
    assert_eq!(playing.state, PlaybackStatus::Playing);
    assert_ne!(playing.play_id, old.play_id);
    assert_eq!(playing.track.unwrap().voice_id.as_deref(), Some("82"));
    assert_eq!(playing.position, Some(17.0));
    assert_eq!(queue(&client).await, ["80", "83", "82", "81"]);
    let paused = select(&client, full).await.unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    let resumed = select(&client, preview).await.unwrap();
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(resumed.play_id, playing.play_id);
    assert_eq!(resumed.queue_revision, playing.queue_revision);
    let calls = api.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _)| path == "/api/dj/around/program/v1")
            .count(),
        1
    );
    assert!(
        !calls
            .iter()
            .any(|(path, _)| path == "/api/v6/dj/program/byradio")
    );
}

#[tokio::test]
async fn absent_empty_and_wrong_identity_leave_old_queue_and_only_throw_can_fallback() {
    let (_root, api, _session, client) = fixture();
    let old = old_selection(&client).await;
    let receipt = weekly(&client).await;
    for mode in [1, 3, 4, 5, 6] {
        api.mode.store(mode, Ordering::SeqCst);
        api.calls.lock().unwrap().clear();
        let error = select(&client, receipt.clone()).await.err().unwrap();
        if matches!(mode, 1 | 5 | 6) {
            assert!(matches!(
                error,
                Error::State("podcast browse around list is empty")
            ));
        } else {
            assert!(matches!(error, Error::QueueChanged));
        }
        assert_eq!(
            *api.calls.lock().unwrap(),
            [(
                "/api/dj/around/program/v1".into(),
                json!({"programId":"82","roundNum":250})
            )]
        );
        let still = client.player_snapshot("1").await.unwrap();
        assert_eq!(still.state, PlaybackStatus::Paused);
        assert_eq!(still.play_id, old.play_id);
        assert_eq!(still.queue_revision, old.queue_revision);
    }
    api.mode.store(2, Ordering::SeqCst);
    let playing = select(&client, receipt).await.unwrap();
    assert_eq!(playing.track.unwrap().voice_id.as_deref(), Some("82"));
    assert_eq!(queue(&client).await, ["80", "82"]);
}

#[tokio::test]
async fn retired_browse_and_changed_credentials_never_dispatch_around() {
    let (_root, api, session, client) = fixture();
    let old = rank(&client, true).await;
    let fresh = rank(&client, true).await;
    api.calls.lock().unwrap().clear();
    assert!(matches!(
        select(&client, old).await,
        Err(Error::StaleResponse)
    ));
    api.credential.fetch_add(1, Ordering::SeqCst);
    assert!(matches!(
        select(&client, fresh).await,
        Err(Error::StaleResponse)
    ));
    let fresh = rank(&client, true).await;
    api.calls.lock().unwrap().clear();
    session
        .adopt(api.clone(), json!({"userId":8}), &session.token().unwrap())
        .unwrap();
    let state = client.player_snapshot("2").await.unwrap();
    let ticket = client
        .prepare_play_selection("2", &state.queue_revision, state.play_id.as_deref())
        .unwrap();
    assert!(matches!(
        client
            .commit_prepared_selection(
                "2",
                &ticket,
                PlaySelection::PodcastBrowseVoice { receipt: fresh }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn held_rights_cannot_dispatch_url_or_overwrite_a_new_native_control() {
    let (_root, api, _session, client) = fixture();
    let old = old_selection(&client).await;
    let receipt = weekly(&client).await;
    api.calls.lock().unwrap().clear();
    api.hold_permission.store(true, Ordering::SeqCst);
    let pending = {
        let client = client.clone();
        tokio::spawn(async move { select(&client, receipt).await })
    };
    tokio::time::timeout(Duration::from_secs(5), api.entered.notified())
        .await
        .unwrap();
    let next = client
        .player_action("1", PlayerAction::Pause { paused: false })
        .await
        .unwrap();
    api.released.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(next.play_id, old.play_id);
    assert_eq!(next.queue_revision, old.queue_revision);
    assert_eq!(queue(&client).await, ["80"]);
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path == "/api/song/enhance/player/url/v1")
    );
}

#[tokio::test]
async fn same_radio_latest_does_not_change_the_clicked_identity_or_supply_a_foreign_clock() {
    let (root, _api, _session, client) = fixture();
    let store = ProfileRegistry::open(root.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let key = "podcast-latest-voice-v1";
    store
        .put(
            "9",
            key,
            "7",
            &json!({"id":"83","mainTrackId":"183","name":"Other","time":42,"duration":120}),
        )
        .unwrap();
    let receipt = weekly(&client).await;
    let clicked = select(&client, receipt.clone()).await.unwrap();
    assert_eq!(clicked.track.unwrap().voice_id.as_deref(), Some("82"));
    assert_eq!(clicked.position, Some(17.0));

    client.player_action("1", PlayerAction::Stop).await.unwrap();
    store
        .put(
            "9",
            key,
            "7",
            &json!({"id":"82","mainTrackId":"182","name":"Exact","time":37,"duration":120}),
        )
        .unwrap();
    let clicked = select(&client, receipt).await.unwrap();
    assert_eq!(clicked.track.unwrap().voice_id.as_deref(), Some("82"));
    assert_eq!(clicked.position, Some(37.0));
    assert_eq!(queue(&client).await, ["83", "82", "81"]);
}

#[tokio::test]
async fn a_refreshed_chart_retires_an_already_started_around_read_before_detail() {
    let (_root, api, _session, client) = fixture();
    let old = old_selection(&client).await;
    let receipt = rank(&client, true).await;
    api.calls.lock().unwrap().clear();
    api.hold_around.store(true, Ordering::SeqCst);
    let pending = {
        let client = client.clone();
        tokio::spawn(async move { select(&client, receipt).await })
    };
    tokio::time::timeout(Duration::from_secs(5), api.entered.notified())
        .await
        .unwrap();
    let fresh = rank(&client, true).await;
    api.released.notify_one();
    assert!(pending.await.unwrap().is_err());
    let still = client.player_snapshot("1").await.unwrap();
    assert_eq!(still.play_id, old.play_id);
    assert_eq!(still.queue_revision, old.queue_revision);
    let calls: Vec<_> = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|(path, _)| path.clone())
        .collect();
    assert_eq!(calls, ["/api/dj/around/program/v1", "/api/charts/voice"]);
    assert_eq!(
        select(&client, fresh)
            .await
            .unwrap()
            .track
            .unwrap()
            .voice_id
            .as_deref(),
        Some("82")
    );
}

#[tokio::test]
async fn current_episode_is_reordered_when_it_is_also_in_the_new_window() {
    let (_root, _api, _session, client) = fixture();
    client
        .play_selection("1", PlaySelection::Voice { id: "81".into() })
        .await
        .unwrap();
    let receipt = rank(&client, true).await;
    let next = select(&client, receipt).await.unwrap();
    assert_eq!(next.track.unwrap().voice_id.as_deref(), Some("82"));
    assert_eq!(queue(&client).await, ["83", "82", "81"]);
}

#[tokio::test]
async fn refresh_retires_the_old_weekly_cursor_before_the_next_feed_read() {
    let (_root, api, _session, client) = fixture();
    async fn open(client: &MusicClient) -> String {
        serde_json::to_value(client.podcast_home("1", None).await.unwrap().value)
            .unwrap()["continuation"].as_str().unwrap().into()
    }
    let old = open(&client).await;
    let fresh = open(&client).await;
    assert_ne!(old, fresh);
    api.calls.lock().unwrap().clear();
    assert!(matches!(
        client.podcast_home("1", Some(&old)).await,
        Err(Error::StaleResponse)
    ));
    assert!(api.calls.lock().unwrap().is_empty());
    client.podcast_home("1", Some(&fresh)).await.unwrap();
    let calls = api.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].0, "/api/voice/homepage/block/page");
    assert_eq!(calls[1].1["cursor"], "{\"fixture\":\"next\"}");
}
