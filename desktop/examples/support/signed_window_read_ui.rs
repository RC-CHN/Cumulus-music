//! Run real Tauri, Vue and libmpv with no production network or profile access.
#[path = "signed_window_read_audio.rs"]
mod audio;
#[path = "capture_ack.rs"]
mod capture_ack;
#[cfg(feature = "signed-window-read-proof")]
#[path = "signed_window_read_live.rs"]
pub mod live;
#[cfg(feature = "signed-window-read-proof")]
#[path = "signed_window_read_sandbox.rs"]
mod sandbox;
#[path = "signed_window_read_gate.rs"]
mod signed_window_read_gate;
#[path = "signed_window_read_ui_fixture.rs"]
mod synthetic;

use cumulus_core::{
    account::SessionStatus, client::MusicClient, playback::controller::PlaybackStatus,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

const STAGES: &[&str] = &[
    "ux_account_resources",
    "ux_search_song",
    "ux_playing",
    "ux_paused",
    "ux_resumed",
    "ux_stopped",
];

fn require_network_isolation() -> Result<(), Box<dyn std::error::Error>> {
    let interfaces = fs::read_to_string("/proc/net/dev")?;
    if interfaces
        .lines()
        .skip(2)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, _)| name.trim())
        .collect::<Vec<_>>()
        != ["lo"]
    {
        return Err(
            "synthetic signed window requires a network namespace with only loopback".into(),
        );
    }
    Ok(())
}

fn seal(context: &tauri::Context<tauri::Wry>) -> Value {
    let mut entries: Vec<_> = context.assets().iter().collect();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    assert!(
        entries
            .iter()
            .any(|(key, _)| key.trim_start_matches('/') == "index.html")
    );
    let mut digest = Sha256::new();
    for (key, bytes) in &entries {
        digest.update((key.len() as u64).to_le_bytes());
        digest.update(key.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(Sha256::digest(bytes));
    }
    json!({"embedded":true,"entries":entries.len(),"sha256":format!("{:x}",digest.finalize())})
}

struct ProofContext {
    context: tauri::Context<tauri::Wry>,
    assets: Value,
    window: tauri::utils::config::WindowConfig,
}

fn prepare_context() -> Result<ProofContext, Box<dyn std::error::Error>> {
    // SAFETY: GTK, the WebView, and playback workers have not started.
    unsafe { cumulus_desktop::prepare_graphics() };
    let mut context = tauri::generate_context!();
    let assets = seal(&context);
    let window = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    let security = &mut context.config_mut().app.security;
    let Some(tauri::utils::config::Csp::Policy(policy)) = security.csp.take() else {
        return Err("embedded frontend CSP missing".into());
    };
    security.csp = Some(tauri::utils::config::Csp::Policy(
        policy.replace(" https://*.music.126.net https://*.music.163.com", ""),
    ));
    Ok(ProofContext {
        context,
        assets,
        window,
    })
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 || args[0] != "--synthetic" {
        return Err("only --synthetic is supported; no real credentials or network".into());
    }
    require_network_isolation()?;
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").ok_or("new proof root required")?);
    fs::DirBuilder::new().mode(0o700).create(&root)?;
    let root = root.canonicalize()?;
    let output = root.join("first");
    fs::DirBuilder::new().mode(0o700).create(&output)?;
    let ProofContext {
        context,
        assets,
        window,
    } = prepare_context()?;
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let state = synthetic::State::open(&root)?;
            let fixture = state.fixture.clone();
            let gate = state.gate.clone();
            let facts = Arc::new(audio::NativeFacts::default());
            let (engine_fixture, engine_gate, engine_facts, engine_root) = (
                fixture.clone(), gate.clone(), facts.clone(), root.clone(),
            );
            let client = MusicClient::open_with_request_context(
                &state.state, Some(&state.profile), state.native, Some(state.gate), Some(state.context),
            )?
            .with_official_search_experiment()
            .with_download_backend(Arc::new(synthetic::RejectMedia(fixture.clone())))
            .with_audio_factory(move || Ok(Box::new(audio::Audio::open(
                engine_gate.clone(), engine_fixture.clone(), engine_facts.clone(), engine_root.clone(),
                audio::Expectation::Synthetic { bytes: synthetic::AAC.len() as u64 },
            )?)));
            app.manage(client);
            app.add_capability(r#"{"identifier":"signed-synthetic-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let webview = output.join("webview");
            let progress = Arc::new(tokio::sync::Mutex::new(0));
            app.listen("ux-proof", move |event| {
                let Ok(input) = serde_json::from_str::<Value>(event.payload()) else { return; };
                let (handle, output, gate, fixture, facts, progress, assets) = (
                    handle.clone(), output.clone(), gate.clone(), fixture.clone(), facts.clone(), progress.clone(), assets.clone(),
                );
                tauri::async_runtime::spawn(async move {
                    let Some(stage) = input["stage"].as_str() else { return; };
                    if !STAGES.contains(&stage) && !["complete", "failed"].contains(&stage) { return; }
                    let mut index = progress.lock().await;
                    if stage == "failed" {
                        let message = input["error"].as_str().unwrap_or("renderer proof failed");
                        let allowed = fixture.calls.lock().unwrap().clone();
                        record(&output, "failed", json!({"stage":STAGES.get(*index),"error":message,
                            "approvedCalls":allowed,"gate":gate.facts().await}));
                        handle.exit(1);
                        return;
                    }
                    let gate_facts = gate.facts().await;
                    let client = handle.state::<MusicClient>();
                    let account = client.account().ok();
                    let authenticated = account.as_ref().is_some_and(|a| {
                        a.status == SessionStatus::Authenticated
                            && a.profile_shortcut.as_ref().is_some_and(|p| p.id == synthetic::OWNER)
                    });
                    let player = if *index >= 2 {
                        match account.as_ref() {
                            Some(account) => client.player_snapshot(&account.epoch).await.ok(),
                            None => None,
                        }
                    } else { None };
                    let player_ok = match stage {
                        "ux_playing" | "ux_resumed" => player.as_ref().is_some_and(|p| {
                            p.state == PlaybackStatus::Playing && p.position.is_some_and(|v| v > 0.1)
                                && p.track.as_ref().is_some_and(|t| t.id == synthetic::SONG)
                        }),
                        "ux_paused" => player.as_ref().is_some_and(|p| p.state == PlaybackStatus::Paused),
                        "ux_stopped" | "complete" => player.as_ref().is_some_and(|p| p.state == PlaybackStatus::Idle),
                        _ => true,
                    };
                    let mut calls = fixture.calls.lock().unwrap().clone();
                    let account_first = calls.first() == Some(&"account");
                    let search_after_resources = calls.iter().position(|x| *x == "assignment").is_none_or(|search| {
                        ["playlists", "likes"].iter().all(|route| calls[..search].contains(route))
                    });
                    calls.sort_unstable();
                    let required_calls = ["account", "assignment", "comprehensive", "detail", "likes", "playlists", "source", "tabs"];
                    let all_reads_once = calls.iter().copied().filter(|name| *name != "default").eq(required_calls)
                        && calls.iter().filter(|&&name| name == "default").count() <= 1;
                    let media_reads = fixture.media_reads.load(Ordering::Acquire);
                    let pinned_loads = facts.pinned_loads.load(Ordering::Acquire);
                    let null_clock = facts.null_clock.load(Ordering::Acquire) > 0;
                    let auxiliary = fixture.auxiliary_attempts.load(Ordering::Acquire);
                    let other_media = fixture.other_media_attempts.load(Ordering::Acquire);
                    let expected = STAGES.get(*index).copied().unwrap_or("complete");
                    let ok = stage == expected && authenticated && player_ok && account_first && search_after_resources
                        && gate_facts.expected_account_verified && gate_facts.playlist_count == Some(1)
                        && gate_facts.liked_count == Some(1) && !gate_facts.closed_after_failure
                        && auxiliary == 0 && other_media == 0
                        && (*index < 1 || gate_facts.selected_from_preview && gate_facts.comprehensive_read && gate_facts.search_tab_read)
                        && (*index < 2 || all_reads_once && gate_facts.song_detail_read && gate_facts.media_authorized
                            && gate_facts.media_consumed && media_reads == 1 && pinned_loads == 1 && null_clock);
                    let proof = json!({
                        "ok":ok,"stage":stage,"expectedStage":expected,"syntheticOnly":true,"closedNetwork":true,
                        "productionWebView":true,"physicalAudioOutput":false,"assets":assets,
                        "accountMatched":authenticated,"approvedCalls":calls,"accountFirst":account_first,
                        "searchAfterResources":search_after_resources,"gate":gate_facts,"mediaReads":media_reads,
                        "anonymousPinnedLoads":pinned_loads,"observedLibmpvNullClock":null_clock,
                        "otherMediaAttempts":other_media,"auxiliaryAttempts":auxiliary,
                        "clock":player.as_ref().and_then(|p|p.position),"renderedState":input["playerState"],
                    });
                    if !ok {
                        record(&output, "failed", proof);
                        handle.exit(1);
                    } else {
                        record(&output, stage, proof);
                        *index += 1;
                        if stage == "complete" {
                            handle.exit(0);
                        } else {
                            let screenshot = std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1");
                            if capture_ack::acknowledge(&handle, &output, stage, input["captureHandshake"] == true).await
                                && !screenshot
                                && let Some(window) = handle.get_webview_window("main")
                            {
                                let _ = window.eval(format!("window.__CUMULUS_PROOF_ACK__={};", json!(stage)));
                            }
                        }
                    }
                });
            });
            tauri::WebviewWindowBuilder::from_config(app, &window)?
                .data_directory(webview)
                .inner_size(1120.0, 760.0)
                .build()?;
            Ok(())
        })
        .on_page_load(|webview, page| {
            if page.event() != PageLoadEvent::Finished { return; }
            let script = concat!(
                include_str!("search_online_fixture.js"),
                "\n",
                include_str!("../signed_window_read_ui_probe.js")
            );
            let result = webview.eval(capture_ack::script(script));
            if result.is_err() { webview.app_handle().exit(1); }
        })
        .run(context)?;
    Ok(())
}

fn record(output: &std::path::Path, stage: &str, proof: Value) {
    let temporary = output.join(format!("{stage}.tmp"));
    fs::write(&temporary, serde_json::to_vec_pretty(&proof).unwrap()).unwrap();
    fs::rename(temporary, output.join(format!("{stage}.json"))).unwrap();
}
