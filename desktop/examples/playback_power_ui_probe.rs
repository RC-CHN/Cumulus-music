//! Production Vue/IPC/SQLite and libmpv/null. Only the experiment API, media
//! bytes, account and final OS power sink are synthetic. Never opens a Win32
//! power sink or touches the host's device settings.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;

use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    playback::{
        PlaybackQueue,
        keep_awake::{PlaybackPowerRequest, PlaybackPowerSink},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

const OWNER: &str = "local-device";
const KIND: &str = "playback-power-v1";
const KEY: &str = "enabled";

struct Library {
    media: relay_fixture::Library,
    group: &'static str,
    experiment_reads: AtomicUsize,
}

impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        self.media.request(path, payload, cache)
    }

    fn playback_power_experiment(&self) -> ApiFuture<'_, Option<PcExperimentGroups>> {
        Box::pin(async move {
            self.experiment_reads.fetch_add(1, Ordering::Relaxed);
            PcExperimentGroups::refreshed(&json!({
                "data":[{"expName":"PH-PC-keepAwake","expGroupName":self.group}]
            }))
            .map(Some)
        })
    }
}

#[derive(Default)]
struct SealedSink(Mutex<Vec<PlaybackPowerRequest>>);

impl PlaybackPowerSink for SealedSink {
    fn submit(&self, request: PlaybackPowerRequest) {
        self.0.lock().unwrap().push(request);
    }
}

impl SealedSink {
    fn observations(&self) -> Vec<&'static str> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|request| match request {
                PlaybackPowerRequest::Clear => "clear",
                PlaybackPowerRequest::KeepSystemAndDisplayAwake => "keep",
            })
            .collect()
    }
}

#[cfg(target_os = "linux")]
fn locale(category: libc::c_int) -> String {
    // SAFETY: null queries the current category and does not alter it. This
    // test runs after the production desktop setup adjusted LC_NUMERIC.
    let name = unsafe { libc::setlocale(category, std::ptr::null()) };
    assert!(!name.is_null());
    // SAFETY: setlocale returns the category name as a C string. Copy it now.
    unsafe { std::ffi::CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned()
}

fn main() {
    // SAFETY: before GTK/runtime or any worker thread.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    let phase = std::env::var("CUMULUS_POWER_PROOF_PHASE").expect("explicit proof phase");
    let group = match phase.as_str() {
        "initial" | "reopen" | "unsupported" => "t1",
        "control" => "t2",
        _ => panic!("unknown proof phase"),
    };
    let output = root.join(&phase);
    fs::create_dir(&output).expect("new isolated evidence directory");
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    if phase == "initial" {
        assert!(store.get("2", "playback", "queue").unwrap().is_none());
        assert!(store.get(OWNER, KIND, KEY).unwrap().is_none());
        let mut queue = PlaybackQueue::new(Some(store.clone()), "2").unwrap();
        queue
            .replace(
                vec![json!({"id":2,"dt":30000,"song":{"id":2,"name":"合成防休眠验证","dt":30000}})],
                0,
                json!({"kind":"fixture"}),
            )
            .unwrap();
        store
            .put(
                "2",
                "playback",
                "position",
                &json!({
                    "entry_id":queue.selected().unwrap().entry_id,
                    "seconds":4.0,"state":"paused","timeline_version":1
                }),
            )
            .unwrap();
    } else {
        assert_eq!(store.get(OWNER, KIND, KEY).unwrap(), Some(json!(true)));
    }
    let library = Arc::new(Library {
        media: relay_fixture::Library::default(),
        group,
        experiment_reads: AtomicUsize::new(0),
    });
    library.media.mode("empty");
    let sink = Arc::new(SealedSink::default());
    let session = Arc::new(AccountSession::new(library.clone(), Some(store.clone())));
    let wave = relay_fixture::wave(&root).unwrap();
    let client = MusicClient::with_session(session, registry)
        .with_audio_factory(move || Ok(Box::new(relay_fixture::NativeAudio::open(wave.clone())?)));
    let client = if phase == "unsupported" {
        client
    } else {
        client.with_playback_power_sink(sink.clone())
    };
    let mut context = assets::context();
    let config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    let page_phase = phase.clone();
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            fs::write(output.join("locale.json"), serde_json::to_vec(&json!({
                "numeric":locale(libc::LC_NUMERIC),"messages":locale(libc::LC_MESSAGES),
                "characters":locale(libc::LC_CTYPE)
            }))?)?;
            app.manage(client);
            app.add_capability(r#"{"identifier":"playback-power-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let data = root.join("state/core.sqlite3");
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or("").to_owned();
                if stage == "resize" {
                    handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(value["width"].as_f64().unwrap(), value["height"].as_f64().unwrap())).unwrap();
                    return;
                }
                if ["reject-write", "allow-write"].contains(&stage.as_str()) {
                    assert_eq!(phase, "initial");
                    let connection = rusqlite::Connection::open(&data).unwrap();
                    connection.execute_batch(if stage == "reject-write" {
                        "CREATE TRIGGER reject_power BEFORE INSERT ON state WHEN NEW.owner='local-device' AND NEW.kind='playback-power-v1' AND NEW.key='enabled' BEGIN SELECT RAISE(ABORT,'synthetic power setting failure'); END;"
                    } else {
                        "DROP TRIGGER reject_power;"
                    }).unwrap();
                    handle.get_webview_window("main").unwrap().eval(format!(
                        "window.__powerFixtureAck={};", json!(stage)
                    )).unwrap();
                    return;
                }
                if stage == "inspect-power" {
                    let sequence = value["sequence"].as_u64().unwrap();
                    let trace = json!(sink.observations());
                    handle.get_webview_window("main").unwrap().eval(format!(
                        "window.__powerProbeReplies[{sequence}]={trace};"
                    )).unwrap();
                    return;
                }
                if !(["complete", "failed"].contains(&stage.as_str()) ||
                    stage.starts_with("ux_") && stage.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')) {
                    return;
                }
                let handle = handle.clone();
                let output = output.clone();
                let sink = sink.clone();
                let library = library.clone();
                let store = store.clone();
                tauri::async_runtime::spawn(async move {
                    let client = handle.state::<MusicClient>();
                    let epoch = client.account().unwrap().epoch;
                    value["nativePlayer"] = json!(client.player_snapshot(&epoch).await.unwrap());
                    value["powerRequests"] = json!(sink.observations());
                    value["devicePreference"] = json!(store.get(OWNER, KIND, KEY).unwrap());
                    value["experimentReads"] = json!(library.experiment_reads.load(Ordering::Relaxed));
                    fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                    if stage == "complete" || stage == "failed" {
                        handle.exit(if stage == "complete" { 0 } else { 1 });
                    } else {
                        capture_ack::acknowledge(&handle, &output, &stage, value["captureHandshake"] == true).await;
                    }
                });
            });
            tauri::WebviewWindowBuilder::from_config(app, &config)?
                .data_directory(root.join("webview")).build()?;
            Ok(())
        })
        .on_page_load(move |view, payload| {
            if payload.event() == PageLoadEvent::Finished && view.label() == "main" {
                view.eval(format!("window.__CUMULUS_POWER_PROOF_PHASE__={};", json!(page_phase))).unwrap();
                let script = include_str!("playback_power_ui_probe.js");
                if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
                    view.eval(capture_ack::script(script)).unwrap();
                } else {
                    view.eval(script).unwrap();
                }
            }
        })
        .run(context).expect("playback power proof");
}
