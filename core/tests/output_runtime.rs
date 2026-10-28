use cumulus_core::{
    Error, Result,
    playback::{engine::*, output::*},
    storage::StateStore,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

struct Native {
    devices: Option<OutputDevices>,
    writes: Vec<String>,
    loads: Vec<(f64, Option<f64>, Duration)>,
    errors: VecDeque<i32>,
    selection_errors: Vec<String>,
    view: EngineSnapshot,
}
struct Engine(Arc<Mutex<Native>>, Arc<AtomicBool>);
impl AudioEngine for Engine {
    fn output_devices(&mut self) -> Result<Option<OutputDevices>> {
        Ok(self.0.lock().unwrap().devices.clone())
    }
    fn set_output_device(&mut self, id: &str) -> Result<()> {
        let mut n = self.0.lock().unwrap();
        n.writes.push(id.into());
        if n.selection_errors.iter().any(|s| s == id) {
            return Err(Error::State("injected selection failure"));
        }
        n.devices.as_mut().unwrap().configured = id.into();
        Ok(())
    }
    fn load(&mut self, r: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        self.0
            .lock()
            .unwrap()
            .loads
            .push((r.start, r.end, r.timeout));
        while self.1.load(Ordering::Acquire) {
            cancel.check()?;
            std::thread::sleep(Duration::from_millis(1));
        }
        cancel.check()?;
        let mut n = self.0.lock().unwrap();
        if let Some(code) = n.errors.pop_front() {
            return Err(Error::Playback {
                operation: "fixture load",
                code,
            });
        }
        n.view.position = Some(r.start);
        n.view.entry_id = Some(7);
        Ok(LoadedMedia {
            entry_id: 7,
            duration: Some(100.),
            codec: None,
        })
    }
    fn pause(&mut self, p: bool) -> Result<()> {
        self.0.lock().unwrap().view.paused = p;
        Ok(())
    }
    fn seek(&mut self, p: f64) -> Result<()> {
        self.0.lock().unwrap().view.position = Some(p);
        Ok(())
    }
    fn volume(&mut self, _: f64) -> Result<()> {
        Ok(())
    }
    fn speed(&mut self, _: f64) -> Result<()> {
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        self.0.lock().unwrap().view.entry_id = None;
        Ok(())
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        let mut n = self.0.lock().unwrap();
        let view = n.view.clone();
        n.view.events.clear();
        Ok(view)
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    runtime: Arc<OutputRuntime>,
    native: Arc<Mutex<Native>>,
    hold: Arc<AtomicBool>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let runtime = OutputRuntime::new(Arc::new(StateStore::open(dir.path()).unwrap()));
        let devices = OutputDevices {
            devices: vec![
                OutputDevice {
                    id: "auto".into(),
                    description: "系统默认".into(),
                },
                OutputDevice {
                    id: "pulse/a".into(),
                    description: "USB 耳机".into(),
                },
            ],
            configured: "auto".into(),
            active_driver: Some("pulse".into()),
        };
        runtime.catalog(Some(devices.clone())).unwrap();
        let native = Arc::new(Mutex::new(Native {
            devices: Some(devices),
            writes: vec![],
            loads: vec![],
            errors: VecDeque::new(),
            selection_errors: vec![],
            view: EngineSnapshot {
                entry_id: Some(7),
                position: Some(25.),
                duration: Some(100.),
                paused: true,
                buffering: false,
                volume: None,
                speed: None,
                audio_output: Some("pulse".into()),
                events: vec![],
            },
        }));
        Self {
            _dir: dir,
            runtime,
            native,
            hold: Arc::new(AtomicBool::new(false)),
        }
    }
    fn engine(&self, kind: OutputKind) -> ManagedOutput<Engine> {
        self.runtime
            .reserve(kind)
            .unwrap()
            .wrap(Box::new(Engine(self.native.clone(), self.hold.clone())))
    }
    fn request() -> LoadRequest {
        LoadRequest {
            source: MediaInput::File("synthetic.wav".into()),
            start: 25.,
            end: Some(70.),
            timeout: Duration::from_secs(3),
        }
    }
    fn phase(&self) -> OutputPhase {
        self.runtime.snapshot().unwrap().engines[0].phase
    }
}
#[test]
fn preference_applies_without_reloading_or_unpausing_and_snapshot_marks_pending() {
    let f = Fixture::new();
    let mut e = f.engine(OutputKind::Audio);
    e.maintain_output().unwrap();
    let p = f.runtime.select(0, "pulse/a").unwrap();
    assert_eq!(f.phase(), OutputPhase::Pending);
    e.maintain_output().unwrap();
    assert_eq!(f.phase(), OutputPhase::Configured);
    assert_eq!(
        f.runtime.snapshot().unwrap().engines[0].revision,
        Some(p.revision)
    );
    let v = e.poll().unwrap();
    assert_eq!(v.entry_id, Some(7));
    assert_eq!(v.position, Some(25.));
    assert!(v.paused);
    assert!(f.native.lock().unwrap().loads.is_empty());
    assert!(matches!(f.runtime.retry(0), Err(Error::StaleResponse)));
    assert!(f.runtime.select(p.revision, "pulse/arbitrary").is_err());
}
#[test]
fn missing_device_falls_back_stickily_until_explicit_retry() {
    let f = Fixture::new();
    let p = f.runtime.select(0, "pulse/a").unwrap();
    let mut e = f.engine(OutputKind::Audio);
    let removed = f
        .native
        .lock()
        .unwrap()
        .devices
        .as_mut()
        .unwrap()
        .devices
        .pop()
        .unwrap();
    e.maintain_output().unwrap();
    assert_eq!(f.phase(), OutputPhase::Fallback);
    assert_eq!(
        f.runtime.preferences().unwrap().unwrap().selection.id(),
        "pulse/a"
    );
    f.native
        .lock()
        .unwrap()
        .devices
        .as_mut()
        .unwrap()
        .devices
        .push(removed);
    e.load(Fixture::request(), &Cancellation::default())
        .unwrap();
    assert_eq!(
        f.native
            .lock()
            .unwrap()
            .devices
            .as_ref()
            .unwrap()
            .configured,
        "auto"
    );
    f.runtime.retry(p.revision).unwrap();
    e.maintain_output().unwrap();
    assert_eq!(
        f.native
            .lock()
            .unwrap()
            .devices
            .as_ref()
            .unwrap()
            .configured,
        "pulse/a"
    );
}
#[test]
fn known_initialization_error_retries_once_with_original_bounds_and_shared_deadline() {
    let f = Fixture::new();
    f.runtime.select(0, "pulse/a").unwrap();
    f.native.lock().unwrap().errors.push_back(-14);
    let mut e = f.engine(OutputKind::Audio);
    e.load(Fixture::request(), &Cancellation::default())
        .unwrap();
    let n = f.native.lock().unwrap();
    assert_eq!(n.loads.len(), 2);
    assert!(n.loads.iter().all(|r| r.0 == 25. && r.1 == Some(70.)));
    assert!(n.loads[1].2 <= n.loads[0].2 && n.loads[0].2 < Duration::from_secs(3));
    assert_eq!(n.writes, ["pulse/a", "auto"]);
}
#[test]
fn recovery_budget_survives_video_engine_replacement_and_retry_resets_it() {
    let f = Fixture::new();
    let p = f.runtime.select(0, "pulse/a").unwrap();
    let mut e = f.engine(OutputKind::Video);
    f.native.lock().unwrap().errors.extend([-14, -14]);
    assert!(
        e.load(Fixture::request(), &Cancellation::default())
            .is_err()
    );
    drop(e);
    let mut next = f.engine(OutputKind::Video);
    f.native.lock().unwrap().errors.push_back(-14);
    assert!(
        next.load(Fixture::request(), &Cancellation::default())
            .is_err()
    );
    assert_eq!(f.native.lock().unwrap().loads.len(), 3);
    f.runtime.retry(p.revision).unwrap();
    f.native.lock().unwrap().errors.push_back(-14);
    next.load(Fixture::request(), &Cancellation::default())
        .unwrap();
    assert_eq!(f.native.lock().unwrap().loads.len(), 5);
}
#[test]
fn decoder_network_and_cancelled_failures_do_not_retry() {
    for code in [-13, -17, -1] {
        let f = Fixture::new();
        let mut e = f.engine(OutputKind::Audio);
        f.native.lock().unwrap().errors.push_back(code);
        assert!(
            e.load(Fixture::request(), &Cancellation::default())
                .is_err()
        );
        assert_eq!(f.native.lock().unwrap().loads.len(), 1);
        assert!(f.native.lock().unwrap().writes.is_empty());
    }
    let f = Fixture::new();
    let mut e = f.engine(OutputKind::Audio);
    let c = Cancellation::default();
    c.cancel();
    assert!(matches!(
        e.load(Fixture::request(), &c),
        Err(Error::Cancelled)
    ));
    assert!(f.native.lock().unwrap().loads.is_empty());
}
#[test]
fn stale_engine_cannot_finish_a_load_or_overwrite_new_instance_status() {
    let f = Fixture::new();
    let mut old = f.engine(OutputKind::Audio);
    f.hold.store(true, Ordering::Release);
    let worker = std::thread::spawn(move || old.load(Fixture::request(), &Cancellation::default()));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while f.native.lock().unwrap().loads.is_empty() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut newest = f.engine(OutputKind::Audio);
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    newest.maintain_output().unwrap();
    assert_eq!(f.phase(), OutputPhase::Configured);
}
#[test]
fn output_failure_event_requests_reload_once_only_for_the_current_entry() {
    let f = Fixture::new();
    let mut e = f.engine(OutputKind::Audio);
    e.maintain_output().unwrap();
    let event = |id| EngineEvent::Ended {
        entry_id: id,
        reason: EndReason::Error,
        error: -14,
    };
    f.native.lock().unwrap().view.events.push(event(99));
    assert_eq!(e.poll().unwrap().events, vec![event(99)]);
    f.native.lock().unwrap().view.events.push(event(7));
    assert_eq!(
        e.poll().unwrap().events,
        vec![EngineEvent::OutputReload { entry_id: 7 }]
    );
    f.native.lock().unwrap().view.events.push(event(7));
    assert_eq!(e.poll().unwrap().events, vec![event(7)]);
    assert_eq!(f.phase(), OutputPhase::Failed);
}
#[test]
fn unsupported_engine_and_failed_selection_are_honest_and_bounded() {
    let f = Fixture::new();
    f.native.lock().unwrap().devices = None;
    let mut e = f.engine(OutputKind::Audio);
    e.maintain_output().unwrap();
    assert_eq!(f.phase(), OutputPhase::Unsupported);
    assert!(f.native.lock().unwrap().writes.is_empty());
    let f = Fixture::new();
    f.runtime.select(0, "pulse/a").unwrap();
    f.native.lock().unwrap().selection_errors = vec!["pulse/a".into(), "auto".into()];
    let mut e = f.engine(OutputKind::Audio);
    e.maintain_output().unwrap();
    assert_eq!(f.phase(), OutputPhase::Failed);
    for _ in 0..3 {
        e.load(Fixture::request(), &Cancellation::default())
            .unwrap();
    }
    assert_eq!(f.native.lock().unwrap().writes, ["pulse/a", "auto"]);
}

#[test]
fn exhausted_initialization_failure_stays_visible_until_success_or_explicit_retry() {
    let f = Fixture::new();
    let mut e = f.engine(OutputKind::Audio);
    f.native.lock().unwrap().errors.extend([-14, -14]);
    assert!(
        e.load(Fixture::request(), &Cancellation::default())
            .is_err()
    );
    assert_eq!(f.phase(), OutputPhase::Failed);
    std::thread::sleep(Duration::from_millis(1100));
    e.maintain_output().unwrap();
    assert_eq!(
        f.phase(),
        OutputPhase::Failed,
        "configuration alone cannot erase a failed initialization"
    );
    e.load(Fixture::request(), &Cancellation::default())
        .unwrap();
    assert_eq!(
        f.phase(),
        OutputPhase::Fallback,
        "a real successful manual load settles initialization"
    );
}
#[test]
fn damaged_settings_use_safe_default_without_implicitly_repairing_storage() {
    let f = Fixture::new();
    f.runtime.select(0, "pulse/a").unwrap();
    let mut e = f.engine(OutputKind::Audio);
    e.maintain_output().unwrap();
    let store = StateStore::open(f._dir.path()).unwrap();
    let raw = serde_json::json!({"selection":"damaged"});
    store
        .put(
            "local-device",
            "audio-output-preferences-v1",
            "current",
            &raw,
        )
        .unwrap();
    e.load(Fixture::request(), &Cancellation::default())
        .unwrap();
    assert_eq!(f.phase(), OutputPhase::Fallback);
    assert!(f.runtime.snapshot().unwrap().damaged);
    assert_eq!(
        f.native
            .lock()
            .unwrap()
            .devices
            .as_ref()
            .unwrap()
            .configured,
        "auto"
    );
    assert_eq!(
        store
            .get("local-device", "audio-output-preferences-v1", "current")
            .unwrap(),
        Some(raw)
    );
    f.runtime.repair().unwrap();
    e.maintain_output().unwrap();
    assert_eq!(f.phase(), OutputPhase::Configured);
}
