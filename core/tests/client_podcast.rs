#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;
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
    count: AtomicUsize,
    resume: AtomicBool,
    liked: Mutex<Vec<u64>>,
    lost: AtomicBool,
    offline: AtomicBool,
}
fn program(id: u64) -> Value {
    json!({"id":id,"name":format!("Episode {id}"),"mainSong":{"id":id+1000,"duration":120000},"duration":120000,"radio":{"id":7,"name":"Radio"},"dj":{"nickname":"Host"},"djPlayRecordVo":{"listenLocation":15000,"isListened":false},"cookie":"SECRET"})
}
fn radio() -> Value {
    json!({"id":7,"name":"Radio","programCount":205,"subed":true,"picUrl":"https://p1.music.126.net/art.jpg?token=SECRET","cookie":"SECRET"})
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), p.clone()));
            if self.offline.load(Ordering::SeqCst) {
                return Err(Error::State("fixture offline"));
            }
            Ok(match path {
                "/api/djradio/v3/get" => json!({"code":200,"data":radio()}),
                "/api/v6/dj/program/byradio" => {
                    let count = self.count.load(Ordering::SeqCst);
                    let offset = p["offset"].as_u64().unwrap() as usize;
                    let limit = p["limit"].as_u64().unwrap() as usize;
                    let mut ids = (1..=count as u64).collect::<Vec<_>>();
                    if p["asc"] == false {
                        ids.reverse();
                    }
                    json!({"code":200,"data":{"programs":ids.into_iter().skip(offset).take(limit).map(program).collect::<Vec<_>>(),"count":count,"more":offset+limit<count,"asc":p["asc"]}})
                }
                "/api/content/my/liked/voice" => {
                    json!({"code":200,"data":{"data":self.liked.lock().unwrap().iter().map(|id|json!({"voiceId":id,"voiceName":"Liked","likeTime":1234})).collect::<Vec<_>>(),"hasMore":false}})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":u8::from(self.resume.load(Ordering::SeqCst))}})
                }
                "/api/dj/program/detail" => {
                    json!({"code":200,"program":program(p["id"].as_str().unwrap().parse().unwrap())})
                }
                "/api/dj/play/url/check" => {
                    let id = p["voiceId"].as_str().unwrap().parse::<u64>().unwrap();
                    json!({"code":200,"data":{"voiceId":id,"songId":id+1000,"canPlay":true}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/program.mp3?key=SECRET","md5":"abc","expi":120,"level":"standard"}]})
                }
                "/api/my/podcast/page/list" => {
                    json!({"code":200,"data":{"radioIdList":[7],"myPodcastVOList":[radio()]}})
                }
                "/api/resource/like" => {
                    let id = p["threadId"]
                        .as_str()
                        .unwrap()
                        .strip_prefix("A_DJ_1_")
                        .unwrap()
                        .parse()
                        .unwrap();
                    self.liked.lock().unwrap().push(id);
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
fn fixture() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    MusicClient,
) {
    let dir = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    api.count.store(205, Ordering::SeqCst);
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let s = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    s.adopt(api.clone(), json!({"userId":9}), &s.token().unwrap())
        .unwrap();
    let c = MusicClient::with_session(s.clone(), registry)
        .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default()))));
    (dir, api, s, c)
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
async fn full_radio_queue_survives_page_slicing_and_keeps_voice_identity_private_fields_out() {
    let (_dir, api, _s, c) = fixture();
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
