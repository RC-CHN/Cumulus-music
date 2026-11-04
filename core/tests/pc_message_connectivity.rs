use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, messages::MessageCounts},
    pc_connectivity::PcRequestObservation as Request,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU16, Ordering},
    },
    time::Duration,
};
use tokio::{sync::oneshot, time::Instant};

struct Counter {
    start: Instant,
    value: u64,
    code: AtomicU16,
    times: Mutex<Vec<u64>>,
    hold: Mutex<Option<oneshot::Receiver<()>>>,
}
impl Counter {
    fn new(value: u64) -> Arc<Self> {
        Arc::new(Self {
            start: Instant::now(),
            value,
            code: AtomicU16::new(200),
            times: Mutex::new(Vec::new()),
            hold: Mutex::new(None),
        })
    }
    fn hold_next(&self) -> oneshot::Sender<()> {
        let (tx, rx) = oneshot::channel();
        *self.hold.lock().unwrap() = Some(rx);
        tx
    }
    fn times(&self) -> Vec<u64> {
        self.times.lock().unwrap().clone()
    }
}
impl Api for Counter {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!((path, payload, cache), ("/api/pl/count", json!({}), false));
            self.times
                .lock()
                .unwrap()
                .push(self.start.elapsed().as_millis() as u64);
            let hold = self.hold.lock().unwrap().take();
            if let Some(hold) = hold {
                hold.await.unwrap();
            }
            Ok(
                json!({"code":self.code.load(Ordering::SeqCst), "msg":self.value, "notice":self.value+1,
                "lastPrivateMsg":"should not cross the native count boundary"}),
            )
        })
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    api: Arc<Counter>,
    session: Arc<AccountSession>,
    client: MusicClient,
}
impl Fixture {
    fn new(authenticated: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(temp.path(), ApiOrigin::Pc).unwrap();
        let api = Counter::new(7);
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        if authenticated {
            session
                .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
                .unwrap();
        }
        let client = MusicClient::with_session(session.clone(), registry);
        Self {
            _temp: temp,
            api,
            session,
            client,
        }
    }
}

async fn settle() {
    for _ in 0..12 {
        tokio::task::yield_now().await;
    }
}
async fn step(ms: u64) {
    tokio::time::advance(Duration::from_millis(ms)).await;
    settle().await;
}

#[tokio::test(start_paused = true)]
async fn probe_and_sheet_visibility_rearm_one_poll_schedule_and_manual_refresh_does_not() {
    let f = Fixture::new(true);
    let host = f.client.start_pc_message_connectivity(true).unwrap();
    let view = host.subscribe();
    settle().await;
    assert_eq!(f.api.times(), [0]);
    assert_eq!(view.borrow().counts.as_ref().unwrap().private, Some(7));

    step(1000).await;
    host.observe_request(1, Request::OtherWrapperRejection)
        .await
        .unwrap();
    settle().await;
    step(4999).await;
    assert_eq!(f.api.times(), [0]);
    step(1).await;
    assert_eq!(f.api.times(), [0, 6000]);
    assert!(view.borrow().online);
    step(239_999).await;
    assert_eq!(
        f.api.times(),
        [0, 6000],
        "original 240-second timer was replaced"
    );
    step(1).await;
    assert_eq!(f.api.times(), [0, 6000, 246_000]);

    host.drawer_visible(1, true).await.unwrap();
    settle().await;
    assert_eq!(f.api.times(), [0, 6000, 246_000, 246_000]);
    step(10_000).await;
    host.refresh_counts(1).await.unwrap();
    settle().await;
    assert_eq!(f.api.times().last(), Some(&256_000));
    step(20_000).await;
    assert_eq!(
        f.api.times().last(),
        Some(&276_000),
        "direct refresh must not rearm"
    );
    host.drawer_visible(1, false).await.unwrap();
    settle().await;
    assert_eq!(f.api.times().iter().filter(|t| **t == 276_000).count(), 2);
    step(30_000).await;
    assert_eq!(
        f.api.times().last(),
        Some(&276_000),
        "old visible interval was canceled"
    );
    step(210_000).await;
    // Offline replay of original M/236 with these same host commands.
    assert_eq!(
        f.api.times(),
        [
            0, 6000, 246_000, 246_000, 256_000, 276_000, 276_000, 516_000
        ]
    );
    host.shutdown().await;
    step(240_000).await;
    assert_eq!(f.api.times().last(), Some(&516_000));
}

#[tokio::test(start_paused = true)]
async fn overlapping_rearms_share_one_read_and_generic_failures_do_not_reclassify_network() {
    let f = Fixture::new(true);
    let release = f.api.hold_next();
    let host = f.client.start_pc_message_connectivity(true).unwrap();
    let view = host.subscribe();
    settle().await;
    assert_eq!(f.api.times(), [0]);
    host.drawer_visible(1, true).await.unwrap();
    host.refresh_counts(1).await.unwrap();
    host.observe_request(1, Request::OtherWrapperRejection)
        .await
        .unwrap();
    settle().await;
    step(5000).await;
    assert_eq!(f.api.times(), [0]);
    release.send(()).unwrap();
    settle().await;
    assert!(view.borrow().online);
    assert_eq!(view.borrow().counts.as_ref().unwrap().private, Some(7));
    f.api.code.store(403, Ordering::SeqCst);
    step(29_999).await;
    assert_eq!(f.api.times(), [0]);
    step(1).await;
    assert_eq!(f.api.times(), [0, 35_000]);
    assert!(view.borrow().online);
    assert!(view.borrow().last_count_read_failed);
    assert_eq!(view.borrow().counts.as_ref().unwrap().private, Some(7));
    host.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn account_switch_cancels_a_pending_read_expiry_clears_and_guest_never_uses_old_transport() {
    let f = Fixture::new(false);
    let host = f.client.start_pc_message_connectivity(true).unwrap();
    let mut view = host.subscribe();
    assert!(matches!(
        f.client.start_pc_message_connectivity(true),
        Err(Error::MutationBusy)
    ));
    host.observe_request(0, Request::OtherWrapperRejection)
        .await
        .unwrap();
    settle().await;
    step(5000).await;
    assert!(
        f.api.times().is_empty(),
        "guest probe cannot reuse a retained transport"
    );

    let old = f.api.hold_next();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    settle().await;
    assert_eq!(f.api.times(), [5000]);
    assert_eq!(view.borrow().epoch, "1");
    assert!(view.borrow().counts.is_none());
    let next = Counter::new(70);
    f.session
        .adopt(
            next.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    settle().await;
    assert!(old.send(()).is_err(), "the old read future was dropped");
    assert_eq!(next.times(), [0]);
    assert_eq!(view.borrow().epoch, "2");
    assert_eq!(view.borrow().counts.as_ref().unwrap().private, Some(70));
    host.drawer_visible(1, true).await.unwrap();
    host.refresh_counts(1).await.unwrap();
    host.observe_request(1, Request::OtherWrapperRejection)
        .await
        .unwrap();
    settle().await;
    step(5000).await;
    assert_eq!(
        next.times(),
        [0],
        "stale commands do not rearm or refresh the new account"
    );

    next.code.store(301, Ordering::SeqCst);
    host.refresh_counts(2).await.unwrap();
    settle().await;
    assert_eq!(view.borrow().epoch, "3");
    assert_eq!(
        view.borrow().counts,
        Some(MessageCounts {
            private: Some(0),
            comments: Some(0),
            forwards: Some(0),
            notices: Some(0),
        })
    );
    step(240_000).await;
    assert_eq!(next.times(), [0, 5000], "expiry leaves no count timer");
    view.borrow_and_update();
    host.shutdown().await;
    assert!(view.changed().await.is_err());
    f.client
        .start_pc_message_connectivity(true)
        .unwrap()
        .shutdown()
        .await;
}

#[tokio::test(start_paused = true)]
async fn destroying_the_host_discards_pending_work_and_releases_its_single_executor_slot() {
    let f = Fixture::new(true);
    let response = f.api.hold_next();
    let host = f.client.start_pc_message_connectivity(true).unwrap();
    let mut view = host.subscribe();
    settle().await;
    assert_eq!(f.api.times(), [0]);
    assert!(view.borrow_and_update().count_read_in_flight);
    drop(host);
    assert!(view.changed().await.is_err());
    assert!(response.send(()).is_err());
    step(480_000).await;
    assert_eq!(f.api.times(), [0]);
    f.client
        .start_pc_message_connectivity(true)
        .unwrap()
        .shutdown()
        .await;
}

#[tokio::test(start_paused = true)]
async fn browser_offline_uses_mock_failure_and_recovery_probes_the_same_counter() {
    let f = Fixture::new(true);
    let host = f.client.start_pc_message_connectivity(true).unwrap();
    let view = host.subscribe();
    settle().await;
    step(100).await;
    host.observe_browser(false).await.unwrap();
    settle().await;
    for _ in 0..3 {
        step(5000).await;
    }
    assert!(!view.borrow().online);
    assert_eq!(f.api.times(), [0], "browser-offline probes stay local");

    // M/119 routes the original native false through triggerOnline; the
    // simultaneous browser notification still updates the navigator sample.
    host.observe_native(false).await.unwrap();
    host.observe_browser(true).await.unwrap();
    settle().await;
    step(999).await;
    assert!(!view.borrow().online);
    assert_eq!(f.api.times(), [0]);
    step(1).await;
    assert_eq!(f.api.times(), [0, 16100]);
    assert!(view.borrow().online);
    host.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn production_count_only_mode_ignores_connectivity_and_tracks_read_in_flight() {
    let f = Fixture::new(true);
    let release = f.api.hold_next();
    let host = f.client.start_pc_message_count_polling().unwrap();
    let view = host.subscribe();
    settle().await;
    assert!(view.borrow().count_read_in_flight);
    host.observe_request(1, Request::OtherWrapperRejection)
        .await
        .unwrap();
    settle().await;
    release.send(()).unwrap();
    settle().await;
    assert!(!view.borrow().count_read_in_flight);
    assert_eq!(view.borrow().counts.as_ref().unwrap().private, Some(7));
    step(5001).await;
    assert_eq!(f.api.times(), [0], "production does not run an M/119 probe");
    host.try_drawer_visible(1, true).unwrap();
    settle().await;
    assert_eq!(f.api.times(), [0, 5001]);
    host.shutdown().await;
}
