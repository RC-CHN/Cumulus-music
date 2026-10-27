use cumulus_core::{
    Error,
    account::{AccountSession, SessionToken},
    playback::{
        TimelineMode,
        source::{ResolverClock, SourceResolver},
    },
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

#[derive(Default)]
struct Clock(AtomicU64);
impl ResolverClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
struct Backend {
    calls: AtomicU64,
    details: AtomicU64,
    row: Mutex<Value>,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}
impl Default for Backend {
    fn default() -> Self {
        Self {
            calls: AtomicU64::new(0),
            details: AtomicU64::new(0),
            row: Mutex::new(
                json!({"id":1,"code":200,"url":"http://m1.music.126.net/authorized.mp3?token=PRIVATE","md5":"abc","expi":"10","level":"standard"}),
            ),
            hold: AtomicBool::new(false),
            started: Notify::new(),
            release: Notify::new(),
        }
    }
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, _payload: Value, _cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            Ok(match path {
                "/api/song/enhance/player/url/v1" => {
                    self.calls.fetch_add(1, Ordering::SeqCst);
                    if self.hold.load(Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    let row = self.row.lock().unwrap().clone();
                    if row.get("outer").is_some() {
                        json!({"code":row["outer"]})
                    } else {
                        json!({"code":200,"data":[row]})
                    }
                }
                "/api/v3/song/detail" => {
                    self.details.fetch_add(1, Ordering::SeqCst);
                    json!({"code":200,"songs":[{"id":1,"dt":120000}]})
                }
                _ => panic!("unexpected {path}"),
            })
        })
    }
}
fn setup() -> (
    Arc<Backend>,
    Arc<Clock>,
    Arc<AccountSession>,
    Arc<SourceResolver>,
) {
    let api = Arc::new(Backend::default());
    let clock = Arc::new(Clock::default());
    let session = Arc::new(AccountSession::new(api.clone(), None));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let resolver = Arc::new(SourceResolver::with_clock(
        session.clone(),
        None,
        clock.clone(),
    ));
    (api, clock, session, resolver)
}
fn spawn(
    resolver: Arc<SourceResolver>,
    token: SessionToken,
) -> tokio::task::JoinHandle<
    cumulus_core::Result<Arc<cumulus_core::playback::source::ResolvedSource>>,
> {
    tokio::spawn(async move { resolver.resolve(&json!({"id":1}), &token).await })
}

#[tokio::test]
async fn cache_is_ttl_quality_encoding_scene_and_account_scoped() {
    let (api, clock, session, resolver) = setup();
    let token = session.token().unwrap();
    let resource = json!({"id":1,"level":"lossless"});
    let first = resolver.resolve(&resource, &token).await.unwrap();
    assert_eq!(first.actual_quality(), Some("standard"));
    resolver.resolve(&resource, &token).await.unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        resolver.cached_until(&resource, &token).unwrap(),
        Some(Duration::from_secs(5))
    );
    clock.0.store(5, Ordering::SeqCst);
    resolver.resolve(&resource, &token).await.unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 2);
    resolver
        .resolve(
            &json!({"id":1,"level":"lossless","encode_type":"mp3"}),
            &token,
        )
        .await
        .unwrap();
    resolver
        .resolve(&json!({"id":1,"level":"lossless","trial_mode":35}), &token)
        .await
        .unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 4);
    session
        .adopt(api.clone(), json!({"userId":9}), &token)
        .unwrap();
    let current = session.token().unwrap();
    assert!(matches!(
        resolver.resolve(&resource, &token).await,
        Err(Error::StaleResponse)
    ));
    resolver.resolve(&resource, &current).await.unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn shared_pending_read_success_and_failure_are_not_replayed() {
    let (api, _, session, resolver) = setup();
    let token = session.token().unwrap();
    api.hold.store(true, Ordering::SeqCst);
    let first = spawn(resolver.clone(), token.clone());
    api.started.notified().await;
    let second = spawn(resolver.clone(), token.clone());
    tokio::task::yield_now().await;
    api.release.notify_waiters();
    let a = first.await.unwrap().unwrap();
    let b = second.await.unwrap().unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(api.calls.load(Ordering::SeqCst), 1);
    resolver.invalidate(None).unwrap();
    api.row.lock().unwrap()["code"] = json!(403);
    let first = spawn(resolver.clone(), token.clone());
    api.started.notified().await;
    let second = spawn(resolver.clone(), token.clone());
    tokio::task::yield_now().await;
    api.release.notify_waiters();
    assert!(first.await.unwrap().err().unwrap().is_service_code(403));
    assert!(second.await.unwrap().err().unwrap().is_service_code(403));
    assert_eq!(api.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn invalidated_and_abandoned_fetches_wake_waiters_without_publishing() {
    let (api, _, session, resolver) = setup();
    let token = session.token().unwrap();
    api.hold.store(true, Ordering::SeqCst);
    let first = spawn(resolver.clone(), token.clone());
    api.started.notified().await;
    resolver.invalidate(Some("1")).unwrap();
    api.release.notify_waiters();
    assert!(matches!(
        first.await.unwrap().err().unwrap().root(),
        Error::StaleResponse
    ));
    assert!(
        resolver
            .cached_until(&json!({"id":1}), &token)
            .unwrap()
            .is_none()
    );
    let owner = spawn(resolver.clone(), token.clone());
    api.started.notified().await;
    let waiter = spawn(resolver.clone(), token.clone());
    tokio::task::yield_now().await;
    owner.abort();
    assert!(owner.await.err().unwrap().is_cancelled());
    assert!(matches!(
        waiter.await.unwrap().err().unwrap().root(),
        Error::Cancelled
    ));
    api.hold.store(false, Ordering::SeqCst);
    resolver.resolve(&json!({"id":1}), &token).await.unwrap();
}

#[tokio::test]
async fn trial_clock_and_full_trial_are_service_facts_not_full_source_fallbacks() {
    let (api, _, session, resolver) = setup();
    let token = session.token().unwrap();
    api.row.lock().unwrap()["freeTrialInfo"] = json!({"start":30,"end":60});
    let source = resolver.resolve(&json!({"id":1}), &token).await.unwrap();
    let timeline = source.timeline(Some(20.0)).unwrap();
    assert_eq!(timeline.snapshot().mode, TimelineMode::Trial);
    assert_eq!(timeline.to_media(30.0).unwrap(), 0.0);
    assert_eq!(timeline.display_end(), Some(50.0));
    assert_eq!(timeline.snapshot().total, Some(120.0));
    assert_eq!(api.details.load(Ordering::SeqCst), 1);
    resolver.invalidate(None).unwrap();
    api.row.lock().unwrap()["freeTrialInfo"] = json!({"start":60,"end":30});
    assert!(resolver.resolve(&json!({"id":1}), &token).await.is_err());
    assert_eq!(api.details.load(Ordering::SeqCst), 1);
    resolver.invalidate(None).unwrap();
    {
        let mut row = api.row.lock().unwrap();
        row["freeTrialInfo"] = Value::Null;
        row["freeTrialPrivilege"] = json!({"resConsumable":1,"userConsumable":true});
    }
    let source = resolver.resolve(&json!({"id":1}), &token).await.unwrap();
    assert_eq!(
        source.timeline(None).unwrap().snapshot().mode,
        TimelineMode::FullTrial
    );
    resolver.invalidate(None).unwrap();
    api.row.lock().unwrap()["md5"] = Value::Null;
    assert!(resolver.resolve(&json!({"id":1}), &token).await.is_err());
}

#[tokio::test]
async fn session_expiration_is_shared_without_hiding_301_or_exposing_old_account_results() {
    let (api, _, session, resolver) = setup();
    let token = session.token().unwrap();
    api.row.lock().unwrap()["outer"] = json!(301);
    let error = resolver
        .resolve(&json!({"id":1}), &token)
        .await
        .err()
        .unwrap();
    assert!(error.is_service_code(301));
    assert_eq!(
        session.snapshot().unwrap().status,
        cumulus_core::account::SessionStatus::Expired
    );
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        resolver.resolve(&json!({"id":1}), &token).await,
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn local_source_requires_library_and_rejects_stale_account_and_cue_container() {
    let (api, _, session, resolver) = setup();
    let token = session.token().unwrap();
    assert!(
        resolver
            .resolve(&json!({"path":"/missing"}), &token)
            .await
            .is_err()
    );
    let dir = tempfile::tempdir().unwrap();
    let store =
        Arc::new(cumulus_core::storage::StateStore::open(dir.path().join("state")).unwrap());
    let library = Arc::new(cumulus_core::library::LocalLibrary::new(store).unwrap());
    let path = dir.path().join("sample.wav");
    std::fs::write(&path, b"synthetic file access fixture").unwrap();
    let local = SourceResolver::new(session.clone(), Some(library));
    let source = local.resolve(&json!({"path":path}), &token).await.unwrap();
    assert!(
        matches!(&source.input,cumulus_core::playback::engine::MediaInput::File(p) if *p==path)
    );
    let cue = dir.path().join("container.cue");
    std::fs::write(&cue, b"FILE sample.wav WAVE").unwrap();
    assert!(local.resolve(&json!({"path":cue}), &token).await.is_err());
    session.adopt(api, json!({"userId":10}), &token).unwrap();
    assert!(matches!(
        local.resolve(&json!({"path":path}), &token).await,
        Err(Error::StaleResponse)
    ));
}
