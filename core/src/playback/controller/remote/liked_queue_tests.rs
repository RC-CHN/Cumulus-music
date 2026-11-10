use super::*;
use crate::{
    account::AccountSession,
    playback::{
        QueueEntry,
        engine::{AudioEngine, Cancellation, EngineSnapshot, LoadRequest, LoadedMedia},
        source::SourceResolver,
    },
    podcast::LikedVisit,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{Mutex, atomic::AtomicBool};
use tokio::sync::{mpsc, oneshot};

struct NoExternalWork;
impl Api for NoExternalWork {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        panic!("unexpected request from queue admission: {path}")
    }
}
impl AudioEngine for NoExternalWork {
    fn load(&mut self, _: LoadRequest, _: &Cancellation) -> Result<LoadedMedia> {
        panic!("queue admission opened the decoder")
    }
    fn pause(&mut self, _: bool) -> Result<()> {
        unreachable!()
    }
    fn seek(&mut self, _: f64) -> Result<()> {
        unreachable!()
    }
    fn volume(&mut self, _: f64) -> Result<()> {
        unreachable!()
    }
    fn speed(&mut self, _: f64) -> Result<()> {
        unreachable!()
    }
    fn stop(&mut self) -> Result<()> {
        unreachable!()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        unreachable!()
    }
}

fn actor(root: &std::path::Path) -> Actor {
    let store = Arc::new(StateStore::open(root).unwrap());
    let api = Arc::new(NoExternalWork);
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let (sender, _) = mpsc::unbounded_channel();
    let mut actor = Actor::new(
        session.clone(),
        store,
        Arc::new(SourceResolver::new(session, None)),
        Arc::new(Mutex::new(Box::new(NoExternalWork))),
        sender,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    actor
        .queue
        .replace(vec![json!({"id":"900"})], 0, json!({}))
        .unwrap();
    actor
}

fn update(actor: &Actor) -> super::super::QueueUpdate {
    let mut queue = actor.queue.snapshot();
    queue.items.push(QueueEntry {
        entry_id: "episode".into(),
        resource: json!({"id":"503"}),
        source: json!({"kind":"podcast","scene":"myLikedVoice"}),
    });
    queue.order.push("episode".into());
    super::super::QueueUpdate {
        expected: actor.snapshot().fence(),
        queue,
        transport: None,
    }
}

#[test]
fn liked_queue_next_refuses_a_visit_retired_after_the_last_general_check() {
    let root = tempfile::tempdir().unwrap();
    let mut actor = actor(root.path());
    let before = actor.queue.snapshot();
    let revision = actor.queue.revision();
    let visit = LikedVisit::default();
    let fence = visit.fence(&actor.token, 0);
    let session = actor.session.clone();
    let token = actor.token.clone();
    let authority = Cancellation::new(move || {
        // This may re-enter AccountSession and it intentionally still
        // reports success after the visit has become invalid for commit.
        session.check_generation(&token).unwrap();
        visit.retire();
        true
    });
    let (reply, _receiver) = oneshot::channel();
    let value = update(&actor);
    assert!(matches!(
        actor.queue_scoped_podcast(value, &reply, &authority, &fence),
        Err(Error::QueueChanged)
    ));
    assert_eq!(actor.queue.snapshot(), before);
    assert_eq!(actor.queue.revision(), revision);

    let current = LikedVisit::default().fence(&actor.token, 0);
    let value = update(&actor);
    actor
        .queue_scoped_podcast(value, &reply, &Cancellation::default(), &current)
        .unwrap();
    assert_eq!(actor.queue.len(), 2);
    assert_eq!(actor.queue.selected(), before.selected());
}

#[tokio::test]
async fn liked_queue_play_refuses_an_external_generation_change_after_the_last_check() {
    let root = tempfile::tempdir().unwrap();
    let mut actor = actor(root.path());
    let external = StateStore::open(root.path()).unwrap();
    let before = actor.queue.snapshot();
    let revision = actor.queue.revision();
    let durable = external.get("9", "playback", "queue").unwrap();
    let fence = LikedVisit::default().fence(&actor.token, 0);
    let authority = Cancellation::new(move || {
        external
            .put("9", "podcast-cache-generation-v1", "current", &json!(1))
            .unwrap();
        true
    });
    let (reply, _receiver) = oneshot::channel();
    let mut value = update(&actor);
    value.queue.current = Some("episode".into());
    assert!(matches!(
        actor
            .update_queue_inner(
                value,
                Some(&reply),
                None,
                false,
                Some(&authority),
                Some(&fence)
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(actor.queue.snapshot(), before);
    assert_eq!(actor.queue.revision(), revision);
    assert_eq!(actor.store.get("9", "playback", "queue").unwrap(), durable);
}
