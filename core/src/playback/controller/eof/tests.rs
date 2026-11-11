use super::{super::state::Loaded, *};
use crate::{
    account::AccountSession,
    playback::{
        PlaybackQueue,
        engine::{AudioEngine, EngineSnapshot, LoadRequest, LoadedMedia},
        source::SourceResolver,
    },
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc;

struct UrlOne;
impl Api for UrlOne {
    fn request<'a>(&'a self, path: &'a str, body: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/song/enhance/player/url/v1");
            let ids: Vec<Value> = serde_json::from_str(body["ids"].as_str().unwrap()).unwrap();
            assert_eq!(crate::protocol::api::identifier(&ids[0]).unwrap(), "1");
            Ok(
                json!({"code":200,"data":[{"id":1,"code":200,"url":"https://m1.music.126.net/fixture.mp3","level":"standard","md5":"abc","expi":120}]}),
            )
        })
    }
}
struct NoDecoder;
impl AudioEngine for NoDecoder {
    fn load(&mut self, _: LoadRequest, _: &Cancellation) -> Result<LoadedMedia> {
        panic!("stale EOF opened a decoder")
    }
    fn pause(&mut self, _: bool) -> Result<()> {
        panic!("stale EOF paused a decoder")
    }
    fn seek(&mut self, _: f64) -> Result<()> {
        panic!("stale EOF sought a decoder")
    }
    fn volume(&mut self, _: f64) -> Result<()> {
        unreachable!()
    }
    fn speed(&mut self, _: f64) -> Result<()> {
        unreachable!()
    }
    fn stop(&mut self) -> Result<()> {
        panic!("stale EOF stopped a decoder")
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        unreachable!()
    }
}

async fn actor(store: Arc<StateStore>) -> Actor {
    let api = Arc::new(UrlOne);
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api, json!({"userId":71}), &session.token().unwrap())
        .unwrap();
    let resolver = Arc::new(SourceResolver::new(session.clone(), None));
    let (sender, _) = mpsc::unbounded_channel();
    let mut actor = Actor::new(
        session,
        store,
        resolver,
        Arc::new(Mutex::new(Box::new(NoDecoder))),
        sender,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    actor.queue.set_mode(PlaybackMode::Sequential).unwrap();
    actor
        .queue
        .replace(
            vec![json!({"id":1}), json!({"id":2}), json!({"id":3})],
            0,
            json!({}),
        )
        .unwrap();
    let item = actor.queue.selected().unwrap();
    let source = actor
        .resolver
        .resolve(&item.resource, &actor.token)
        .await
        .unwrap();
    actor.loaded = Some(Loaded {
        item,
        timeline: source.timeline(Some(40.0)).unwrap(),
        source,
        media: LoadedMedia {
            entry_id: 6,
            duration: Some(40.0),
            codec: None,
        },
        history: Default::default(),
        podcast_progress: None,
        local_equalizer: false,
        indexed_audio: None,
        audiobook_skip: None,
    });
    actor.state = PlaybackStatus::Ended;
    actor.position = Some(40.0);
    actor.play_id = Some("ended-play".into());
    actor
}

fn held_reply(actor: &mut Actor, authority: Cancellation) -> Ready {
    let queue = actor.queue.snapshot();
    let result = Ok(Outcome::PlayExisting {
        target: queue.items[2].entry_id.clone(),
        notify: true,
    });
    let id = Uuid::new_v4();
    // The integration fixture exercises the real classifier. Here the reply is
    // deliberately held between classification and actor delivery.
    actor.eof_flight = Some(Flight {
        id,
        worker: tokio::spawn(std::future::pending()),
    });
    Ready {
        id,
        ticket: Ticket {
            token: actor.token.clone(),
            generation: actor.generation(),
            native_entry: 6,
            fence: actor.snapshot().fence(),
            queue,
            was_paused: false,
            authority,
        },
        result,
    }
}

#[tokio::test]
async fn old_eof_reply_cannot_overwrite_peer_queue_new_intent_or_revoked_native_owner() {
    for cause in [
        "queue",
        "control",
        "authority",
        "entry",
        "account",
        "newer_flight",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(dir.path()).unwrap());
        let mut actor = actor(store.clone()).await;
        let mut notices = actor.skip_notices.subscribe();
        let allowed = Arc::new(AtomicBool::new(true));
        let valid = allowed.clone();
        let old = held_reply(
            &mut actor,
            Cancellation::new(move || valid.load(Ordering::Acquire)),
        );
        let old_current = actor.queue.selected().unwrap().entry_id;
        let mut expected = old_current.clone();
        let mut newer = None;
        match cause {
            "queue" => {
                let mut peer = PlaybackQueue::new(Some(store.clone()), "71").unwrap();
                expected = peer.snapshot().items[1].entry_id.clone();
                peer.select(&expected).unwrap();
            }
            "control" => actor.control_revision += 1,
            "authority" => allowed.store(false, Ordering::Release),
            "entry" => actor.loaded.as_mut().unwrap().media.entry_id = 9,
            "account" => {
                actor
                    .session
                    .adopt(Arc::new(UrlOne), json!({"userId":72}), &actor.token)
                    .unwrap();
            }
            "newer_flight" => newer = Some(held_reply(&mut actor, Cancellation::default()).id),
            _ => unreachable!(),
        }
        actor.finish_ordinary_eof(old).await.unwrap();
        assert!(
            matches!(
                notices.try_recv(),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
            ),
            "{cause}"
        );
        assert_eq!(actor.state, PlaybackStatus::Ended, "{cause}");
        assert_eq!(actor.play_id.as_deref(), Some("ended-play"), "{cause}");
        assert_eq!(actor.position, Some(40.0), "{cause}");
        assert_eq!(actor.eof_flight.as_ref().map(|f| f.id), newer, "{cause}");
        let durable = PlaybackQueue::new(Some(store), "71").unwrap();
        assert_eq!(durable.selected().unwrap().entry_id, expected, "{cause}");
    }
}
