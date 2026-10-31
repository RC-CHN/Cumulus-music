#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod native;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{IntelligenceCommand, IntelligenceOrigin, PlaySelection, PlayerAction},
    },
    membership::Quality,
    playback::{PlaybackQueue, intelligence::IntelligencePhase},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

#[derive(Default)]
struct Backend {
    expired: AtomicBool,
    songs: Mutex<Vec<Value>>,
    rows: Mutex<Vec<Value>>,
    calls: Mutex<Vec<(String, Value)>>,
    ids: Mutex<Vec<Value>>,
    blocked: Mutex<Vec<Value>>,
    hold_catalog: AtomicBool,
    hold_recommend: AtomicBool,
    fail: AtomicBool,
    foreign: AtomicBool,
    catalog: Notify,
    recommend: Notify,
    release: Notify,
    url_count: AtomicUsize,
}
fn song(id: u64) -> Value {
    json!({"id":id,"name":format!("Synthetic {id}"),"dt":120000,"ar":[{"id":20,"name":"Synthetic artist"}],"al":{"id":3,"name":"Synthetic album"},"privilege":{"pl":320000,"maxbr":999000,"flag":0}})
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            Ok(match path {
                "/api/user/playlist" => {
                    json!({"code":200,"more":false,"playlist":[{"id":31,"specialType":5,"userId":if self.foreign.load(Ordering::SeqCst){10}else{9}}]})
                }
                "/api/v6/playlist/detail" => {
                    json!({"code":200,"playlist":{"id":31,"trackIds":self.ids.lock().unwrap().clone()}})
                }
                "/api/v3/song/detail" => {
                    if self.hold_catalog.load(Ordering::SeqCst) {
                        self.catalog.notify_one();
                        self.release.notified().await;
                    }
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["c"].as_str().unwrap()).unwrap();
                    let songs = self.songs.lock().unwrap().clone();
                    json!({"code":200,"songs":songs.into_iter().filter(|s|ids.iter().any(|i|i["id"].as_str()==Some(&s["id"].to_string()))).collect::<Vec<_>>()})
                }
                "/api/music/dislike/get" => {
                    json!({"code":200,"data":{"songIdList":self.blocked.lock().unwrap().clone(),"artistIdList":[]}})
                }
                "/api/playmode/intelligence/list" => {
                    if self.hold_recommend.load(Ordering::SeqCst) {
                        self.recommend.notify_one();
                        self.release.notified().await;
                    }
                    if self.fail.load(Ordering::SeqCst) {
                        return Err(Error::State("synthetic recommendation failure"));
                    }
                    if self.expired.load(Ordering::SeqCst) {
                        return Ok(json!({"code":301}));
                    }
                    json!({"code":200,"data":self.rows.lock().unwrap().clone()})
                }
                "/api/song/enhance/player/url/v1" => {
                    self.url_count.fetch_add(1, Ordering::SeqCst);
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/synthetic.mp3?private=SECRET","md5":"synthetic","expi":120,"level":"standard"}]})
                }
                _ => panic!("unexpected API {path}"),
            })
        })
    }
}
struct Fixture {
    _temp: tempfile::TempDir,
    store: Arc<cumulus_core::storage::StateStore>,
    api: Arc<Backend>,
    session: Arc<AccountSession>,
    client: Arc<MusicClient>,
    device: Arc<native::Device>,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let api = Arc::new(Backend::default());
        *api.ids.lock().unwrap() = (7..=9).map(|id| json!({"id":id,"v":0})).collect();
        *api.songs.lock().unwrap() = (7..=11).map(song).collect();
        *api.rows.lock().unwrap() =
            vec![json!({"id":10,"songInfo":song(10),"recommended":true,"alg":"synthetic"})];
        let registry = ProfileRegistry::open(temp.path(), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let device = Arc::new(native::Device::default());
        let output = device.clone();
        let client = Arc::new(
            MusicClient::with_session(session.clone(), registry)
                .with_audio_factory(move || Ok(native::audio_engine(output.clone()))),
        );
        Self {
            _temp: temp,
            store,
            api,
            session,
            client,
            device,
        }
    }
    async fn revision(&self) -> String {
        self.client.player_queue("1").await.unwrap().revision
    }
    fn start(&self, revision: String, id: &str, seed: Option<&str>) -> IntelligenceCommand {
        IntelligenceCommand::Start {
            operation_id: id.into(),
            expected_queue_revision: revision,
            origin: IntelligenceOrigin::Likes {
                playlist_id: "31".into(),
                revision: None,
                first_id: seed.map(Into::into),
            },
        }
    }
    fn saved(&self) -> PlaybackQueue {
        PlaybackQueue::new(Some(self.store.clone()), "9").unwrap()
    }
    fn requests(&self) -> Vec<Value> {
        self.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == "/api/playmode/intelligence/list")
            .map(|(_, p)| p.clone())
            .collect()
    }
}
fn operation() -> String {
    uuid::Uuid::new_v4().to_string()
}
async fn wait(n: &Notify) {
    tokio::time::timeout(Duration::from_secs(5), n.notified())
        .await
        .unwrap();
}

#[tokio::test]
async fn actual_start_uses_original_count_filters_and_commits_private_metadata_with_seed() {
    let f = Fixture::new();
    let mut denied = song(8);
    denied["privilege"]["pl"] = json!(0);
    f.api.songs.lock().unwrap()[1] = denied;
    *f.api.rows.lock().unwrap() = vec![
        json!({"id":10,"songInfo":song(10)}),
        json!({"id":10,"songInfo":song(10),"alg":"last"}),
        json!({"id":8}),
        json!({"id":99}),
        json!({"id":11,"songInfo":song(11)}),
    ];
    let result = f
        .client
        .player_intelligence("1", f.start(f.revision().await, &operation(), Some("7")))
        .await
        .unwrap();
    assert_eq!(result.intelligence, Some(IntelligencePhase::Active));
    assert_eq!(
        f.requests(),
        [json!({"playlistId":"31","songId":"7","type":"fromPlayOne","startMusicId":"7","count":3})]
    );
    let q = f.client.player_queue("1").await.unwrap();
    assert_eq!(
        q.tracks
            .iter()
            .map(|e| e.track.id.as_str())
            .collect::<Vec<_>>(),
        ["7", "10"]
    );
    let saved = f.saved();
    let s = saved.intelligence_session().unwrap();
    assert_eq!(s.liked_metadata.len(), 3);
    assert_eq!(s.fallback.items.len(), 2);
    assert_eq!(s.recommendations["10"].algorithm, Some(json!("last")));
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    let public = serde_json::to_string(&result).unwrap();
    assert!(!public.contains("SECRET") && !public.contains("liked_metadata"));
}

#[tokio::test]
async fn play_all_chooses_only_eligible_seed_and_requires_account_owned_favorite() {
    let f = Fixture::new();
    *f.api.blocked.lock().unwrap() = vec![json!(7), json!(8)];
    let view = f
        .client
        .player_intelligence("1", f.start(f.revision().await, &operation(), None))
        .await
        .unwrap();
    assert_eq!(view.track.unwrap().id, "9");
    assert_eq!(f.requests()[0]["type"], "fromPlayAll");
    assert_eq!(f.requests()[0]["startMusicId"], "9");
    assert_eq!(f.requests()[0]["count"], 3);
    let g = Fixture::new();
    g.api.foreign.store(true, Ordering::SeqCst);
    assert!(
        g.client
            .player_intelligence("1", g.start(g.revision().await, &operation(), None))
            .await
            .is_err()
    );
    assert_eq!(g.device.state.lock().unwrap().loads, 0);
    assert!(g.requests().is_empty());
}

#[tokio::test]
async fn cancellation_before_seed_is_nonmutating_and_releases_the_feature_gate() {
    let f = Fixture::new();
    f.api.hold_catalog.store(true, Ordering::SeqCst);
    let revision = f.revision().await;
    let id = operation();
    let c = f.client.clone();
    let cmd = f.start(revision.clone(), &id, Some("7"));
    let task = tokio::spawn(async move { c.player_intelligence("1", cmd).await });
    wait(&f.api.catalog).await;
    assert!(matches!(
        f.client
            .player_intelligence("1", f.start(revision.clone(), &operation(), None))
            .await,
        Err(Error::MutationBusy)
    ));
    assert!(f.client.player_intelligence_cancel("1", &id).unwrap());
    assert!(matches!(task.await.unwrap(), Err(Error::Cancelled)));
    assert_eq!(f.revision().await, revision);
    assert!(f.saved().intelligence_session().is_none());
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
    f.api.hold_catalog.store(false, Ordering::SeqCst);
    f.client
        .player_intelligence("1", f.start(revision, &operation(), Some("7")))
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_recommendations_are_explicitly_resumable_without_replaying_seed() {
    let f = Fixture::new();
    f.api.hold_recommend.store(true, Ordering::SeqCst);
    let id = operation();
    let c = f.client.clone();
    let cmd = f.start(f.revision().await, &id, Some("7"));
    let task = tokio::spawn(async move { c.player_intelligence("1", cmd).await });
    wait(&f.api.recommend).await;
    f.client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = f
        .client
        .player_action("1", PlayerAction::Seek { seconds: 10.0 })
        .await
        .unwrap();
    f.client.player_intelligence_cancel("1", &id).unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::Cancelled)));
    assert_eq!(
        f.saved().intelligence_session().unwrap().context.phase,
        IntelligencePhase::Seeded
    );
    f.api.hold_recommend.store(false, Ordering::SeqCst);
    let done = f
        .client
        .player_intelligence(
            "1",
            IntelligenceCommand::Continue {
                operation_id: operation(),
                expected_queue_revision: paused.queue_revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(done.play_id, paused.play_id);
    assert_eq!(done.position, paused.position);
    assert_eq!(done.state, paused.state);
    assert_eq!(done.intelligence, Some(IntelligencePhase::Active));
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    assert_eq!(f.requests().len(), 2);
    assert_eq!(f.requests()[0], f.requests()[1]);
}

#[tokio::test]
async fn failed_recommendation_restores_likes_without_resuming_a_stopped_decoder() {
    let f = Fixture::new();
    f.api.hold_recommend.store(true, Ordering::SeqCst);
    f.api.fail.store(true, Ordering::SeqCst);
    let c = f.client.clone();
    let cmd = f.start(f.revision().await, &operation(), Some("7"));
    let task = tokio::spawn(async move { c.player_intelligence("1", cmd).await });
    wait(&f.api.recommend).await;
    let stopped = f
        .client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    f.api.release.notify_one();
    let done = task.await.unwrap().unwrap();
    assert_eq!(done.state, stopped.state);
    assert_eq!(done.play_id, stopped.play_id);
    assert_eq!(done.intelligence, Some(IntelligencePhase::Fallback));
    assert_eq!(f.client.player_queue("1").await.unwrap().tracks.len(), 3);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
}

#[tokio::test]
async fn delayed_recommendations_cannot_replace_a_new_music_selection() {
    let f = Fixture::new();
    f.api.hold_recommend.store(true, Ordering::SeqCst);
    let c = f.client.clone();
    let cmd = f.start(f.revision().await, &operation(), Some("7"));
    let task = tokio::spawn(async move { c.player_intelligence("1", cmd).await });
    wait(&f.api.recommend).await;
    let fresh = f
        .client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["11".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    f.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    let q = f.client.player_queue("1").await.unwrap();
    assert_eq!(q.tracks[0].track.id, "11");
    assert_eq!(q.revision, fresh.queue_revision);
    assert!(f.saved().intelligence_session().is_none());
}

#[tokio::test]
async fn account_change_cancels_a_pending_read_even_if_backend_never_replies() {
    let f = Fixture::new();
    f.api.hold_catalog.store(true, Ordering::SeqCst);
    let c = f.client.clone();
    let cmd = f.start(f.revision().await, &operation(), Some("7"));
    let task = tokio::spawn(async move { c.player_intelligence("1", cmd).await });
    wait(&f.api.catalog).await;
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::StaleResponse)
    ));
    assert!(f.saved().intelligence_session().is_none());
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
}

#[tokio::test]
async fn stale_playlist_revision_and_blocked_explicit_seed_never_start_audio() {
    let f = Fixture::new();
    let cmd = IntelligenceCommand::Start {
        operation_id: operation(),
        expected_queue_revision: f.revision().await,
        origin: IntelligenceOrigin::Likes {
            playlist_id: "31".into(),
            revision: Some("stale".into()),
            first_id: Some("7".into()),
        },
    };
    assert!(matches!(
        f.client.player_intelligence("1", cmd).await,
        Err(Error::QueueChanged)
    ));
    *f.api.blocked.lock().unwrap() = vec![json!(7)];
    assert!(
        f.client
            .player_intelligence("1", f.start(f.revision().await, &operation(), Some("7")))
            .await
            .is_err()
    );
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
    assert!(f.requests().is_empty());
}

#[tokio::test]
async fn current_song_entry_checks_eligibility_and_restores_the_original_ordinary_queue() {
    use cumulus_core::playback::PlaybackMode;
    let f = Fixture::new();
    let ordinary = f
        .client
        .play_selection(
            "1",
            PlaySelection::Playlist {
                id: "31".into(),
                start_id: Some("8".into()),
                revision: None,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let eligibility = f
        .client
        .player_intelligence_eligibility("1", &ordinary.queue_revision)
        .await
        .unwrap();
    assert!(eligibility.can_enter);
    assert_eq!(eligibility.liked_playlist_id.as_deref(), Some("31"));
    let active = f
        .client
        .player_intelligence(
            "1",
            IntelligenceCommand::Start {
                operation_id: operation(),
                expected_queue_revision: ordinary.queue_revision,
                origin: IntelligenceOrigin::Current,
            },
        )
        .await
        .unwrap();
    assert_eq!(active.track.as_ref().unwrap().id, "8");
    assert_ne!(ordinary.play_id, active.play_id);
    assert_eq!(f.requests()[0]["type"], "fromPlayOne");
    let saved = f.saved();
    let s = saved.intelligence_session().unwrap();
    assert_eq!(s.backup.items.len(), 3);
    assert_eq!(
        s.backup
            .items
            .iter()
            .find(|e| Some(&e.entry_id) == s.backup.current.as_ref())
            .unwrap()
            .resource["id"],
        "8"
    );
    let left = f
        .client
        .player_action(
            "1",
            PlayerAction::ModeFor {
                mode: PlaybackMode::OrderLoop,
                expected_queue_revision: active.queue_revision,
                fm: false,
            },
        )
        .await
        .unwrap();
    assert!(left.intelligence.is_none());
    assert_eq!(left.play_id, active.play_id);
    assert_eq!(left.queue_len, 3);
}

#[tokio::test]
async fn substitutions_deduplicate_after_replacement_and_missing_catalogue_position_stops_selection()
 {
    let f = Fixture::new();
    let mut denied = song(7);
    denied["privilege"]["pl"] = json!(0);
    denied["noCopyrightRcmd"] = json!({"type":1,"songId":8});
    f.api.songs.lock().unwrap()[0] = denied;
    f.client
        .player_intelligence("1", f.start(f.revision().await, &operation(), Some("8")))
        .await
        .unwrap();
    assert_eq!(f.requests()[0]["startMusicId"], "8");
    assert_eq!(
        f.saved()
            .intelligence_session()
            .unwrap()
            .fallback
            .items
            .len(),
        2
    );
    let g = Fixture::new();
    g.api.songs.lock().unwrap().retain(|s| s["id"] != 8);
    g.client
        .player_intelligence("1", g.start(g.revision().await, &operation(), Some("7")))
        .await
        .unwrap();
    assert_eq!(
        g.saved()
            .intelligence_session()
            .unwrap()
            .fallback
            .items
            .len(),
        1
    );
    assert_eq!(g.requests()[0]["count"], 3);
}

#[tokio::test]
async fn verified_plain_download_can_seed_denied_online_song_and_tampering_cannot() {
    use cumulus_core::{
        download::offline::{OfflineLibrary, PublishedDownload},
        media::integrity::inspect_file,
    };
    let f = Fixture::new();
    let mut denied = song(7);
    denied["privilege"]["pl"] = json!(0);
    f.api.songs.lock().unwrap()[0] = denied.clone();
    let path = f._temp.path().join("synthetic.flac");
    std::fs::write(&path, include_bytes!("fixtures/tag_audio/seed.flac")).unwrap();
    let ledger = OfflineLibrary::new(f.session.clone()).unwrap();
    ledger
        .register(
            &f.session.token().unwrap(),
            &PublishedDownload {
                task_id: "synthetic-publication".into(),
                song_id: "7".into(),
                requested_quality: Quality::Lossless,
                target: path.clone(),
                song: denied,
                resource: json!({"id":7,"level":"lossless"}),
                encrypted: false,
                integrity: inspect_file(&path).unwrap(),
            },
        )
        .unwrap();
    let done = f
        .client
        .player_intelligence("1", f.start(f.revision().await, &operation(), Some("7")))
        .await
        .unwrap();
    assert_eq!(done.track.unwrap().id, "7");
    let saved = f.saved();
    assert!(saved.snapshot().items[0].resource["download_ledger"].is_object());
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(p, args)| p == "/api/song/enhance/player/url/v1"
                && args["ids"].as_str().unwrap().contains('7'))
    );
    f.client
        .player_action(
            "1",
            PlayerAction::ModeFor {
                mode: cumulus_core::playback::PlaybackMode::OrderLoop,
                expected_queue_revision: done.queue_revision,
                fm: false,
            },
        )
        .await
        .unwrap();
    std::fs::write(path, b"tampered synthetic media").unwrap();
    assert!(
        f.client
            .player_intelligence("1", f.start(f.revision().await, &operation(), Some("7")))
            .await
            .is_err()
    );
    assert_eq!(f.requests().len(), 1);
}

#[tokio::test]
async fn session_expiration_is_reported_without_mutating_an_invalidated_account() {
    let f = Fixture::new();
    f.api.expired.store(true, Ordering::SeqCst);
    let error = f
        .client
        .player_intelligence("1", f.start(f.revision().await, &operation(), Some("7")))
        .await
        .err()
        .unwrap();
    assert!(error.is_service_code(301));
    assert_eq!(
        f.session.snapshot().unwrap().status,
        cumulus_core::account::SessionStatus::Expired
    );
    assert_eq!(
        f.saved().intelligence_session().unwrap().context.phase,
        IntelligencePhase::Seeded
    );
}

#[tokio::test]
async fn newer_selection_during_catalogue_preparation_cannot_be_overwritten_by_seed() {
    let f = Fixture::new();
    f.api.hold_catalog.store(true, Ordering::SeqCst);
    let c = f.client.clone();
    let cmd = f.start(f.revision().await, &operation(), Some("7"));
    let task = tokio::spawn(async move { c.player_intelligence("1", cmd).await });
    wait(&f.api.catalog).await;
    f.api.hold_catalog.store(false, Ordering::SeqCst);
    let fresh = f
        .client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["11".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    f.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        f.client.player_queue("1").await.unwrap().revision,
        fresh.queue_revision
    );
    assert!(f.requests().is_empty());
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
}

#[tokio::test]
async fn legacy_seeded_session_can_continue_after_client_reopen_without_automatic_http_or_audio() {
    let f = Fixture::new();
    f.api.hold_recommend.store(true, Ordering::SeqCst);
    let id = operation();
    let c = f.client.clone();
    let cmd = f.start(f.revision().await, &id, Some("7"));
    let task = tokio::spawn(async move { c.player_intelligence("1", cmd).await });
    wait(&f.api.recommend).await;
    f.client.player_intelligence_cancel("1", &id).unwrap();
    assert!(task.await.unwrap().is_err());
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    let mut record = f
        .store
        .get("9", "playback-intelligence-v1", "session")
        .unwrap()
        .unwrap();
    record.as_object_mut().unwrap().remove("liked_metadata");
    f.store
        .put("9", "playback-intelligence-v1", "session", &record)
        .unwrap();
    let device = Arc::new(native::Device::default());
    let output = device.clone();
    let reopened = MusicClient::with_session(
        f.session.clone(),
        ProfileRegistry::open(f._temp.path(), ApiOrigin::Pc).unwrap(),
    )
    .with_audio_factory(move || Ok(native::audio_engine(output.clone())));
    let revision = reopened.player_queue("1").await.unwrap().revision;
    assert_eq!(f.requests().len(), 1);
    assert_eq!(device.state.lock().unwrap().loads, 0);
    f.api.hold_recommend.store(false, Ordering::SeqCst);
    let done = reopened
        .player_intelligence(
            "1",
            IntelligenceCommand::Continue {
                operation_id: operation(),
                expected_queue_revision: revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(done.intelligence, Some(IntelligencePhase::Active));
    assert_eq!(device.state.lock().unwrap().loads, 0);
    assert_eq!(f.requests()[0], f.requests()[1]);
    assert_eq!(
        f.saved().intelligence_session().unwrap().backup.items.len(),
        3
    );
}
