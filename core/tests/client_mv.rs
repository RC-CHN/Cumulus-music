#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod audio;
#[path = "support/mv_playback.rs"]
mod video;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    membership::Quality,
    mv::playback::VideoStatus,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;
struct Backend {
    mv: Arc<video::Server>,
    items: Mutex<Vec<u32>>,
    offline: AtomicBool,
    lose_write: AtomicBool,
    hold_song: AtomicBool,
    trial: AtomicBool,
    likes: Mutex<Vec<String>>,
    like_writes: Mutex<Vec<Value>>,
    lose_like: AtomicBool,
    started: Notify,
    release: Notify,
}
fn raw(id: u32) -> Value {
    json!({"id":id,"secret":"SECRET","resource":{"mp":{"pl":1080,"dl":1080,"fee":0},"data":{"id":id,"name":format!("MV {id}"),"cover":"http://p1.music.126.net/a.jpg?signature=SECRET","duration":120000,"artists":[{"id":3,"name":"艺术家"}],"brs":[{"br":240,"size":12207,"point":0}]}}})
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            match path {
                "/api/song/like/get" => {
                    Ok(json!({"code":200,"ids":self.likes.lock().unwrap().clone()}))
                }
                "/api/song/like" => {
                    self.like_writes.lock().unwrap().push(payload.clone());
                    let id = payload["trackId"].as_str().unwrap().to_owned();
                    let mut likes = self.likes.lock().unwrap();
                    likes.retain(|value| value != &id);
                    if payload["like"] == true {
                        likes.push(id);
                    }
                    if self.lose_like.swap(false, Ordering::AcqRel) {
                        return Err(Error::Transport("synthetic lost like reply".into()));
                    }
                    Ok(json!({"code":200}))
                }
                "/api/mlog/detail/v1" => {
                    let mut result = self.mv.request(path, payload, cache).await?;
                    result["data"]["resource"]["data"]["relateSong"] = json!([{"id":"7","name":"Related song","privilege":{"id":"7","pl":320000}}]);
                    Ok(result)
                }
                "/api/mlog/mycollect/bytime/get" => {
                    if self.offline.load(Ordering::Acquire) {
                        return Err(Error::Transport("fixture unavailable".into()));
                    }
                    Ok(
                        json!({"code":200,"data":{"feeds":self.items.lock().unwrap().iter().copied().map(raw).collect::<Vec<_>>(),"more":false}}),
                    )
                }
                "/api/mv/sub" | "/api/mv/unsub" => {
                    let value = if path.ends_with("/sub") {
                        &payload["mvId"]
                    } else {
                        &payload["mvIds"][0]
                    };
                    let id = value.as_str().unwrap().parse().unwrap();
                    let mut items = self.items.lock().unwrap();
                    items.retain(|n| *n != id);
                    if path.ends_with("/sub") {
                        items.push(id);
                    }
                    if self.lose_write.swap(false, Ordering::AcqRel) {
                        self.offline.store(true, Ordering::Release);
                        return Err(Error::Transport("lost response".into()));
                    }
                    Ok(json!({"code":200}))
                }
                "/api/v3/song/detail" => {
                    if self.hold_song.swap(false, Ordering::AcqRel) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["c"].as_str().unwrap()).unwrap();
                    Ok(
                        json!({"code":200,"songs":ids.iter().map(|id|json!({"id":id["id"],"name":"Song","dt":120000,"ar":[{"id":3,"name":"Artist"}],"al":{"id":4,"name":"Album","picUrl":"http://p1.music.126.net/cover.jpg?signature=SECRET"}})).collect::<Vec<_>>()}),
                    )
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    Ok(
                        json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/audio?key=SECRET","md5":"abcd","expi":120,"level":"standard","freeTrialInfo":if self.trial.load(Ordering::Acquire) {json!({"start":30,"end":60})} else {Value::Null}}]}),
                    )
                }
                _ => self.mv.request(path, payload, cache).await,
            }
        })
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    video: video::Fixture,
    audio: Arc<audio::Device>,
    api: Arc<Backend>,
    session: Arc<AccountSession>,
    client: Arc<MusicClient>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let video = video::Fixture::new();
        let audio = Arc::new(audio::Device::default());
        let api = Arc::new(Backend {
            mv: video.api.clone(),
            items: Mutex::new((1..=55).collect()),
            offline: AtomicBool::new(false),
            lose_write: AtomicBool::new(false),
            hold_song: AtomicBool::new(false),
            trial: AtomicBool::new(false),
            likes: Mutex::new(vec![]),
            like_writes: Mutex::new(vec![]),
            lose_like: AtomicBool::new(false),
            started: Notify::new(),
            release: Notify::new(),
        });
        let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let dev = audio.clone();
        let client = MusicClient::with_session(session.clone(), registry)
            .with_audio_factory(move || Ok(audio::audio_engine(dev.clone())))
            .with_video_factory(video.factory.clone());
        Self {
            _dir: dir,
            video,
            audio,
            api,
            session,
            client: Arc::new(client),
        }
    }
    async fn until(&self, status: VideoStatus) -> VideoView {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let v = self.client.video_snapshot("1").await.unwrap();
                if v.state == status {
                    return v;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap()
    }
}
fn query(offset: usize) -> MvQuery {
    MvQuery {
        keyword: String::new(),
        offset,
        limit: 24,
        revision: None,
    }
}
fn open() -> VideoAction {
    VideoAction::Open {
        id: "123".into(),
        resolution: 480,
        download_task_id: None,
    }
}
fn songs() -> PlaySelection {
    PlaySelection::Songs {
        ids: vec!["7".into()],
        index: 0,
        quality: Quality::Standard,
    }
}
fn related(song: &str) -> PlaySelection {
    PlaySelection::MvRelated {
        mv_id: "123".into(),
        song_id: song.into(),
        quality: Quality::Standard,
    }
}

#[tokio::test]
async fn mv_related_song_hands_off_and_resumes_same_queue_entry_without_reset() {
    let f = Fixture::new();
    let detail = f.client.mv_detail("1", "123").await.unwrap().value;
    assert_eq!(detail.related_songs[0].id, "7");
    assert_eq!(detail.liked, None);
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    f.client.play_selection("1", related("7")).await.unwrap();
    let mut sub = f.client.subscribe_playback("1").await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while sub.current().unwrap().state
            != cumulus_core::playback::controller::PlaybackStatus::Playing
        {
            sub.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(f.video.last().lock().unwrap().paused);
    f.client
        .player_action("1", PlayerAction::Seek { seconds: 23.0 })
        .await
        .unwrap();
    let before = f.client.player_queue("1").await.unwrap();
    let loads = f.audio.state.lock().unwrap().loads;
    f.client.video_action("1", VideoAction::Play).await.unwrap();
    let resumed = f.client.play_selection("1", related("7")).await.unwrap();
    let after = f.client.player_queue("1").await.unwrap();
    assert_eq!(before.current, after.current);
    assert_eq!(before.revision, after.revision);
    assert_eq!(f.audio.state.lock().unwrap().loads, loads);
    assert!(resumed.position.unwrap_or(0.0) >= 22.0);
    assert!(f.video.last().lock().unwrap().paused);
    assert!(f.client.play_selection("1", related("8")).await.is_err());
    assert_eq!(
        f.client.player_queue("1").await.unwrap().current,
        after.current
    );
    assert!(matches!(
        f.client.play_selection("0", related("7")).await,
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn delayed_related_song_cannot_take_focus_back_from_a_new_video() {
    let f = Fixture::new();
    f.api.hold_song.store(true, Ordering::Release);
    let client = f.client.clone();
    let pending = tokio::spawn(async move { client.play_selection("1", related("7")).await });
    f.api.started.notified().await;
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    f.api.release.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(f.audio.state.lock().unwrap().loads, 0);
    assert!(!f.video.last().lock().unwrap().paused);
}

#[tokio::test]
async fn collection_paging_filtering_revision_and_public_whitelist() {
    let f = Fixture::new();
    let page = f.client.mv_page("1", query(0), true).await.unwrap();
    assert_eq!(page.total, 55);
    assert_eq!(page.cards.len(), 24);
    assert!(page.cards.iter().all(|c| c.subscribed));
    let serialized = serde_json::to_string(&page).unwrap();
    assert!(!serialized.contains("SECRET"));
    assert!(!serialized.contains("signature"));
    let mut next = query(48);
    next.revision = Some(page.revision.clone());
    assert_eq!(
        f.client
            .mv_page("1", next, false)
            .await
            .unwrap()
            .cards
            .len(),
        7
    );
    let mut filter = query(0);
    filter.keyword = "MV 55".into();
    assert_eq!(f.client.mv_page("1", filter, false).await.unwrap().total, 1);
    f.api.items.lock().unwrap().pop();
    let mut stale = query(24);
    stale.revision = Some(page.revision);
    assert!(matches!(
        f.client.mv_page("1", stale, true).await,
        Err(Error::QueueChanged)
    ));
    let detail = f.client.mv_detail("1", "123").await.unwrap();
    assert!(!detail.value.cached);
    assert!(detail.value.downloads[0].available);
    f.video.api.fail_detail.store(true, Ordering::Release);
    assert!(f.client.mv_detail("1", "123").await.unwrap().value.cached);
}
#[tokio::test]
async fn uncertain_mv_mutation_reconciles_without_replaying_and_stale_account_cannot_write() {
    let f = Fixture::new();
    f.client.mv_page("1", query(0), true).await.unwrap();
    f.api.lose_write.store(true, Ordering::Release);
    assert!(f.client.mv_collect("1", "1", false).await.is_err());
    assert_eq!(
        f.client
            .mv_page("1", query(0), false)
            .await
            .unwrap()
            .operations
            .len(),
        1
    );
    f.api.offline.store(false, Ordering::Release);
    assert_eq!(
        f.client
            .mv_reconcile("1", "1")
            .await
            .unwrap()
            .value
            .observed,
        Some(false)
    );
    assert!(
        f.client
            .mv_page("1", query(0), false)
            .await
            .unwrap()
            .operations
            .is_empty()
    );
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.mv_collect("1", "1", true).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        f.client.mv_reconcile("1", "1").await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.api.items.lock().unwrap().len(), 54);
}
#[tokio::test]
async fn actual_music_and_video_controls_handoff_and_frames_never_expose_authorization() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    assert!(!f.audio.state.lock().unwrap().view.paused);
    assert!(
        f.client
            .video_action(
                "1",
                VideoAction::Open {
                    id: "invalid".into(),
                    resolution: 0,
                    download_task_id: None
                }
            )
            .await
            .is_err()
    );
    assert!(f.client.video_action("1", VideoAction::Play).await.is_err());
    assert!(!f.audio.state.lock().unwrap().view.paused);
    f.client.video_action("1", open()).await.unwrap();
    let v = f.until(VideoStatus::Playing).await;
    assert!(f.audio.state.lock().unwrap().view.paused);
    assert!(!f.video.last().lock().unwrap().paused);
    assert!(!serde_json::to_string(&v).unwrap().contains("SECRET"));
    assert!(
        f.client
            .video_frame("1", &v.play_id, 0, 128, 72)
            .unwrap()
            .is_some()
    );
    f.client
        .player_action("1", PlayerAction::Play)
        .await
        .unwrap();
    assert!(f.video.last().lock().unwrap().paused);
    assert!(!f.audio.state.lock().unwrap().view.paused);
    f.client.video_action("1", VideoAction::Play).await.unwrap();
    assert!(f.audio.state.lock().unwrap().view.paused);
    f.client.video_action("1", VideoAction::Stop).await.unwrap();
    assert!(
        f.client
            .video_frame("1", &v.play_id, 0, 128, 72)
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn delayed_song_metadata_and_pending_video_cannot_resume_after_focus_changes() {
    let f = Fixture::new();
    f.api.hold_song.store(true, Ordering::Release);
    let c = f.client.clone();
    let pending = tokio::spawn(async move { c.play_selection("1", songs()).await });
    f.api.started.notified().await;
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    f.api.release.notify_waiters();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(f.audio.state.lock().unwrap().loads, 0);
    f.video.api.hold.store(true, Ordering::Release);
    f.client
        .video_action(
            "1",
            VideoAction::Open {
                id: "124".into(),
                resolution: 240,
                download_task_id: None,
            },
        )
        .await
        .unwrap();
    f.video.api.started.notified().await;
    f.client.play_selection("1", songs()).await.unwrap();
    f.video.api.release.notify_waiters();
    assert_eq!(
        f.client.video_snapshot("1").await.unwrap().state,
        VideoStatus::Idle
    );
    assert!(!f.audio.state.lock().unwrap().view.paused);
    assert_eq!(f.video.devices.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn account_epoch_revokes_video_snapshot_actions_and_frames() {
    let f = Fixture::new();
    f.client.video_action("1", open()).await.unwrap();
    let v = f.until(VideoStatus::Playing).await;
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.video_snapshot("1").await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        f.client.video_action("1", VideoAction::Play).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        f.client.video_frame("1", &v.play_id, 0, 128, 72),
        Err(Error::StaleResponse)
    ));
    assert!(f.client.video_snapshot("2").await.unwrap().id.is_none());
    assert!(f.video.last().lock().unwrap().stopped);
    assert!(
        f.client
            .video_frame("2", &v.play_id, 0, 128, 72)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn cancelled_account_retirement_keeps_old_output_owned_until_native_stop_acknowledges() {
    let f = Fixture::new();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    let device = f.video.last();
    let (gate, started) = {
        let d = device.lock().unwrap();
        (d.hold_stop.clone(), d.stop_started.clone())
    };
    gate.store(true, Ordering::Release);
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let client = f.client.clone();
    let retiring = tokio::spawn(async move { client.video_snapshot("2").await });
    started.notified().await;
    retiring.abort();
    let _ = retiring.await;
    let client = f.client.clone();
    let mut next = tokio::spawn(async move { client.video_action("2", open()).await });
    let waiting = tokio::time::timeout(std::time::Duration::from_millis(40), &mut next)
        .await
        .is_err();
    gate.store(false, Ordering::Release);
    assert!(
        waiting,
        "next account must wait for the previous native output to stop"
    );
    next.await.unwrap().unwrap();
    assert!(device.lock().unwrap().stopped);
    assert_eq!(f.client.video_snapshot("2").await.unwrap().epoch, "2");
}

#[path = "support/native_media_client.rs"]
mod native_media_tests;

#[path = "support/native_likes_client.rs"]
mod native_likes_tests;

#[path = "support/native_media_details.rs"]
mod native_media_details_tests;

#[path = "support/native_media_steps.rs"]
mod native_media_steps_tests;
