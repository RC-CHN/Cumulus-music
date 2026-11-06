//! Real desktop initialization and the ordinary actor's trusted file-open entry.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/native_local_equalizer/engine.rs"]
mod engine;
#[path = "support/native_local_equalizer/flow.rs"]
mod flow;
#[path = "support/native_local_equalizer/spatial_ui.rs"]
mod spatial_ui;

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
const FLAC_48K: &[u8] = include_bytes!("../../core/tests/fixtures/stereo-pcm-source-48k.flac");
const FLAC_48K_MANIFEST: &str =
    include_str!("../../core/tests/fixtures/stereo-pcm-source-48k.manifest.json");

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
                // The injected API does not have a native credential store.
                // A synthetic 301 would expire this guest on every restore.
                Err(Error::LoginDeferred)
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
    exercise_peq: bool,
    exercise_spatial: bool,
    indexed: bool,
    fixture_source: Value,
    started: AtomicBool,
}

/// Independent reconstruction of the frozen FLAC fixture's signed PCM.
/// The original published manifest pins the payload, without a WAV asset.
fn frozen_wave(manifest: &Value, rate: u32, frames: u32) -> Vec<u8> {
    let payload = frames * 4;
    let mut bytes = Vec::with_capacity(44 + payload as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + payload).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 2, 0]);
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 4).to_le_bytes());
    bytes.extend_from_slice(&[4, 0, 16, 0]);
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&payload.to_le_bytes());
    for i in 0..frames {
        let left = ((i * 97 + (i / 128) * 23) % 8192) as i16 - 4096;
        bytes.extend_from_slice(&left.to_le_bytes());
        bytes.extend_from_slice(&(-(left / 8)).to_le_bytes());
    }
    assert_eq!(
        manifest["pcmSha256"],
        format!("{:x}", Sha256::digest(&bytes[44..]))
    );
    bytes
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
    let case = args.first().and_then(|value| value.to_str());
    let wave = matches!(case, Some("spatial_wav" | "spatial_wav_48k"));
    let indexed = matches!(case, Some("indexed_flac" | "indexed_wrong_digest"));
    assert!(!indexed || cfg!(all(target_os = "linux", target_arch = "x86_64")));
    let rate_48k = matches!(case, Some("spatial_wav_48k" | "spatial_flac_48k"));
    let (expect_graph, exercise_peq, exercise_spatial) = match case {
        Some("trusted" | "indexed_flac") => (true, false, false),
        Some("wrong_digest" | "indexed_wrong_digest") => (false, false, false),
        Some("paused_peq") => (true, true, false),
        Some("spatial_controls" | "spatial_wav" | "spatial_wav_48k" | "spatial_flac_48k") => {
            (true, false, true)
        }
        _ => panic!("invalid proof case"),
    };
    let required = if wave {
        2
    } else {
        4 + usize::from(exercise_peq)
    };
    assert!(
        (required..=required + 1).contains(&args.len()),
        "usage: native_local_equalizer_probe trusted|wrong_digest|indexed_flac|indexed_wrong_digest|spatial_controls|spatial_flac_48k ABS_FLAC SHA256 NEW_ROOT [ABS_MPV]; paused_peq ABS_FLAC SHA256 NEW_ROOT ABS_FILTER [ABS_MPV]; or spatial_wav|spatial_wav_48k NEW_ROOT [ABS_MPV]"
    );
    let decoder = (!wave).then(|| {
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
            // Keep the exact same on-disk library; only its expected digest differs.
            let first = if actual.starts_with('0') { '1' } else { '0' };
            format!("{first}{}", &actual[1..])
        };
        (sdk, digest)
    });
    let root = PathBuf::from(&args[if wave { 1 } else { 3 }]);
    fs::create_dir(&root).expect("new proof root");
    let root = root.canonicalize().expect("proof root");
    let (flac, manifest, rate): (&[u8], &str, u32) = if rate_48k {
        (FLAC_48K, FLAC_48K_MANIFEST, 48_000)
    } else {
        (FLAC, FLAC_MANIFEST, 44_100)
    };
    let manifest: Value = serde_json::from_str(manifest).expect("frozen synthetic manifest");
    assert_eq!(
        manifest["flacSha256"],
        format!("{:x}", Sha256::digest(flac))
    );
    let frames = rate * 2 + 7;
    assert_eq!(manifest["sourceFrames"], frames);
    assert_eq!(manifest["sourceRate"], rate);
    let (kind, bytes) = if wave {
        ("wav", frozen_wave(&manifest, rate, frames))
    } else {
        ("flac", flac.to_vec())
    };
    let media_root = if indexed {
        let media = root.join("media");
        fs::create_dir(&media).expect("new synthetic indexed media root");
        media
    } else {
        root.clone()
    };
    let file = media_root.join(format!("合成 原生均衡器.{kind}"));
    let fixture_source = json!({"kind":kind,"frames":frames,"sampleRate":rate,
        "sha256":format!("{:x}", Sha256::digest(&bytes)),
        "sourcePcmSha256":manifest["pcmSha256"],
        "flacDecoderConfigured":decoder.is_some()});
    fs::write(&file, bytes).expect("synthetic native file");
    let filter = exercise_peq.then(|| {
        let path = PathBuf::from(&args[4]);
        assert!(path.is_absolute() && path.is_file());
        path
    });
    let mpv = args.get(required).map(PathBuf::from);
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
                    let engine = MpvEngine::open(MpvConfig {
                        library: mpv.clone(),
                        null_output: true,
                    })?;
                    let engine = match &decoder {
                        Some((sdk, digest)) => engine.with_local_equalizer_decoder(
                            TrustedFlacRuntime::new(sdk.clone(), digest)?,
                        ),
                        None => engine.with_local_equalizer_pcm_wave(),
                    };
                    let engine = match &filter {
                        Some(path) => engine.with_effects_library(path.clone()),
                        None => engine,
                    };
                    Ok(Box::new(Recorded {
                        engine,
                        evidence: native_evidence.clone(),
                        record_rate: rate_48k,
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
                exercise_peq,
                exercise_spatial,
                indexed,
                fixture_source,
                started: AtomicBool::new(false),
            });
            if exercise_spatial {
                spatial_ui::install(app)?;
            }
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
            if proof.exercise_spatial {
                let script = spatial_ui::script();
                window.eval(script).expect("spatial controls probe script");
                return;
            }
            tauri::async_runtime::spawn(async move {
                let proof = handle.state::<Proof>();
                let client = handle.state::<MusicClient>();
                let initial_epoch = client.account().ok().map(|account| account.epoch);
                let result: Result<Value> = flow::verify(
                    &client,
                    &proof.store,
                    proof.file.clone(),
                    &proof.evidence,
                    proof.expect_graph,
                    proof.exercise_peq,
                    proof.indexed,
                )
                .await;
                let (name, mut report, code) = match result {
                    Ok(report) => ("report.json", report, 0),
                    Err(error) => (
                        "failed.json",
                        json!({"ok":false,"failure":error.to_string(),
                        "initialEpoch":initial_epoch,
                        "finalEpoch":client.account().ok().map(|account| account.epoch),
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
