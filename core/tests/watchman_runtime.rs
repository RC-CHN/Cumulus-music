use cumulus_core::{Error, Result, protocol::api::ApiFuture, storage::StateStore, watchman::*};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Notify, oneshot};
struct Clock(AtomicU64);
impl EntropyClock for Clock {
    fn now_ms(&self) -> Result<u64> {
        Ok(self.0.fetch_add(1, Ordering::SeqCst))
    }
    fn fill(&self, b: &mut [u8]) -> Result<()> {
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i * 37) as u8;
        }
        Ok(())
    }
}
fn config_response() -> Value {
    json!({"code":200,"result":{"v":BUILD_VERSION,"luv":"2.7.5_previous","ivp":300000,"cvk":"cache+key","ass":["ac.dun.163.com","ac.dun.163yun.com"],"conf":encode_text(&json!({"pn":"YD00000558929251","dtvp":86400000,"ret":1,"tto":2000}).to_string()).unwrap()}})
}
fn device(code: i64) -> Value {
    json!([
        code,
        if code == 420 {
            json!("2026-10-27T08:00:00Z")
        } else {
            Value::Null
        },
        "synthetic-tid",
        "synthetic-did",
        null,
        "synthetic-ni"
    ])
}
enum Reply {
    Value(Value),
    Fail,
    Pending(oneshot::Receiver<Result<Value>>),
}
struct Backend {
    config: Mutex<VecDeque<Reply>>,
    device: Mutex<VecDeque<Reply>>,
    behavior: Mutex<VecDeque<Reply>>,
    calls: Mutex<Vec<Request>>,
    notify: Notify,
}
impl Backend {
    fn new(c: Vec<Reply>, d: Vec<Reply>, b: Vec<Reply>) -> Arc<Self> {
        Arc::new(Self {
            config: Mutex::new(c.into()),
            device: Mutex::new(d.into()),
            behavior: Mutex::new(b.into()),
            calls: Mutex::new(vec![]),
            notify: Notify::new(),
        })
    }
    async fn wait_count(&self, n: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let wait = self.notify.notified();
                if self.calls.lock().unwrap().len() >= n {
                    return;
                }
                wait.await;
            }
        })
        .await
        .unwrap();
    }
    fn count(&self, path: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path() == path)
            .count()
    }
}
impl Transport for Backend {
    fn send(&self, r: Request) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            r.form()?;
            let queue = match r.kind {
                RequestKind::Configuration { .. } => &self.config,
                RequestKind::Report {
                    kind: ReportKind::Device,
                    ..
                } => &self.device,
                _ => &self.behavior,
            };
            let reply = queue
                .lock()
                .unwrap()
                .pop_front()
                .expect("unplanned Watchman request");
            self.calls.lock().unwrap().push(r);
            self.notify.notify_waiters();
            match reply {
                Reply::Value(v) => Ok(v),
                Reply::Fail => Err(Error::Transport("synthetic failure".into())),
                Reply::Pending(rx) => rx.await.map_err(|_| Error::Cancelled)?,
            }
        })
    }
}
struct Collector {
    device: AtomicUsize,
    behavior: AtomicUsize,
    invalid: bool,
}
impl Collector {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            device: AtomicUsize::new(0),
            behavior: AtomicUsize::new(0),
            invalid: false,
        })
    }
}
impl Collection for Collector {
    fn collect<'a>(
        &'a self,
        mode: CollectionMode,
        _: &'a Config,
        elapsed: u64,
    ) -> ApiFuture<'a, Observations> {
        Box::pin(async move {
            let fact = |key: &str, value: Value| Observation {
                key: key.into(),
                value,
                failed: false,
            };
            if self.invalid {
                return Ok(Observations {
                    environment: vec![fact("Ma", json!("cannot replace nonce"))],
                    ..Default::default()
                });
            }
            if mode == CollectionMode::Device {
                self.device.fetch_add(1, Ordering::SeqCst);
                Ok(Observations {
                    device: vec![fact(
                        "$b",
                        json!("synthetic environment for mock transport only"),
                    )],
                    environment: vec![fact("hb", json!("synthetic"))],
                    events: vec![],
                })
            } else {
                self.behavior.fetch_add(1, Ordering::SeqCst);
                Ok(Observations {
                    device: vec![],
                    environment: vec![fact("cc", json!(elapsed)), fact("Sa", json!([1120, 760]))],
                    events: vec![fact(
                        "_click",
                        json!([1, 10, 3, 12, 34, "synthetic-button"]),
                    )],
                })
            }
        })
    }
}
fn setup(
    backend: Arc<Backend>,
    collector: Arc<dyn Collection>,
) -> (
    tempfile::TempDir,
    Arc<StateStore>,
    Arc<Clock>,
    Arc<WatchmanRuntime>,
) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(StateStore::open(dir.path()).unwrap());
    let clock = Arc::new(Clock(AtomicU64::new(1000)));
    let runtime = WatchmanRuntime::new(
        db.clone(),
        backend,
        clock.clone(),
        collector,
        "YD00000558929251",
    )
    .unwrap();
    (dir, db, clock, runtime)
}
fn fields_for(build: SdkBuild, packet: &str) -> BTreeMap<u16, Vec<u8>> {
    let bytes = decode_packet_for(build, packet).unwrap();
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < bytes.len() {
        let id = u16::from_be_bytes(bytes[i..i + 2].try_into().unwrap());
        let n = u16::from_be_bytes(bytes[i + 2..i + 4].try_into().unwrap()) as usize;
        assert!(out.insert(id, bytes[i + 4..i + 4 + n].to_vec()).is_none());
        i += 4 + n;
    }
    out
}

#[test]
fn official_public_fields_sequence_omission_and_clock_correction() {
    let oracle: Value =
        serde_json::from_str(include_str!("fixtures/watchman-runtime-official.json")).unwrap();
    for v in oracle["publicFields"].as_array().unwrap() {
        let all = v["all"].as_bool().unwrap();
        let context = CommonFields {
            product: "YD00000558929251",
            business: all.then_some("synthetic-business"),
            nonce: all.then_some("synthetic-nonce"),
            tid: if all { "synthetic-tid" } else { "" },
            did: if all { "synthetic-did" } else { "" },
            track: all.then_some("track-中"),
            local_ms: v["time"].as_u64().unwrap(),
            server_offset_ms: v["offset"].as_i64().unwrap(),
            sequence: (v["previousSeq"].as_u64().unwrap() as u8).saturating_add(1),
        };
        let mut result: Vec<_> = public_fields(
            context,
            hex::decode(v["random"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
        )
        .unwrap()
        .iter()
        .map(hex::encode)
        .collect();
        result.sort();
        assert_eq!(json!(result), v["fields"]);
    }
    for v in oracle["dates"].as_array().unwrap() {
        if v["ms"].is_null() {
            assert!(parse_server_time(&v["value"]).is_err());
        } else {
            assert_eq!(
                parse_server_time(&v["value"]).unwrap(),
                v["ms"].as_i64().unwrap()
            );
        }
    }
}
#[test]
fn official_post_envelope_callback_and_origin_boundaries() {
    let oracle: Value =
        serde_json::from_str(include_str!("fixtures/watchman-runtime-official.json")).unwrap();
    for v in oracle["plans"].as_array().unwrap() {
        let r = Request {
            host: ApiHost::Primary,
            callback: "__wmjsonp_1122334".into(),
            kind: RequestKind::Report {
                build: SdkBuild::Af2952a4,
                kind: if v["url"].as_str().unwrap().ends_with("/d") {
                    ReportKind::Device
                } else {
                    ReportKind::Behavior
                },
                packet: "synthetic+/.P".into(),
            },
        };
        assert_eq!(r.url().unwrap(), v["url"]);
        assert_eq!(r.form().unwrap(), v["body"]);
        assert_eq!(
            r.parse_response(b"__wmjsonp_1122334([200]);").unwrap(),
            json!([200])
        );
        for raw in [
            b"[200]".as_slice(),
            b"wrong([200])",
            b"__wmjsonp_1122334([200]);evil()",
        ] {
            assert!(r.parse_response(raw).is_err());
        }
    }
    for s in [
        "http://ac.dun.163.com",
        "ac.dun.163.com.evil",
        "https://evil@ac.dun.163.com",
        "ac.dun.163.com:443",
        "ac.dun.163.com/path",
    ] {
        assert!(ApiHost::parse(s).is_err());
    }
    assert_eq!(
        ApiHost::parse("https://ac.dun.163.com/").unwrap(),
        ApiHost::Primary
    );
}
#[tokio::test]
async fn configuration_fallback_cache_reopen_304_and_rejected_version_preserve_state() {
    let mut unknown = config_response();
    unknown["result"]["v"] = json!("unreviewed");
    let backend = Backend::new(
        vec![
            Reply::Fail,
            Reply::Value(config_response()),
            Reply::Value(json!({"code":304})),
            Reply::Value(unknown),
        ],
        vec![],
        vec![],
    );
    let (dir, db, clock, _) = setup(backend.clone(), Collector::new());
    let service = ConfigService::new(
        db.clone(),
        backend.clone(),
        clock.clone(),
        "YD00000558929251",
    )
    .unwrap();
    let one = service.load(false).await.unwrap();
    assert_eq!(one.revision(), 1);
    assert_eq!(backend.count("/v2/config/js"), 2);
    let reopened = ConfigService::new(
        Arc::new(StateStore::open(dir.path()).unwrap()),
        backend.clone(),
        clock,
        "YD00000558929251",
    )
    .unwrap();
    assert_eq!(reopened.load(false).await.unwrap().revision(), 1);
    assert_eq!(backend.count("/v2/config/js"), 2);
    let two = reopened.load(true).await.unwrap();
    assert_eq!(two.revision(), 2);
    assert!(two.expires_ms() > one.expires_ms());
    {
        let calls = backend.calls.lock().unwrap();
        assert_eq!(calls[0].host, ApiHost::Primary);
        assert_eq!(calls[1].host, ApiHost::Secondary);
        assert!(calls[2].url().unwrap().contains("cvk=cache%2Bkey"));
    }
    assert!(reopened.load(true).await.is_err());
    assert_eq!(reopened.cached().unwrap().unwrap().revision(), 2);
}
#[tokio::test]
async fn configuration_304_without_cache_and_unknown_host_do_not_install_or_fallback() {
    for response in [
        json!({"code":304}),
        {
            let mut v = config_response();
            v["result"]["ass"] = json!(["unreviewed.example"]);
            v
        },
        json!({"code":500}),
    ] {
        let backend = Backend::new(vec![Reply::Value(response)], vec![], vec![]);
        let (_dir, db, clock, _) = setup(backend.clone(), Collector::new());
        let c = ConfigService::new(db, backend.clone(), clock, "YD00000558929251").unwrap();
        assert!(c.load(false).await.is_err());
        assert!(c.cached().unwrap().is_none());
        assert_eq!(backend.count("/v2/config/js"), 1);
    }
}
#[tokio::test]
async fn config_build_upgrade_is_persisted_and_304_does_not_revert_to_legacy() {
    let mut current = config_response();
    current["result"]["v"] = json!(SdkBuild::B602a5ad7.version());
    current["result"]["luv"] = json!(BUILD_VERSION);
    current["result"]["cvk"] = json!("new-config");
    let backend = Backend::new(
        vec![
            Reply::Value(config_response()),
            Reply::Value(current),
            Reply::Value(json!({"code":304})),
        ],
        vec![],
        vec![],
    );
    let (dir, db, clock, _) = setup(backend.clone(), Collector::new());
    let config =
        ConfigService::new(db, backend.clone(), clock.clone(), "YD00000558929251").unwrap();
    assert_eq!(
        config.load(false).await.unwrap().config().build(),
        SdkBuild::Af2952a4
    );
    assert_eq!(
        config.load(true).await.unwrap().config().build(),
        SdkBuild::B602a5ad7
    );
    drop(config);
    let reopened = ConfigService::new(
        Arc::new(StateStore::open(dir.path()).unwrap()),
        backend.clone(),
        clock,
        "YD00000558929251",
    )
    .unwrap();
    assert_eq!(
        reopened.load(false).await.unwrap().config().build(),
        SdkBuild::B602a5ad7
    );
    assert_eq!(backend.count("/v2/config/js"), 2);
    let validated = reopened.load(true).await.unwrap();
    assert_eq!(validated.config().build(), SdkBuild::B602a5ad7);
    assert_eq!(validated.revision(), 3);
    assert!(
        backend
            .calls
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .url()
            .unwrap()
            .contains("cvk=new-config")
    );
}
#[tokio::test]
async fn runtime_actual_packets_device_correction_behavior_retry_and_token_consistency() {
    for build in [SdkBuild::Af2952a4, SdkBuild::B602a5ad7] {
        let mut config = config_response();
        config["result"]["v"] = json!(build.version());
        let backend = Backend::new(
            vec![Reply::Value(config)],
            vec![Reply::Value(device(420)), Reply::Value(device(200))],
            vec![
                Reply::Value(json!([420, "2026-10-27T09:00:00Z", "tid-new"])),
                Reply::Value(json!([200, null, "tid-final"])),
            ],
        );
        let collector = Collector::new();
        let (_dir, db, _, runtime) = setup(backend.clone(), collector.clone());
        runtime
            .set_track_native(Some("native-track".into()))
            .unwrap();
        let result = runtime.get_token("native-business", None).await.unwrap();
        assert_eq!(result.status(), TokenStatus::BehaviorAccepted);
        let token: Value =
            serde_json::from_str(&decode_text(result.encoded_native()).unwrap()).unwrap();
        assert_eq!(token["d"], "synthetic-did");
        assert!(token.get("t").is_none());
        assert_eq!(collector.device.load(Ordering::SeqCst), 2);
        assert_eq!(collector.behavior.load(Ordering::SeqCst), 1);
        let calls = backend.calls.lock().unwrap();
        assert_eq!(calls.len(), 5);
        let mut last_sequence = 0;
        let mut behavior_events = None;
        let mut old_clock = 0;
        for r in calls.iter().skip(1) {
            let RequestKind::Report {
                kind,
                packet,
                build: request_build,
            } = &r.kind
            else {
                panic!()
            };
            assert_eq!(*request_build, build);
            assert!(
                r.form()
                    .unwrap()
                    .contains(&format!("v={}", build.wire_version()))
            );
            let f = fields_for(build, packet);
            assert_eq!(f[&1], build.version().as_bytes());
            assert_eq!(f[&0], b"200");
            assert_eq!(f[&9], b"YD00000558929251");
            assert_eq!(f[&10], b"native-track");
            assert_eq!(f[&6][0], last_sequence + 1);
            last_sequence += 1;
            let time = u32::from_be_bytes(f[&5].clone().try_into().unwrap());
            if last_sequence > 1 {
                assert!(time >= old_clock);
            }
            old_clock = time;
            if *kind == ReportKind::Behavior {
                assert_eq!(f[&3], token["b"].as_str().unwrap().as_bytes());
                assert_eq!(f[&2], b"native-business");
                assert!(!f.contains_key(&200));
                if let Some(old) = &behavior_events {
                    assert_eq!(&f[&112], old);
                } else {
                    behavior_events = Some(f[&112].clone());
                }
            } else {
                assert!(f.contains_key(&200));
                assert!(!f.contains_key(&112));
            }
        }
        drop(calls);
        let row = db
            .get("device", "watchman-device-v1", "YD00000558929251")
            .unwrap()
            .unwrap();
        assert_eq!(row["version"], build.version());
        assert_eq!(row["revision"], 4);
        assert_eq!(row["tid"], "tid-final");
        runtime.close().unwrap();
    }
}
#[tokio::test]
async fn device_initialization_is_shared_and_caller_cancellation_does_not_cancel_other_waiter() {
    let (tx, rx) = oneshot::channel();
    let backend = Backend::new(
        vec![Reply::Value(config_response())],
        vec![Reply::Pending(rx)],
        vec![Reply::Value(json!([200])), Reply::Value(json!([200]))],
    );
    let (_dir, _, _, runtime) = setup(backend.clone(), Collector::new());
    let first = {
        let r = runtime.clone();
        tokio::spawn(async move { r.get_token("first", None).await })
    };
    backend.wait_count(2).await;
    let second = {
        let r = runtime.clone();
        tokio::spawn(async move { r.get_token("second", None).await })
    };
    tokio::task::yield_now().await;
    first.abort();
    tx.send(Ok(device(200))).unwrap();
    assert_eq!(
        second.await.unwrap().unwrap().status(),
        TokenStatus::BehaviorAccepted
    );
    assert_eq!(backend.count("/v3/d"), 1);
    assert_eq!(backend.count("/v3/b"), 1);
    runtime.close().unwrap();
}
#[tokio::test]
async fn incompatible_sdk_upgrade_waits_for_prior_flight_then_initializes_its_own_device() {
    let (tx, rx) = oneshot::channel();
    let mut new_config = config_response();
    new_config["result"]["v"] = json!(SdkBuild::B602a5ad7.version());
    // luv deliberately excludes the old build; that flight cannot satisfy the
    // new caller, even though both initializations share one product namespace.
    let backend = Backend::new(
        vec![Reply::Value(config_response()), Reply::Value(new_config)],
        vec![Reply::Pending(rx), Reply::Value(device(200))],
        vec![Reply::Value(json!([200])), Reply::Value(json!([200]))],
    );
    let (_dir, db, clock, runtime) = setup(backend.clone(), Collector::new());
    let a = {
        let r = runtime.clone();
        tokio::spawn(async move { r.get_token("old", None).await })
    };
    backend.wait_count(2).await;
    clock.0.store(400_000, Ordering::SeqCst);
    let b = {
        let r = runtime.clone();
        tokio::spawn(async move { r.get_token("new", None).await })
    };
    backend.wait_count(3).await;
    tx.send(Ok(device(200))).unwrap();
    assert_eq!(
        a.await.unwrap().unwrap().status(),
        TokenStatus::BehaviorAccepted
    );
    assert_eq!(
        b.await.unwrap().unwrap().status(),
        TokenStatus::BehaviorAccepted
    );
    assert_eq!(backend.count("/v3/d"), 2);
    let calls = backend.calls.lock().unwrap();
    let new_device = calls
        .iter()
        .position(|r| {
            matches!(
                r.kind,
                RequestKind::Report {
                    build: SdkBuild::B602a5ad7,
                    kind: ReportKind::Device,
                    ..
                }
            )
        })
        .unwrap();
    let new_behavior = calls
        .iter()
        .position(|r| {
            matches!(
                r.kind,
                RequestKind::Report {
                    build: SdkBuild::B602a5ad7,
                    kind: ReportKind::Behavior,
                    ..
                }
            )
        })
        .unwrap();
    assert!(new_device < new_behavior);
    assert_eq!(
        db.get("device", "watchman-device-v1", "YD00000558929251")
            .unwrap()
            .unwrap()["version"],
        SdkBuild::B602a5ad7.version()
    );
    runtime.close().unwrap();
}
#[tokio::test]
async fn delayed_old_caller_cannot_report_with_new_incompatible_device_record() {
    use std::{
        future::Future,
        task::{Context, Waker},
    };
    let (tx, rx) = oneshot::channel();
    let mut current = config_response();
    current["result"]["v"] = json!(SdkBuild::B602a5ad7.version());
    let backend = Backend::new(
        vec![Reply::Value(config_response()), Reply::Value(current)],
        vec![Reply::Pending(rx), Reply::Value(device(200))],
        vec![Reply::Value(json!([200])), Reply::Value(json!([200]))],
    );
    let (_dir, _, clock, runtime) = setup(backend.clone(), Collector::new());
    // Poll the first caller once, then leave its continuation unscheduled until
    // the second caller has replaced the shared device record with a new build.
    let mut old = Box::pin(runtime.get_token("old", None));
    assert!(
        old.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    backend.wait_count(2).await;
    clock.0.store(400_000, Ordering::SeqCst);
    let new = {
        let r = runtime.clone();
        tokio::spawn(async move { r.get_token("new", None).await })
    };
    backend.wait_count(3).await;
    tx.send(Ok(device(200))).unwrap();
    assert_eq!(
        new.await.unwrap().unwrap().status(),
        TokenStatus::BehaviorAccepted
    );
    assert!(matches!(old.await, Err(Error::StaleResponse)));
    assert_eq!(backend.count("/v3/b"), 1);
    runtime.close().unwrap();
}
#[tokio::test]
async fn shared_device_failure_does_not_start_one_retry_per_waiter() {
    let (tx, rx) = oneshot::channel();
    let backend = Backend::new(
        vec![Reply::Value(config_response())],
        vec![Reply::Pending(rx)],
        vec![],
    );
    let (_dir, _, _, runtime) = setup(backend.clone(), Collector::new());
    let first = {
        let r = runtime.clone();
        tokio::spawn(async move { r.get_token("first", None).await })
    };
    backend.wait_count(2).await;
    let second = {
        let r = runtime.clone();
        tokio::spawn(async move { r.get_token("second", None).await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    tx.send(Ok(json!([500]))).unwrap();
    assert!(first.await.unwrap().is_err());
    assert!(second.await.unwrap().is_err());
    assert_eq!(backend.count("/v3/d"), 1);
    assert_eq!(backend.count("/v3/b"), 0);
    runtime.close().unwrap();
}
#[tokio::test]
async fn behavior_fallback_hosts_timeout_illegal_and_420_unchecked_remain_distinct() {
    for (replies, status) in [
        (
            vec![Reply::Fail, Reply::Value(json!([200]))],
            Some(TokenStatus::BehaviorAccepted),
        ),
        (
            vec![Reply::Value(json!([420, 0])), Reply::Fail, Reply::Fail],
            Some(TokenStatus::UncheckedRetry),
        ),
        (
            vec![Reply::Value(json!([500]))],
            Some(TokenStatus::FailureFallback),
        ),
        (vec![Reply::Value(json!([470]))], None),
    ] {
        let backend = Backend::new(
            vec![Reply::Value(config_response())],
            vec![Reply::Value(device(200))],
            replies,
        );
        let (_dir, _, _, runtime) = setup(backend.clone(), Collector::new());
        let result = runtime.get_token("biz", None).await;
        if let Some(status) = status {
            assert_eq!(result.unwrap().status(), status);
        } else {
            assert!(result.is_err());
            assert_eq!(backend.count("/v3/b"), 1);
        }
        runtime.close().unwrap();
    }
    let (tx, rx) = oneshot::channel();
    let backend = Backend::new(
        vec![Reply::Value(config_response())],
        vec![Reply::Value(device(200))],
        vec![Reply::Pending(rx)],
    );
    let (_dir, _, _, runtime) = setup(backend.clone(), Collector::new());
    let result = runtime.get_token("biz", Some(5)).await.unwrap();
    assert_eq!(result.status(), TokenStatus::TimeoutFallback);
    assert!(tx.send(Ok(json!([200]))).is_err());
    assert_eq!(backend.count("/v3/b"), 1);
    runtime.close().unwrap();
}
#[tokio::test]
async fn close_cancels_inflight_reports_and_prevents_late_device_commit_or_new_calls() {
    let (tx, rx) = oneshot::channel();
    let backend = Backend::new(
        vec![Reply::Value(config_response())],
        vec![Reply::Pending(rx)],
        vec![],
    );
    let (_dir, db, _, runtime) = setup(backend.clone(), Collector::new());
    let task = {
        let r = runtime.clone();
        tokio::spawn(async move { r.get_token("biz", None).await })
    };
    backend.wait_count(2).await;
    runtime.close().unwrap();
    let error = match task.await.unwrap() {
        Err(error) => error,
        Ok(_) => panic!("closed runtime returned a token"),
    };
    assert!(matches!(error.root(), Error::Cancelled));
    tokio::task::yield_now().await;
    assert!(tx.send(Ok(device(200))).is_err());
    assert!(
        db.get("device", "watchman-device-v1", "YD00000558929251")
            .unwrap()
            .is_none()
    );
    assert!(runtime.get_token("biz", None).await.is_err());
    assert_eq!(backend.calls.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn persisted_device_reopen_nd_info_and_due_refresh_use_same_native_state() {
    let backend = Backend::new(
        vec![
            Reply::Value(config_response()),
            Reply::Value(json!({"code":304})),
        ],
        vec![Reply::Value(device(200)), Reply::Value(device(200))],
        vec![Reply::Value(json!([200]))],
    );
    let (dir, db, clock, runtime) = setup(backend.clone(), Collector::new());
    runtime.get_token("biz", None).await.unwrap();
    runtime.close().unwrap();
    let reopened = WatchmanRuntime::new(
        Arc::new(StateStore::open(dir.path()).unwrap()),
        backend.clone(),
        clock.clone(),
        Collector::new(),
        "YD00000558929251",
    )
    .unwrap();
    let nd: Value =
        serde_json::from_str(&decode_text(&reopened.nd_info().await.unwrap()).unwrap()).unwrap();
    assert_eq!(nd["d"], "synthetic-did");
    assert_eq!(backend.count("/v3/d"), 1);
    let row = db
        .get("device", "watchman-device-v1", "YD00000558929251")
        .unwrap()
        .unwrap();
    clock.0.store(
        row["expires_ms"].as_u64().unwrap() - 299000,
        Ordering::SeqCst,
    );
    reopened.refresh_device_if_due().await.unwrap();
    assert_eq!(backend.count("/v3/d"), 2);
    assert_eq!(backend.count("/v2/config/js"), 2);
    reopened.close().unwrap();
}
#[tokio::test]
async fn collector_cannot_replace_public_fields_or_submit_events_in_device_report() {
    let collector = Arc::new(Collector {
        device: AtomicUsize::new(0),
        behavior: AtomicUsize::new(0),
        invalid: true,
    });
    let backend = Backend::new(vec![Reply::Value(config_response())], vec![], vec![]);
    let (_dir, _, _, runtime) = setup(backend.clone(), collector);
    assert!(runtime.get_token("biz", None).await.is_err());
    assert_eq!(backend.count("/v3/d"), 0);
    runtime.close().unwrap();
}
#[tokio::test]
async fn periodic_task_initialization_and_stop_do_not_spawn_an_extra_request() {
    let backend = Backend::new(
        vec![Reply::Value(config_response())],
        vec![Reply::Value(device(200))],
        vec![],
    );
    let (_dir, _, _, runtime) = setup(backend.clone(), Collector::new());
    let refresh = runtime.start_refresh();
    backend.wait_count(2).await;
    refresh.stop().await.unwrap();
    runtime.close().unwrap();
    assert_eq!(backend.calls.lock().unwrap().len(), 2);
}
