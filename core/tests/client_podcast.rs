#[path = "client_podcast/audiobook.rs"]
mod audiobook;
#[path = "client_podcast/badges.rs"]
mod badges;
#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;
#[path = "client_podcast/program_menu.rs"]
mod program_menu;
#[path = "client_podcast/recommendations.rs"]
mod recommendations;
#[path = "client_podcast/search_playback.rs"]
mod search_playback;
#[path = "client_podcast/vinyl_subscription.rs"]
mod vinyl_subscription;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    podcast::PodcastScope,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<(String, Value)>>,
    menu_count_hold: AtomicBool,
    menu_count_started: tokio::sync::Notify,
    menu_count_release: tokio::sync::Notify,
    count: AtomicUsize,
    resume: AtomicBool,
    liked: Mutex<Vec<u64>>,
    lost: AtomicBool,
    offline: AtomicBool,
    personalization_disabled: AtomicBool,
    search_creator: AtomicUsize,
    search_playback_response: Mutex<Value>,
    search_playback_revision: AtomicUsize,
    search_playback_deny: AtomicUsize,
    search_playback_hold: AtomicBool,
    search_playback_started: tokio::sync::Notify,
    search_playback_release: tokio::sync::Notify,
    progress_revision: AtomicUsize,
    progress: Mutex<Vec<Value>>,
    progress_hold: AtomicBool,
    progress_started: tokio::sync::Notify,
    progress_released: tokio::sync::Notify,
    trial: AtomicBool,
    audiobook: AtomicBool,
    recommendation_response: Mutex<Value>,
    recommendation_hold: AtomicBool,
    recommendation_resume_treatment: AtomicBool,
    recommendation_started: tokio::sync::Notify,
    recommendation_release: tokio::sync::Notify,
    recommendation_collect_fixture: AtomicBool,
    recommendation_collected: AtomicBool,
    vinyl_fixture: AtomicBool,
    vinyl_subscribed: AtomicBool,
    vinyl_hold_detail: AtomicBool,
    vinyl_detail_started: tokio::sync::Notify,
    vinyl_detail_release: tokio::sync::Notify,
}
fn program(id: u64) -> Value {
    json!({"id":id,"name":format!("Episode {id}"),"mainSong":{"id":id+1000,"duration":120000},"duration":120000,"radio":{"id":7,"name":"Radio"},"dj":{"nickname":"Host"},"djPlayRecordVo":{"listenLocation":15000,"isListened":false},"cookie":"SECRET"})
}
fn radio() -> Value {
    json!({"id":7,"name":"Radio","programCount":205,"subed":true,"picUrl":"https://p1.music.126.net/art.jpg?token=SECRET","cookie":"SECRET"})
}
impl Api for Backend {
    fn search_privilege_credential_revision(&self) -> cumulus_core::Result<Option<String>> {
        let revision = self.search_playback_revision.load(Ordering::SeqCst);
        Ok((revision > 0).then(|| revision.to_string()))
    }

    fn verified_pc_experiment_revision(&self) -> cumulus_core::Result<Option<String>> {
        Ok(self
            .recommendation_resume_treatment
            .load(Ordering::SeqCst)
            .then(|| "synthetic-podcast-resume".to_owned()))
    }

    fn verified_pc_experiment_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, cumulus_core::protocol::pc_experiments::PcExperimentGroups> {
        Box::pin(async move {
            assert_eq!(revision, "synthetic-podcast-resume");
            cumulus_core::protocol::pc_experiments::PcExperimentGroups::refreshed(
                &json!({"data":[{"expName":"Podcast_ResumePlaySwitch","expGroupName":"t1"}]}),
            )
        })
    }

    fn podcast_progress_revision(&self) -> cumulus_core::Result<Option<String>> {
        let revision = self.progress_revision.load(Ordering::SeqCst);
        Ok((revision > 0).then(|| revision.to_string()))
    }

    fn upload_podcast_progress<'a>(
        &'a self,
        payload: Value,
        revision: &'a str,
        allowed: &'a cumulus_core::protocol::api::PodcastProgressPermission,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            if self.podcast_progress_revision()?.as_deref() != Some(revision) {
                return Err(Error::CredentialChanged);
            }
            if !allowed()? {
                return Err(Error::Cancelled);
            }
            self.progress.lock().unwrap().push(payload);
            self.progress_started.notify_one();
            if self.progress_hold.load(Ordering::SeqCst) {
                self.progress_released.notified().await;
                return Err(Error::State("ambiguous progress result"));
            }
            Ok(())
        })
    }

    fn request<'a>(&'a self, path: &'a str, p: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), p.clone()));
            if self.offline.load(Ordering::SeqCst) {
                return Err(Error::State("fixture offline"));
            }
            Ok(match path {
                "/api/search/pc/complex/page" => {
                    assert_eq!(p["keyword"], "synthetic search podcast");
                    self.search_playback_response.lock().unwrap().clone()
                }
                "/api/djradio/my/radio/recommend" => {
                    assert!(!cache);
                    assert_eq!(p, json!({}));
                    if self.recommendation_hold.load(Ordering::SeqCst) {
                        self.recommendation_started.notify_one();
                        self.recommendation_release.notified().await;
                    }
                    self.recommendation_response.lock().unwrap().clone()
                }
                "/api/resource/commentInfo/list" => {
                    assert!(!cache);
                    assert_eq!(p["resourceType"], 1);
                    let ids: Vec<String> =
                        serde_json::from_str(p["resourceIds"].as_str().unwrap())?;
                    assert_eq!(ids.len(), 1);
                    if self.menu_count_hold.swap(false, Ordering::SeqCst) {
                        self.menu_count_started.notify_one();
                        self.menu_count_release.notified().await;
                    }
                    let id = &ids[0];
                    let thread = if id == "2" {
                        format!("R_SO_4_{id}")
                    } else {
                        format!("A_DJ_1_{id}")
                    };
                    json!({"code":200,"data":[{"threadId":thread,"commentCount":54}]})
                }
                "/api/user/setting" => json!({"code":200,"setting":{"personalServiceSetting":
                    if self.personalization_disabled.load(Ordering::SeqCst) {2} else {0}}}),
                "/api/social/my/recent/update/voice/list/v1" => {
                    if p == json!({}) {
                        let mut row = program(42);
                        row["liked"] = json!(true);
                        row["adjustedPlayCount"] = json!(42);
                        json!({"code":200,"data":{"data":[row,program(43)],
                            "hasMore":1,"updateTime":"1700000000123","newVoiceCount":7}})
                    } else {
                        assert_eq!(p, json!({"updateTime":"1700000000123"}));
                        json!({"code":200,"data":{"data":[program(43),program(44)],
                            "hasMore":false,"newVoiceCount":99}})
                    }
                }
                "/api/voice/homepage/block/page" => {
                    assert!(!self.personalization_disabled.load(Ordering::SeqCst));
                    assert_eq!(p["cursor"], "");
                    json!({"code":200,"data":{"cursor":"PRIVATE_FIXTURE_CURSOR","hasMore":true,
                        "blocks":[{"showType":"VOICE_HOMEPAGE_FIXED_VOICELIST",
                            "uiElement":{"mainTitle":{"title":"Fixture radio"}},
                            "creatives":[{"creativeExtInfoVO":{"radio":radio()}}]}]}})
                }
                "/api/dj/radio/category/radio/list" => {
                    assert_eq!(
                        p,
                        json!({"parentId":"2","subId":"000","cursor":"","limit":20})
                    );
                    json!({"code":200,"data":{"radioList":[
                        {"id":"41","name":"Fractional","score":8.75,"cookie":"SECRET"},
                        {"id":"42","name":"Explicit numeric zero","score":0},
                        {"id":"43","name":"Explicit text zero","score":"0"},
                        {"id":"44","name":"Retained precision","score":"9.50"},
                        {"id":"45","name":"Missing","chartMetaVO":{"score":9.7}},
                        {"id":"46","name":"Invalid text","score":"https://ignored.invalid/SECRET"},
                        {"id":"47","name":"Invalid object","score":{"value":9.5,"secret":"SECRET"}},
                        {"id":"48","name":"Signed number","score":-0.25}
                    ],"page":{"cursor":"SECRET_CURSOR","more":true}}})
                }
                "/api/djradio/v3/get" => {
                    let mut value = radio();
                    if self.vinyl_fixture.load(Ordering::SeqCst) {
                        assert_eq!(p, json!({"id":"7"}));
                        assert!(!cache);
                        if self.vinyl_hold_detail.swap(false, Ordering::SeqCst) {
                            self.vinyl_detail_started.notify_one();
                            self.vinyl_detail_release.notified().await;
                        }
                        value["subed"] = json!(self.vinyl_subscribed.load(Ordering::SeqCst));
                        value["dj"] = json!({"userId":55});
                    }
                    if self.recommendation_collect_fixture.load(Ordering::SeqCst) {
                        value["subed"] =
                            json!(self.recommendation_collected.load(Ordering::SeqCst));
                    }
                    if self.search_creator.load(Ordering::SeqCst) > 0 {
                        value["dj"] = json!({"userId":self.search_creator.load(Ordering::SeqCst)});
                    }
                    json!({"code":200,"data":value})
                }
                "/api/v6/dj/program/byradio" => {
                    let count = self.count.load(Ordering::SeqCst);
                    let offset = p["offset"].as_u64().unwrap() as usize;
                    let limit = p["limit"].as_u64().unwrap() as usize;
                    let mut ids = (1..=count as u64).collect::<Vec<_>>();
                    if p["asc"] == false {
                        ids.reverse();
                    }
                    json!({"code":200,"data":{"programs":ids.into_iter().skip(offset).take(limit).map(|id| self.episode(id)).collect::<Vec<_>>(),"count":count,"more":offset+limit<count,"asc":p["asc"]}})
                }
                "/api/content/my/liked/voice" => {
                    json!({"code":200,"data":{"data":self.liked.lock().unwrap().iter().map(|id|json!({"voiceId":id,"voiceName":"Liked","likeTime":1234})).collect::<Vec<_>>(),"hasMore":false}})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":u8::from(self.resume.load(Ordering::SeqCst))}})
                }
                "/api/dj/program/detail" => {
                    json!({"code":200,"program":self.episode(p["id"].as_str().unwrap().parse().unwrap())})
                }
                "/api/voice/alg/skip/info/get" if self.audiobook.load(Ordering::SeqCst) => {
                    assert_eq!(p, json!({"programIdsStr":["1"]}));
                    json!({"code":200,"data":[{"programId":1,"skipBeginDuration":26,"skipEndDuration":16}]})
                }
                "/api/dj/play/url/check" => {
                    let id = p["voiceId"].as_str().unwrap().parse::<u64>().unwrap();
                    if self.search_playback_hold.swap(false, Ordering::SeqCst) {
                        self.search_playback_started.notify_one();
                        self.search_playback_release.notified().await;
                    }
                    let allowed = self.search_playback_deny.load(Ordering::SeqCst) != id as usize;
                    json!({"code":200,"data":{"voiceId":id,"songId":id+1000,"canPlay":allowed}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap()).unwrap();
                    let trial =
                        self.trial.load(Ordering::SeqCst) && (ids[0] == "1001" || ids[0] == 1001);
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/program.mp3?key=SECRET","md5":"abc","expi":120,"level":"standard",
                        "freeTrialInfo":trial.then(|| json!({"start":0,"end":20}))}]})
                }
                "/api/my/podcast/page/list" => {
                    if self.recommendation_collect_fixture.load(Ordering::SeqCst)
                        && !self.recommendation_collected.load(Ordering::SeqCst)
                    {
                        return Ok(
                            json!({"code":200,"data":{"radioIdList":[],"myPodcastVOList":[]}}),
                        );
                    }
                    json!({"code":200,"data":{"radioIdList":[7],"myPodcastVOList":[radio()]}})
                }
                "/api/djradio/sub" | "/api/djradio/unsub"
                    if self.vinyl_fixture.load(Ordering::SeqCst) =>
                {
                    assert_eq!(p, json!({"id":"7"}));
                    assert!(!cache);
                    self.vinyl_subscribed
                        .store(path.ends_with("/sub"), Ordering::SeqCst);
                    json!({"code":200})
                }
                "/api/djradio/sub"
                    if self.recommendation_collect_fixture.load(Ordering::SeqCst) =>
                {
                    assert_eq!(p, json!({"id":"7"}));
                    self.recommendation_collected.store(true, Ordering::SeqCst);
                    json!({"code":200})
                }
                "/api/resource/like" | "/api/resource/unlike" => {
                    let id = p["threadId"]
                        .as_str()
                        .unwrap()
                        .strip_prefix("A_DJ_1_")
                        .unwrap()
                        .parse()
                        .unwrap();
                    self.liked.lock().unwrap().retain(|v| *v != id);
                    if path.ends_with("/like") {
                        self.liked.lock().unwrap().push(id);
                    }
                    if self.lost.load(Ordering::SeqCst) {
                        return Err(Error::State("lost reply"));
                    }
                    json!({"code":200})
                }
                _ => panic!("unexpected {path} {p}"),
            })
        })
    }
}
impl Backend {
    fn episode(&self, id: u64) -> Value {
        let mut value = program(id);
        if self.vinyl_fixture.load(Ordering::SeqCst) {
            value["dj"]["userId"] = json!(if id == 2 { 9 } else { 55 });
            value["radio"]["subCount"] = json!(331);
        }
        if self.audiobook.load(Ordering::SeqCst) {
            value["radio"]["categoryId"] = json!(10001);
            value["duration"] = json!(240000);
            value["mainSong"]["duration"] = json!(240000);
        }
        value
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    MusicClient,
) {
    let (dir, api, session, client, _) = fixture_with_device();
    (dir, api, session, client)
}
fn fixture_with_device() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    MusicClient,
    Arc<engine::Device>,
) {
    let dir = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    api.count.store(205, Ordering::SeqCst);
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let s = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    s.adopt(api.clone(), json!({"userId":9}), &s.token().unwrap())
        .unwrap();
    let device = Arc::new(engine::Device::default());
    let audio = device.clone();
    let c = MusicClient::with_session(s.clone(), registry)
        .with_audio_factory(move || Ok(engine::audio_engine(audio.clone())));
    (dir, api, s, c, device)
}
#[tokio::test]
async fn category_only_projects_the_official_score_and_preserves_zero_type() {
    let (_dir, api, _session, client) = fixture();
    let response = client
        .podcast_catalogue(
            "1",
            PodcastCatalogueQuery::Radios {
                parent_id: "2".into(),
                sub_id: "000".into(),
                continuation: None,
            },
        )
        .await
        .unwrap();
    let value = serde_json::to_value(response).unwrap();
    assert_eq!(value["epoch"], "1");
    assert_eq!(
        value["value"]["cards"]
            .as_array()
            .unwrap()
            .iter()
            .map(|card| (&card["id"], &card["score"]))
            .collect::<Vec<_>>(),
        [
            (&json!("41"), &json!("8.75")),
            (&json!("42"), &Value::Null),
            (&json!("43"), &json!("0")),
            (&json!("44"), &json!("9.50")),
            (&json!("45"), &Value::Null),
            (&json!("46"), &Value::Null),
            (&json!("47"), &Value::Null),
            (&json!("48"), &json!("-0.25")),
        ]
    );
    assert_eq!(api.calls.lock().unwrap().len(), 1);
    assert!(!value.to_string().contains("SECRET"));
}

fn query(scope: PodcastScope, offset: usize) -> PodcastQuery {
    PodcastQuery {
        scope,
        offset,
        keyword: String::new(),
        limit: 50,
        revision: None,
    }
}
fn scope() -> PodcastScope {
    PodcastScope::Radio {
        id: "7".into(),
        ascending: true,
    }
}
#[tokio::test]
async fn podcast_search_write_rechecks_creator_and_never_repeats_already_applied_state() {
    let (_root, api, _, client) = fixture();
    let command = || PodcastCommand::ChangeSearchSubscription {
        id: "7".into(),
        expected: false,
    };
    for creator in [0, 9] {
        api.search_creator.store(creator, Ordering::SeqCst);
        assert!(matches!(
            client.podcast_command("1", command()).await,
            Err(Error::InvalidInput(_))
        ));
    }
    api.search_creator.store(501, Ordering::SeqCst);
    let result = client.podcast_command("1", command()).await.unwrap();
    assert_eq!(
        serde_json::to_value(result.value).unwrap()["phase"],
        "unchanged"
    );
    let calls = api.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|(path, _)| path == "/api/djradio/v3/get"));
}

#[tokio::test]
async fn podcast_continuation_checks_opt_out_and_retires_old_cursor() {
    let (_dir, api, _, client) = fixture();
    let first = serde_json::to_value(client.podcast_home("1", None).await.unwrap()).unwrap();
    let handle = first["value"]["continuation"].as_str().unwrap();
    assert!(!handle.contains("PRIVATE_FIXTURE_CURSOR"));
    api.personalization_disabled.store(true, Ordering::SeqCst);
    let revoked =
        serde_json::to_value(client.podcast_home("1", Some(handle)).await.unwrap()).unwrap();
    assert_eq!(
        revoked["value"],
        json!({"recommendationEnabled":false,"blocks":[],"continuation":null})
    );
    api.personalization_disabled.store(false, Ordering::SeqCst);
    assert!(matches!(
        client.podcast_home("1", Some(handle)).await,
        Err(Error::StaleResponse)
    ));
    let calls = api.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/user/setting",
            "/api/voice/homepage/block/page",
            "/api/user/setting"
        ]
    );
}
#[tokio::test]
async fn podcast_quality_still_uses_voice_authorization_without_changing_song_preferences() {
    use cumulus_core::membership::Quality;
    let (_dir, api, _session, c) = fixture();
    let p = c.podcast_page("1", query(scope(), 0), true).await.unwrap();
    let play = c
        .play_selection(
            "1",
            PlaySelection::Podcast {
                scope: scope(),
                revision: p.revision,
                start_id: Some("1".into()),
            },
        )
        .await
        .unwrap();
    assert!(play.can_change_quality);
    assert!(!play.quality_preferences.applies_to_current);
    let changed = c
        .player_action(
            "1",
            PlayerAction::QualityFor {
                play_id: play.play_id.unwrap(),
                expected_revision: "podcast".into(),
                level: Quality::HiRes,
            },
        )
        .await
        .unwrap();
    assert_eq!(changed.requested_quality.as_deref(), Some("hires"));
    assert_eq!(changed.quality_preferences.quality, Quality::Standard);
    assert_eq!(
        changed.quality_preferences.revision.as_deref(),
        Some("initial")
    );
    assert!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, p)| path == "/api/song/enhance/player/url/v1" && p["level"] == "hires")
    );
    c.player_action("1", PlayerAction::Stop).await.unwrap();
}
#[tokio::test]
async fn full_radio_queue_survives_page_slicing_and_keeps_voice_identity_private_fields_out() {
    let (_dir, api, session, c) = fixture();
    let p = c
        .podcast_page("1", query(scope(), 150), true)
        .await
        .unwrap();
    assert_eq!(p.episodes.len(), 50);
    assert_eq!(p.total, 205);
    assert_eq!(p.episodes[0].id, "151");
    assert!(!serde_json::to_string(&p).unwrap().contains("SECRET"));
    let _sub = c.subscribe_playback("1").await.unwrap();
    let play = c
        .play_selection(
            "1",
            PlaySelection::Podcast {
                scope: scope(),
                revision: p.revision.clone(),
                start_id: Some("151".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(play.queue_len, 205);
    assert!(play.history_warning.is_none());
    let history = cumulus_core::history::HistoryService::new(session.clone()).unwrap();
    let token = session.token().unwrap();
    assert_eq!(
        history
            .snapshot_for(&token, cumulus_core::history::HistoryKind::Voice)
            .unwrap()
            .local[0]
            .id,
        "151"
    );
    assert_eq!(
        history
            .snapshot_for(&token, cumulus_core::history::HistoryKind::Podcast)
            .unwrap()
            .local[0]
            .id,
        "7"
    );
    assert!(
        history
            .snapshot_for(&token, cumulus_core::history::HistoryKind::Song)
            .unwrap()
            .local
            .is_empty()
    );
    let t = play.track.unwrap();
    assert_eq!(t.id, "1151");
    assert_eq!(t.voice_id.as_deref(), Some("151"));
    let queue = c.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 205);
    assert_eq!(queue.tracks[204].track.voice_id.as_deref(), Some("205"));
    assert!(!serde_json::to_string(&queue).unwrap().contains("SECRET"));
    let calls = api.calls.lock().unwrap().clone();
    assert!(
        calls
            .iter()
            .any(|(path, p)| path == "/api/dj/play/url/check" && p["voiceId"] == "151")
    );
    api.count.store(204, Ordering::SeqCst);
    assert!(matches!(
        c.play_selection(
            "1",
            PlaySelection::Podcast {
                scope: scope(),
                revision: p.revision,
                start_id: None
            }
        )
        .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(c.player_queue("1").await.unwrap().tracks.len(), 205);
    c.player_action("1", PlayerAction::Stop).await.unwrap();
}
#[tokio::test]
async fn resume_uses_remote_location_then_local_checkpoint_and_honors_switch() {
    let (dir, api, _s, c) = fixture();
    api.count.store(1, Ordering::SeqCst);
    api.resume.store(true, Ordering::SeqCst);
    let p = c.podcast_page("1", query(scope(), 0), true).await.unwrap();
    let _sub = c.subscribe_playback("1").await.unwrap();
    let select = || PlaySelection::Podcast {
        scope: scope(),
        revision: p.revision.clone(),
        start_id: None,
    };
    assert_eq!(
        c.play_selection("1", select()).await.unwrap().position,
        Some(15.0)
    );
    c.player_action("1", PlayerAction::Seek { seconds: 42.5 })
        .await
        .unwrap();
    c.player_action("1", PlayerAction::Stop).await.unwrap();
    assert_eq!(
        c.play_selection("1", select()).await.unwrap().position,
        Some(42.5)
    );
    c.player_action("1", PlayerAction::Stop).await.unwrap();
    let store = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let mut v = store
        .get("9", "podcast-positions-v1", "1")
        .unwrap()
        .unwrap();
    v["isListened"] = json!(true);
    store.put("9", "podcast-positions-v1", "1", &v).unwrap();
    assert_eq!(
        c.play_selection("1", select()).await.unwrap().position,
        Some(0.0)
    );
    c.player_action("1", PlayerAction::Seek { seconds: 35.0 })
        .await
        .unwrap();
    c.player_action("1", PlayerAction::Stop).await.unwrap();
    api.resume.store(false, Ordering::SeqCst);
    assert_eq!(
        c.play_selection("1", select()).await.unwrap().position,
        Some(0.0)
    );
    c.player_action("1", PlayerAction::Stop).await.unwrap();
}

async fn played_voice(client: &MusicClient, epoch: &str) -> PlayerView {
    played_voice_from(client, epoch, None).await
}

async fn played_voice_from(client: &MusicClient, epoch: &str, id: Option<&str>) -> PlayerView {
    let page = client
        .podcast_page(epoch, query(scope(), 0), true)
        .await
        .unwrap();
    client
        .play_selection(
            epoch,
            PlaySelection::Podcast {
                scope: scope(),
                revision: page.revision,
                start_id: Some(id.unwrap_or(&page.episodes[0].id).to_owned()),
            },
        )
        .await
        .unwrap()
}

async fn wait_for_progress(api: &Backend, count: usize) -> Vec<Value> {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            {
                let progress = api.progress.lock().unwrap();
                if progress.len() >= count {
                    return progress.clone();
                }
            }
            api.progress_started.notified().await;
        }
    })
    .await
    .expect("no progress request")
}

#[tokio::test]
async fn episode_progress_uses_the_departed_program_and_distinguishes_manual_skip_from_eof() {
    use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus, engine::EndReason};
    let (_dir, api, _session, client, device) = fixture_with_device();
    api.count.store(2, Ordering::SeqCst);
    api.progress_revision.store(1, Ordering::SeqCst);
    api.progress_hold.store(true, Ordering::SeqCst);
    // The terminal Stop below must not stop a newly looped first episode.
    // OrderLoop is the default, so explicitly select the non-wrapping mode.
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Sequential,
            },
        )
        .await
        .unwrap();
    let first = played_voice(&client, "1").await.track.unwrap();
    let id: u64 = first.voice_id.unwrap().parse().unwrap();
    let first_native = device.state.lock().unwrap().view.entry_id.unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 12.75 })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 17.75 })
        .await
        .unwrap();
    assert!(api.progress.lock().unwrap().is_empty());
    // A slow/ambiguous checkpoint must not block moving to the next episode.
    let next = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.player_action("1", PlayerAction::Next),
    )
    .await
    .unwrap()
    .unwrap();
    let next_id: u64 = next
        .track
        .as_ref()
        .unwrap()
        .voice_id
        .as_ref()
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(id, next_id);
    assert_eq!(
        wait_for_progress(&api, 1).await,
        [json!({"programId": id, "listenLocation": 17000.0, "isListened": false})]
    );
    let second_native = device.state.lock().unwrap().view.entry_id.unwrap();
    let loads = device.state.lock().unwrap().loads;
    engine::event(&device, first_native, EndReason::Eof);
    engine::event(&device, second_native, EndReason::Eof);
    api.progress_hold.store(false, Ordering::SeqCst);
    api.progress_released.notify_one();
    assert_eq!(
        wait_for_progress(&api, 2).await,
        [
            json!({"programId": id, "listenLocation": 17000.0, "isListened": false}),
            json!({"programId": next_id, "listenLocation": 120000.0, "isListened": true})
        ]
    );
    // The progress sender runs independently from the player actor. Observe
    // the settled EOF rather than relying on the sender's scheduling order.
    let ended = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let view = client.player_snapshot("1").await.unwrap();
            if view.state == PlaybackStatus::Ended {
                break view;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("second episode did not reach terminal EOF");
    assert_eq!(ended.mode, PlaybackMode::Sequential);
    assert_eq!(ended.play_id, next.play_id);
    assert_eq!(ended.entry_id, next.entry_id);
    assert_eq!(ended.queue_revision, next.queue_revision);
    assert_eq!(ended.track.unwrap().voice_id, Some(next_id.to_string()));
    assert_eq!(ended.position, Some(120.0));
    assert_eq!(device.state.lock().unwrap().loads, loads);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    assert_eq!(device.state.lock().unwrap().loads, loads);
    assert_eq!(api.progress.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn episode_progress_discards_trial_opt_out_and_old_credential_revision() {
    let (_dir, api, _session, client) = fixture();
    api.count.store(2, Ordering::SeqCst);
    api.progress_revision.store(1, Ordering::SeqCst);
    api.trial.store(true, Ordering::SeqCst);
    let trial = played_voice_from(&client, "1", Some("1")).await;
    assert_eq!(
        trial.timeline.unwrap().mode,
        cumulus_core::playback::TimelineMode::Trial
    );
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    let full = played_voice_from(&client, "1", Some("2")).await;
    let id: u64 = full.track.unwrap().voice_id.unwrap().parse().unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 8.0 })
        .await
        .unwrap();
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    assert_eq!(
        wait_for_progress(&api, 1).await,
        [json!({"programId": id, "listenLocation": 8000.0, "isListened": false})]
    );

    // Start an independent account fixture for the preference/revision fences.
    let (_dir, api, session, client) = fixture();
    api.count.store(1, Ordering::SeqCst);
    api.progress_revision.store(1, Ordering::SeqCst);
    let history = cumulus_core::history::HistoryService::new(session.clone()).unwrap();
    let token = session.token().unwrap();
    history.set_sync_for(&token, false).unwrap();
    played_voice(&client, "1").await;
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    history.set_sync_for(&token, true).unwrap();
    played_voice(&client, "1").await;
    client
        .player_action("1", PlayerAction::Seek { seconds: 37.0 })
        .await
        .unwrap();
    api.progress_revision.store(2, Ordering::SeqCst);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    played_voice(&client, "1").await;
    client
        .player_action("1", PlayerAction::Seek { seconds: 58.0 })
        .await
        .unwrap();
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    assert_eq!(
        wait_for_progress(&api, 1).await,
        [json!({"programId": 1, "listenLocation": 58000.0, "isListened": false})]
    );
}

#[tokio::test]
async fn reenabled_history_sync_cannot_release_a_checkpoint_queued_before_opt_out() {
    use cumulus_core::history::{HistoryService, HistorySyncAction};

    // Both the Settings command and the legacy History command can change
    // this account's local preference. Exercise each while the sender is held.
    for settings in [true, false] {
        let (_dir, api, session, client) = fixture();
        api.count.store(3, Ordering::SeqCst);
        api.progress_revision.store(1, Ordering::SeqCst);
        api.progress_hold.store(true, Ordering::SeqCst);
        let depart = |id, seconds| {
            let client = &client;
            async move {
                played_voice_from(client, "1", Some(id)).await;
                client
                    .player_action("1", PlayerAction::Seek { seconds })
                    .await
                    .unwrap();
                client.player_action("1", PlayerAction::Stop).await.unwrap();
            }
        };

        depart("1", 7.5).await;
        let already_dispatched = json!({"programId":1,"listenLocation":7000.0,"isListened":false});
        assert_eq!(
            wait_for_progress(&api, 1).await.as_slice(),
            std::slice::from_ref(&already_dispatched)
        );
        // Program 2 reaches its terminal actor event, but the writer cannot
        // process it while the synthetic transport holds program 1.
        depart("2", 16.5).await;
        assert_eq!(api.progress.lock().unwrap().len(), 1);
        let set = |enabled| {
            if settings {
                assert_eq!(
                    client
                        .history_sync_preference(
                            "1",
                            HistorySyncAction::Set {
                                enabled,
                                confirmed_first_disable: true,
                            },
                        )
                        .unwrap()
                        .value
                        .enabled,
                    enabled
                );
            } else {
                HistoryService::new(session.clone())
                    .unwrap()
                    .set_sync_for(&session.token().unwrap(), enabled)
                    .unwrap();
            }
        };
        set(false);
        set(true);
        depart("3", 29.5).await;
        set(true); // Re-saving an unchanged preference must not drop program 3.
        api.progress_hold.store(false, Ordering::SeqCst);
        api.progress_released.notify_one();
        assert_eq!(
            wait_for_progress(&api, 2).await,
            [
                already_dispatched,
                json!({"programId":3,"listenLocation":29000.0,"isListened":false}),
            ]
        );
    }
}

#[tokio::test]
async fn account_switch_discards_queued_episode_progress_without_replaying_to_the_new_api() {
    let (_dir, old, session, client) = fixture();
    old.count.store(2, Ordering::SeqCst);
    old.progress_revision.store(1, Ordering::SeqCst);
    old.progress_hold.store(true, Ordering::SeqCst);
    played_voice(&client, "1").await;
    client.player_action("1", PlayerAction::Next).await.unwrap();
    wait_for_progress(&old, 1).await;
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    let new = Arc::new(Backend::default());
    new.count.store(1, Ordering::SeqCst);
    new.progress_revision.store(1, Ordering::SeqCst);
    session
        .adopt(new.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    played_voice(&client, "2").await;
    client
        .player_action("2", PlayerAction::Seek { seconds: 9.0 })
        .await
        .unwrap();
    client.player_action("2", PlayerAction::Stop).await.unwrap();
    assert_eq!(
        wait_for_progress(&new, 1).await,
        [json!({"programId": 1, "listenLocation": 9000.0, "isListened": false})]
    );
    assert_eq!(old.progress.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn system_play_resumes_a_paused_voice_but_restarts_a_stopped_voice() {
    let (_dir, api, _s, c) = fixture();
    api.count.store(1, Ordering::SeqCst);
    api.resume.store(true, Ordering::SeqCst);
    let p = c.podcast_page("1", query(scope(), 0), true).await.unwrap();
    let state = c
        .play_selection(
            "1",
            PlaySelection::Podcast {
                scope: scope(),
                revision: p.revision,
                start_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.position, Some(15.0));
    c.player_action("1", PlayerAction::Seek { seconds: 42.5 })
        .await
        .unwrap();
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = c.native_media_snapshot().unwrap().target.unwrap();
    c.native_media_play(&paused).await.unwrap();
    assert_eq!(c.native_media_details().unwrap().position, Some(42.5));
    c.player_action("1", PlayerAction::Stop).await.unwrap();
    let stopped = c.native_media_snapshot().unwrap().target.unwrap();
    c.native_media_play(&stopped).await.unwrap();
    assert_eq!(c.native_media_details().unwrap().position, Some(0.0));
    assert!(matches!(
        c.native_media_play(&paused).await,
        Err(Error::StaleResponse)
    ));
}
#[tokio::test]
async fn uncertain_writes_are_visible_offline_and_isolated_from_the_next_account() {
    let (dir, api, s, c) = fixture();
    let p = c
        .podcast_page("1", query(PodcastScope::Podcasts, 0), true)
        .await
        .unwrap();
    assert_eq!(p.cards.len(), 1);
    api.lost.store(true, Ordering::SeqCst);
    assert!(
        c.podcast_command(
            "1",
            PodcastCommand::Change {
                action: cumulus_core::podcast::PodcastAction::Like,
                id: "1".into(),
                desired: true
            }
        )
        .await
        .is_err()
    );
    api.offline.store(true, Ordering::SeqCst);
    let calls = api.calls.lock().unwrap().len();
    let state = c.podcast_state("1", false).await.unwrap().value;
    assert!(state.liked_ids.is_none());
    assert_eq!(state.operations.len(), 1);
    assert_eq!(state.operations[0].id, "1");
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    let reopened = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    assert!(
        reopened
            .store()
            .get("9", "podcast-mutations-v1", "podcast:like:1")
            .unwrap()
            .is_some()
    );
    s.adopt(api.clone(), json!({"userId":10}), &s.token().unwrap())
        .unwrap();
    assert!(matches!(
        c.podcast_state("1", false).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        c.podcast_state("2", false)
            .await
            .unwrap()
            .value
            .operations
            .is_empty()
    );
    assert_eq!(api.calls.lock().unwrap().len(), calls);
}

#[tokio::test]
async fn native_voice_like_uses_voice_identity_and_recovers_lost_reply_readonly() {
    let (_dir, api, _session, client) = fixture();
    api.count.store(2, Ordering::SeqCst);
    let page = client
        .podcast_page("1", query(scope(), 0), true)
        .await
        .unwrap();
    client
        .play_selection(
            "1",
            PlaySelection::Podcast {
                scope: scope(),
                start_id: Some("2".into()),
                revision: page.revision,
            },
        )
        .await
        .unwrap();
    let selection = client.native_like_selection().unwrap().unwrap();
    assert_eq!(
        selection.resource,
        cumulus_core::client::native_media::NativeLikeResource::Voice("2".into())
    );
    let view = client.native_like_snapshot(&selection).unwrap();
    assert_eq!(view.observed, Some(false));
    api.lost.store(true, Ordering::SeqCst);
    assert!(client.native_like_toggle(&view).await.is_err());
    let pending = client.native_like_snapshot(&selection).unwrap();
    assert!(pending.pending);
    assert!(matches!(
        client.native_like_toggle(&pending).await,
        Err(Error::MutationUncertain)
    ));
    api.lost.store(false, Ordering::SeqCst);
    let liked = client.native_like_refresh(&selection).await.unwrap();
    assert_eq!(liked.observed, Some(true));
    assert!(!liked.pending);
    client.native_like_toggle(&liked).await.unwrap();
    assert_eq!(
        client.native_like_snapshot(&selection).unwrap().observed,
        Some(false)
    );
    let calls = api.calls.lock().unwrap();
    let writes: Vec<_> = calls
        .iter()
        .filter(|(path, _)| path == "/api/resource/like" || path == "/api/resource/unlike")
        .collect();
    assert_eq!(writes.len(), 2);
    assert!(writes.iter().all(|(_, p)| p["threadId"] == "A_DJ_1_2"));
    assert!(!calls.iter().any(|(path, _)| path == "/api/song/like"));
}

#[tokio::test]
async fn recent_podcast_restores_snapshot_fences_account_and_preserves_first_count() {
    let (_dir, api, session, client) = fixture();
    let first = client
        .podcast_recent("1", PodcastRecentQuery::Open)
        .await
        .unwrap();
    let first = serde_json::to_value(first.value).unwrap();
    let revision = first["revision"].as_str().unwrap().to_owned();
    assert_eq!(first["newVoiceCount"], 7);
    assert_eq!(first["hasMore"], true);
    assert_eq!(first["episodes"][0]["listenerCount"], 42);
    assert_eq!(first["episodes"][0]["listenLocationMs"], 15000);
    assert_eq!(first["episodes"][0]["liked"], true);
    assert!(!first.to_string().contains("SECRET"));
    assert!(!first.to_string().contains("1700000000123"));
    assert_eq!(
        serde_json::to_value(
            client
                .podcast_recent("1", PodcastRecentQuery::Open)
                .await
                .unwrap()
                .value
        )
        .unwrap(),
        first
    );
    let next = client
        .podcast_recent(
            "1",
            PodcastRecentQuery::Next {
                revision: revision.clone(),
            },
        )
        .await
        .unwrap();
    let next = serde_json::to_value(next.value).unwrap();
    assert_eq!(next["newVoiceCount"], 7);
    assert_eq!(next["hasMore"], false);
    assert_eq!(
        next["episodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].clone())
            .collect::<Vec<_>>(),
        json!(["42", "43", "44"]).as_array().unwrap().to_owned()
    );
    assert!(matches!(
        client
            .podcast_recent(
                "1",
                PodcastRecentQuery::Next {
                    revision: revision.clone()
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.contains("recent/update"))
            .map(|(_, body)| body.clone())
            .collect::<Vec<_>>(),
        vec![json!({}), json!({"updateTime":"1700000000123"})]
    );

    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.podcast_recent("1", PodcastRecentQuery::Open).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client
            .podcast_recent("2", PodcastRecentQuery::Next { revision })
            .await,
        Err(Error::QueueChanged)
    ));
    let new = serde_json::to_value(
        client
            .podcast_recent("2", PodcastRecentQuery::Open)
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_ne!(new["revision"], first["revision"]);
    assert_eq!(new["episodes"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn radio_order_requires_a_current_view_and_survives_reopening_only_for_its_owner() {
    let (root, api, session, client) = fixture();
    let read = || PodcastRadioOrder::Read { id: "7".into() };
    assert_eq!(client.podcast_radio_order("1", read()).unwrap().value, None);
    let original = client
        .podcast_page("1", query(scope(), 0), true)
        .await
        .unwrap();
    assert_eq!(original.episodes[0].id, "1");
    let set = |revision: &str| PodcastRadioOrder::Set {
        scope: original.scope.clone(),
        revision: revision.into(),
        ascending: false,
    };
    api.offline.store(true, Ordering::SeqCst);
    let calls = api.calls.lock().unwrap().len();
    assert!(matches!(
        client.podcast_radio_order("1", set("obsolete")),
        Err(Error::QueueChanged)
    ));
    assert_eq!(
        client
            .podcast_radio_order("1", set(&original.revision))
            .unwrap()
            .value,
        Some(false)
    );
    assert!(matches!(
        client.podcast_radio_order("1", set(&original.revision)),
        Err(Error::QueueChanged)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), calls);

    let reopened = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let bound = Arc::new(AccountSession::new(api.clone(), Some(reopened.store())));
    bound
        .adopt(api.clone(), json!({"userId":9}), &bound.token().unwrap())
        .unwrap();
    let reopened = MusicClient::with_session(bound, reopened);
    let saved = reopened.podcast_radio_order("1", read()).unwrap().value;
    assert_eq!(saved, Some(false));
    assert_eq!(
        reopened
            .podcast_radio_order("1", PodcastRadioOrder::Read { id: "8".into() })
            .unwrap()
            .value,
        None
    );
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    api.offline.store(false, Ordering::SeqCst);
    let ordered = PodcastScope::Radio {
        id: "7".into(),
        ascending: saved.unwrap(),
    };
    let reopened_view = reopened
        .podcast_page("1", query(ordered, 0), true)
        .await
        .unwrap();
    assert_eq!(reopened_view.episodes[0].id, "205");
    assert_ne!(reopened_view.revision, original.revision);
    let calls = api.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _)| path == "/api/v6/dj/program/byradio")
            .map(|(_, payload)| payload.clone())
            .collect::<Vec<_>>(),
        [
            json!({"radioId":"7","limit":200,"offset":0,"asc":true}),
            json!({"radioId":"7","limit":200,"offset":200,"asc":true}),
            json!({"radioId":"7","limit":200,"offset":0,"asc":false}),
            json!({"radioId":"7","limit":200,"offset":200,"asc":false}),
        ]
    );
    drop(calls);

    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert_eq!(client.podcast_radio_order("2", read()).unwrap().value, None);
    assert!(matches!(
        client.podcast_radio_order("1", read()),
        Err(Error::StaleResponse)
    ));
}
