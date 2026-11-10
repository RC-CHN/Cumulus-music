#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    network_diagnostics::DefaultRoute,
    playback::{PlaybackMode, controller::PlaybackStatus},
    podcast::{PodcastScope, PurchasedKind},
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
    deny_permission: AtomicU64,
    purchase_revision_salt: AtomicU64,
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
                "/api/charts/voice" => json!({"code":200,"data":[
                    {"voiceMetaVO":episode(82)}, {"voiceMetaVO":episode(86)}, {"voiceMetaVO":episode(84)}
                ]}),
                "/api/voicelist/bought/list" => {
                    assert_eq!(args, json!({"limit":20,"offset":0}));
                    let mut first = episode(82);
                    first["productType"] = json!(1);
                    first["purchaseProof"] =
                        json!(self.purchase_revision_salt.load(Ordering::SeqCst));
                    let mut second = episode(83);
                    second["productType"] = json!(1);
                    json!({"code":200,"data":{"list":[
                        {"id":82,"name":"Same id radio","productType":0}, first, second
                    ],"count":3,"hasMore":false}})
                }
                "/api/content/my/liked/voice" => {
                    json!({"code":200,"data":{"data":[],"hasMore":false}})
                }
                "/api/resource/commentInfo/list" => {
                    assert_eq!(args, json!({"resourceType":1,"resourceIds":r#"["82"]"#}));
                    json!({"code":200,"data":[{"threadId":"A_DJ_1_82","commentCount":36}]})
                }
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
                    json!({"code":200,"data":{"voiceId":id,"songId":id+100,
                        "canPlay":self.deny_permission.load(Ordering::SeqCst) != id}})
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
    select_gesture(client, receipt, false).await
}
async fn select_gesture(
    client: &MusicClient,
    receipt: String,
    double_click: bool,
) -> Result<PlayerView> {
    let state = client.player_snapshot("1").await?;
    let ticket =
        client.prepare_play_selection("1", &state.queue_revision, state.play_id.as_deref())?;
    client
        .commit_prepared_selection(
            "1",
            &ticket,
            PlaySelection::PodcastBrowseVoice {
                receipt,
                double_click,
            },
        )
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

async fn purchases(client: &MusicClient, refresh: bool) -> String {
    client
        .podcast_page(
            "1",
            PodcastQuery {
                scope: PodcastScope::Purchased,
                keyword: String::new(),
                offset: 0,
                limit: 50,
                revision: None,
            },
            refresh,
        )
        .await
        .unwrap()
        .revision
}

async fn purchase_play(
    client: &MusicClient,
    revision: String,
    id: &str,
    kind: PurchasedKind,
) -> Result<PlayerView> {
    purchase_play_gesture(client, revision, id, kind, false).await
}

async fn purchase_play_gesture(
    client: &MusicClient,
    revision: String,
    id: &str,
    kind: PurchasedKind,
    menu: bool,
) -> Result<PlayerView> {
    let state = client.player_snapshot("1").await?;
    let ticket =
        client.prepare_play_selection("1", &state.queue_revision, state.play_id.as_deref())?;
    client
        .commit_prepared_selection(
            "1",
            &ticket,
            PlaySelection::PodcastPurchasedEpisode {
                target: PodcastPurchasedTarget {
                    kind,
                    id: id.into(),
                    revision,
                },
                menu,
            },
        )
        .await
}

#[tokio::test]
async fn purchased_menu_plays_or_resumes_without_toggling_and_next_is_revision_bound() {
    let (_root, api, _session, client) = fixture();
    let old = old_selection(&client).await;
    let revision = purchases(&client, false).await;
    let target = |id: &str| PodcastPurchasedTarget {
        kind: PurchasedKind::Episode,
        id: id.into(),
        revision: revision.clone(),
    };
    let before = client.player_snapshot("1").await.unwrap();
    let next = |target, queue_revision| PlayerAction::QueuePurchasedProgram {
        target,
        expected_queue_revision: queue_revision,
    };
    let error = client
        .player_action(
            "1",
            next(
                PodcastPurchasedTarget {
                    kind: PurchasedKind::Radio,
                    ..target("82")
                },
                before.queue_revision.clone(),
            ),
        )
        .await;
    assert!(matches!(error, Err(Error::InvalidInput(_))));
    let next_view = client
        .player_action("1", next(target("82"), before.queue_revision.clone()))
        .await
        .unwrap();
    assert_eq!(next_view.state, PlaybackStatus::Paused);
    assert_eq!(next_view.play_id, old.play_id);
    assert_eq!(next_view.entry_id, old.entry_id);
    assert_eq!(next_view.position, old.position);
    assert_eq!(queue(&client).await, ["80", "82"]);
    assert!(matches!(
        client
            .player_action("1", next(target("83"), before.queue_revision))
            .await,
        Err(Error::QueueChanged)
    ));

    let started = purchase_play_gesture(
        &client,
        revision.clone(),
        "82",
        PurchasedKind::Episode,
        true,
    )
    .await
    .unwrap();
    assert_eq!(started.state, PlaybackStatus::Playing);
    assert_eq!(
        started.track.as_ref().unwrap().voice_id.as_deref(),
        Some("82")
    );
    assert_eq!(queue(&client).await, ["80", "82"]);
    api.calls.lock().unwrap().clear();
    let current = purchase_play_gesture(
        &client,
        revision.clone(),
        "82",
        PurchasedKind::Episode,
        true,
    )
    .await
    .unwrap();
    assert_eq!(current.state, PlaybackStatus::Playing);
    assert_eq!(current.play_id, started.play_id);
    let paused = purchase_play(&client, revision.clone(), "82", PurchasedKind::Episode)
        .await
        .unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    let resumed = purchase_play_gesture(
        &client,
        revision.clone(),
        "82",
        PurchasedKind::Episode,
        true,
    )
    .await
    .unwrap();
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(resumed.play_id, started.play_id);
    assert_eq!(resumed.entry_id, started.entry_id);
    assert_eq!(resumed.queue_revision, started.queue_revision);
    assert!(api.calls.lock().unwrap().is_empty());

    let menu = serde_json::to_value(
        client
            .podcast_purchased_program_menu("1", &target("82"), DefaultRoute::Present)
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_eq!(menu, json!({"commentCount":36,"downloadComplete":false}));
    api.purchase_revision_salt.store(1, Ordering::SeqCst);
    assert_ne!(purchases(&client, true).await, revision);
    api.calls.lock().unwrap().clear();
    assert!(
        client
            .podcast_purchased_program_menu("1", &target("82"), DefaultRoute::Present)
            .await
            .is_err()
    );
    assert!(
        client
            .player_action("1", next(target("83"), resumed.queue_revision))
            .await
            .is_err()
    );
    assert!(api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn purchased_next_on_an_empty_queue_authorizes_before_installing_one_episode() {
    let (_root, api, _session, client) = fixture();
    let revision = purchases(&client, false).await;
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Sequential,
            },
        )
        .await
        .unwrap();
    let before = client.player_snapshot("1").await.unwrap();
    let next = |id: &str| PlayerAction::QueuePurchasedProgram {
        target: PodcastPurchasedTarget {
            kind: PurchasedKind::Episode,
            id: id.into(),
            revision: revision.clone(),
        },
        expected_queue_revision: before.queue_revision.clone(),
    };
    api.calls.lock().unwrap().clear();
    api.deny_permission.store(83, Ordering::SeqCst);
    assert!(client.player_action("1", next("83")).await.is_err());
    assert!(queue(&client).await.is_empty());
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path.contains("enhance/player"))
    );
    api.calls.lock().unwrap().clear();
    let played = client.player_action("1", next("82")).await.unwrap();
    assert_eq!(played.state, PlaybackStatus::Playing);
    assert_eq!(
        played.track.as_ref().unwrap().voice_id.as_deref(),
        Some("82")
    );
    assert_eq!(queue(&client).await, ["82"]);
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/dj/program/detail",
            "/api/dj/play/url/check",
            "/api/song/enhance/player/url/v1"
        ]
    );
}

#[tokio::test]
async fn bought_episode_selects_only_the_clicked_voice_and_retains_current_queue_policy() {
    let (_root, api, _session, client) = fixture();
    let old = old_selection(&client).await;
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let revision = purchases(&client, false).await;
    api.calls.lock().unwrap().clear();
    assert!(matches!(
        purchase_play(&client, revision.clone(), "82", PurchasedKind::Radio).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(api.calls.lock().unwrap().is_empty());
    api.deny_permission.store(83, Ordering::SeqCst);
    assert!(
        purchase_play(&client, revision.clone(), "83", PurchasedKind::Episode)
            .await
            .is_err()
    );
    assert_eq!(
        client.player_snapshot("1").await.unwrap().play_id,
        old.play_id
    );
    assert_eq!(queue(&client).await, ["80"]);
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path.contains("enhance/player"))
    );

    api.calls.lock().unwrap().clear();
    let started = purchase_play(&client, revision.clone(), "82", PurchasedKind::Episode)
        .await
        .unwrap();
    assert_eq!(started.state, PlaybackStatus::Playing);
    assert_eq!(started.mode, PlaybackMode::Random);
    let row = started.track.as_ref().unwrap();
    assert_eq!(
        (row.voice_id.as_deref(), row.id.as_str()),
        (Some("82"), "182")
    );
    assert_eq!(queue(&client).await, ["80", "82"]);
    let calls = std::mem::take(&mut *api.calls.lock().unwrap());
    assert_eq!(
        calls
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/dj/program/detail",
            "/api/dj/play/url/check",
            "/api/song/enhance/player/url/v1",
        ]
    );
    assert_eq!(calls[1].1["voiceId"], "82");
    assert_eq!(calls[2].1["ids"], r#"["182"]"#);

    let paused = purchase_play(&client, revision, "82", PurchasedKind::Episode)
        .await
        .unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    assert_eq!(paused.play_id, started.play_id);
    assert_eq!(paused.entry_id, started.entry_id);
    assert_eq!(paused.position, started.position);
    assert!(api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_retired_purchase_list_cannot_submit_late_source_authorization() {
    let (_root, api, _session, client) = fixture();
    let old = old_selection(&client).await;
    let revision = purchases(&client, false).await;
    api.calls.lock().unwrap().clear();
    api.hold_permission.store(true, Ordering::SeqCst);
    let pending = tokio::spawn({
        let client = client.clone();
        let revision = revision.clone();
        async move { purchase_play(&client, revision, "82", PurchasedKind::Episode).await }
    });
    tokio::time::timeout(Duration::from_secs(2), api.entered.notified())
        .await
        .unwrap();
    api.purchase_revision_salt.store(1, Ordering::SeqCst);
    assert_ne!(purchases(&client, true).await, revision);
    api.released.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(
        client.player_snapshot("1").await.unwrap().play_id,
        old.play_id
    );
    assert_eq!(queue(&client).await, ["80"]);
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path.contains("enhance/player"))
    );
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
async fn rank_double_click_uses_bound_chart_or_around_according_to_the_native_preference() {
    let (_root, api, _session, client) = fixture();
    old_selection(&client).await;
    let preview = rank(&client, true).await;
    let full = rank(&client, false).await;
    api.calls.lock().unwrap().clear();
    assert!(matches!(
        select_gesture(&client, preview, true).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(api.calls.lock().unwrap().is_empty());
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let playing = select_gesture(&client, full.clone(), true).await.unwrap();
    assert_eq!(playing.mode, PlaybackMode::Sequential);
    assert_eq!(
        playing.track.as_ref().unwrap().voice_id.as_deref(),
        Some("82")
    );
    assert_eq!(queue(&client).await, ["82", "86", "84"]);
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path == "/api/dj/around/program/v1")
    );

    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    api.calls.lock().unwrap().clear();
    let paused = select_gesture(&client, full, true).await.unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    assert_eq!(paused.play_id, playing.play_id);
    assert_eq!(paused.entry_id, playing.entry_id);
    assert_eq!(paused.position, playing.position);
    assert_eq!(paused.mode, PlaybackMode::Sequential);
    assert_eq!(queue(&client).await, ["82", "86", "84"]);
    assert!(api.calls.lock().unwrap().is_empty());

    old_selection(&client).await;
    client
        .queue_click_preferences_command(
            "1",
            QueueClickPreferencesCommand::Change {
                expected_revision: "0".into(),
                replace_queue: false,
            },
        )
        .unwrap();
    let full = rank(&client, false).await;
    api.calls.lock().unwrap().clear();
    let retained = select_gesture(&client, full, true).await.unwrap();
    assert_eq!(retained.track.unwrap().voice_id.as_deref(), Some("82"));
    assert_eq!(queue(&client).await, ["80", "83", "82", "81"]);
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == "/api/dj/around/program/v1")
            .count(),
        1
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
                PlaySelection::PodcastBrowseVoice {
                    receipt: fresh,
                    double_click: false
                }
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
