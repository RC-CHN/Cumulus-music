//! Real desktop initialization and the ordinary actor's trusted file-open entry.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/native_local_equalizer/engine.rs"]
mod engine;
#[path = "support/native_local_equalizer/flow.rs"]
mod flow;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    playback::engine::{MpvConfig, MpvEngine, TrustedFlacRuntime},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
    storage::StateStore,
};
use engine::{Evidence, Recorded};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tauri::{Manager, webview::PageLoadEvent};

const FLAC: &[u8] = include_bytes!("../../core/tests/fixtures/stereo-pcm-source.flac");
const FLAC_MANIFEST: &str =
    include_str!("../../core/tests/fixtures/stereo-pcm-source.manifest.json");

#[derive(Default)]
struct Offline {
    account_reads: AtomicUsize,
    denied_other_requests: AtomicUsize,
}
impl Api for Offline {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if path == "/api/w/nuser/account/get" {
                self.account_reads.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"code":301}))
            } else {
                self.denied_other_requests.fetch_add(1, Ordering::SeqCst);
                Err(Error::Transport(
                    "sealed local file probe has no service transport".into(),
                ))
            }
        })
    }
}

struct Proof {
    root: PathBuf,
    file: PathBuf,
    store: Arc<StateStore>,
    evidence: Evidence,
    offline: Arc<Offline>,
    expect_graph: bool,
    started: AtomicBool,
}

#[cfg(target_os = "linux")]
fn locale(category: libc::c_int) -> String {
    // SAFETY: inspected during process/GUI startup; the C-owned value is copied.
    unsafe {
        let value = libc::setlocale(category, std::ptr::null());
        assert!(!value.is_null());
        std::ffi::CStr::from_ptr(value)
            .to_string_lossy()
            .into_owned()
    }
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    assert!(
        matches!(args.len(), 4 | 5),
        "usage: native_local_equalizer_probe trusted|wrong_digest ABS_FLAC SHA256 NEW_ROOT [ABS_MPV]"
    );
    let expect_graph = match args[0].to_str() {
        Some("trusted") => true,
        Some("wrong_digest") => false,
        _ => panic!("invalid proof case"),
    };
    let sdk = PathBuf::from(&args[1]);
    assert!(sdk.is_absolute() && sdk.is_file());
    let digest = args[2].to_str().expect("package decoder SHA256");
    let actual = format!(
        "{:x}",
        Sha256::digest(fs::read(&sdk).expect("staged FLAC library"))
    );
    assert_eq!(digest, actual, "wrong requested package library");
    let digest = if expect_graph {
        actual
    } else {
        // Keep the exact same on-disk ELF; only the expected digest differs.
        let first = if actual.starts_with('0') { '1' } else { '0' };
        format!("{first}{}", &actual[1..])
    };
    let root = PathBuf::from(&args[3]);
    fs::create_dir(&root).expect("new proof root");
    let root = root.canonicalize().expect("proof root");
    let file = root.join("合成 原生均衡器.flac");
    let manifest: Value = serde_json::from_str(FLAC_MANIFEST).expect("frozen synthetic manifest");
    assert_eq!(
        manifest["flacSha256"],
        format!("{:x}", Sha256::digest(FLAC))
    );
    assert_eq!(manifest["sourceFrames"], 88_207);
    fs::write(&file, FLAC).expect("synthetic native file");
    let mpv = args.get(4).map(PathBuf::from);
    assert!(
        mpv.as_ref()
            .is_none_or(|path| path.is_absolute() && path.is_file())
    );

    #[cfg(target_os = "linux")]
    let original_messages = {
        // SAFETY: test-only process startup before worker or GTK creation.
        assert!(!unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) }.is_null());
        assert!(!matches!(locale(libc::LC_NUMERIC).as_str(), "C" | "POSIX"));
        locale(libc::LC_MESSAGES)
    };
    // SAFETY: precedes GUI and native workers.
    unsafe { cumulus_desktop::prepare_graphics() };
    let mut context = assets::context();
    let window = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;

    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
            let store = registry.store();
            let offline = Arc::new(Offline::default());
            let session = Arc::new(AccountSession::new(offline.clone(), Some(store.clone())));
            let evidence = Evidence::default();
            let native_evidence = evidence.clone();
            let client =
                MusicClient::with_session(session, registry).with_audio_factory(move || {
                    let runtime = TrustedFlacRuntime::new(sdk.clone(), &digest)?;
                    let engine = MpvEngine::open(MpvConfig {
                        library: mpv.clone(),
                        null_output: true,
                    })?
                    .with_local_equalizer_decoder(runtime);
                    Ok(Box::new(Recorded {
                        engine,
                        evidence: native_evidence.clone(),
                    }))
                });
            tauri::async_runtime::block_on(client.start_local_library())?;
            app.manage(client);
            app.manage(Proof {
                root: root.clone(),
                file,
                store,
                evidence,
                offline,
                expect_graph,
                started: AtomicBool::new(false),
            });
            tauri::WebviewWindowBuilder::from_config(app, &window)?
                .data_directory(root.join("webview"))
                .build()?;
            #[cfg(target_os = "linux")]
            {
                assert_eq!(locale(libc::LC_NUMERIC), "C");
                assert_eq!(locale(libc::LC_MESSAGES), original_messages);
            }
            Ok(())
        })
        .on_page_load(|window, event| {
            if window.label() != "main" || event.event() != PageLoadEvent::Finished {
                return;
            }
            let handle = window.app_handle().clone();
            let proof = handle.state::<Proof>();
            if proof.started.swap(true, Ordering::SeqCst) {
                return;
            }
            tauri::async_runtime::spawn(async move {
                let proof = handle.state::<Proof>();
                let client = handle.state::<MusicClient>();
                let result: Result<Value> = flow::verify(
                    &client,
                    &proof.store,
                    proof.file.clone(),
                    &proof.evidence,
                    proof.expect_graph,
                )
                .await;
                let (name, mut report, code) = match result {
                    Ok(report) => ("report.json", report, 0),
                    Err(error) => (
                        "failed.json",
                        json!({"ok":false,"failure":error.to_string(),
                        "native":&*proof.evidence.lock().unwrap()}),
                        1,
                    ),
                };
                report["webviewFinished"] = json!(true);
                report["realMusicRequests"] = json!(0);
                report["realAccountWrites"] = json!(0);
                report["syntheticAccountReads"] =
                    json!(proof.offline.account_reads.load(Ordering::SeqCst));
                report["deniedOtherRequests"] =
                    json!(proof.offline.denied_other_requests.load(Ordering::SeqCst));
                #[cfg(target_os = "linux")]
                {
                    report["numericLocaleIsC"] = json!(true);
                    report["messagesLocalePreserved"] = json!(true);
                }
                fs::write(
                    proof.root.join(name),
                    serde_json::to_vec_pretty(&report).unwrap(),
                )
                .unwrap();
                handle.exit(code);
            });
        })
        .run(context)
        .expect("ordinary local EQ desktop proof");
}
