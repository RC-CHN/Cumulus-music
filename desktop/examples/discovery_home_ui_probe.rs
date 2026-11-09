//! Production Tauri/WebKit homepage using only the synthetic read fixture.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "../../core/tests/support/discovery_home.rs"]
mod fixture;
// Reuse the existing exact synthetic CDN -> local WAV -> real libmpv null
// substitution; ordinary discovery still uses its unchanged fixture.
#[path = "support/discovery_recall_audio/mod.rs"]
mod recall_audio;

use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    // SAFETY: first operation, before GTK/runtime or worker creation.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof root"));
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .expect("new private proof root");
    let recall = std::env::var("CUMULUS_DISCOVERY_RECALL_PROOF").as_deref() == Ok("1");
    let mut context = assets::context();
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
            let fixture = Arc::new(fixture::Fixture::default());
            let store = registry.store();
            let session = Arc::new(AccountSession::new(fixture.clone(), Some(store.clone())));
            session.adopt(fixture.clone(), json!({"userId":42,"nickname":"发现测试账号"}), &session.token()?)?;
            let mut client = MusicClient::with_session(session, registry);
            if recall {
                fixture.recall.store(1, Ordering::SeqCst);
                let wave = recall_audio::wave(&root)?;
                client = client.with_audio_factory(move || Ok(Box::new(recall_audio::NativeAudio::open(wave.clone())?)));
                tauri::async_runtime::block_on(client.refresh_pc_presentation())?;
            }
            app.manage(client);
            app.add_capability(r#"{"identifier":"discovery-home-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let webview = root.join("webview");
            app.listen(if recall { "ux-proof" } else { "discovery-home-proof" }, move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).expect("probe payload");
                let stage = value["stage"].as_str().expect("probe stage").to_owned();
                match stage.as_str() {
                    "mode" => {
                        let mode = value["mode"].as_u64().filter(|v| *v <= 6).expect("probe fixture mode");
                        fixture.mode.store(mode as u8, Ordering::SeqCst);
                    }
                    "complete" | "failed" | "home" | "dark" | "seeded_style" | "ranking" | "recent_style" | "retry" | "privacy" | "ux_recall_ready" | "ux_recall_closed" | "ux_recall_playing" => {
                        let calls = fixture.calls.lock().unwrap();
                        value["paths"] = json!(calls.iter().map(|(path, _, _)| path).collect::<Vec<_>>());
                        value["styleCalls"] = json!(calls.iter().filter(|(path, _, _)| matches!(path.as_str(),
                            "/api/homepage/category/daily/song/list" | "/api/homepage/daily/song/config/get"))
                            .collect::<Vec<_>>());
                        if recall {
                            value["guideShown"] = json!(store.get("42", "home-recall-v1", "guide-shown").unwrap() == Some(json!(true)));
                            value["prefillSaved"] = json!(store.get("42", "home-recall-v1", "prefill").unwrap().is_some());
                            value["mediaCalls"] = json!(calls.iter().filter(|(path, _, _)| path == "/api/song/enhance/player/url/v1")
                                .map(|(_, args, _)| serde_json::from_str::<Value>(args["ids"].as_str().unwrap()).unwrap())
                                .collect::<Vec<_>>());
                            value["homepageBatches"] = json!(calls.iter().filter(|(path, _, _)| path == "/api/batch")
                                .map(|(_, args, _)| serde_json::from_str::<Value>(args[fixture::GENERAL].as_str().unwrap()).unwrap())
                                .collect::<Vec<_>>());
                        }
                        fs::write(root.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                        if stage == "complete" || stage == "failed" {
                            handle.exit(i32::from(stage == "failed"));
                        } else if recall && stage.starts_with("ux_") {
                            let handle = handle.clone();
                            let root = root.clone();
                            let handshake = value["captureHandshake"] == true;
                            tauri::async_runtime::spawn(async move {
                                capture_ack::acknowledge(&handle, &root, &stage, handshake).await;
                            });
                        }
                    }
                    _ => {}
                }
            });
            tauri::WebviewWindowBuilder::from_config(app, &window_config)?
                .data_directory(webview)
                .build()?;
            Ok(())
        })
        .on_page_load(move |window, payload| {
            if payload.event() == PageLoadEvent::Finished && window.label() == "main" {
                let override_script = std::env::var_os("CUMULUS_DISCOVERY_HOME_PROOF_SCRIPT")
                    .map(|path| fs::read_to_string(path).expect("local synthetic probe script"));
                let script = override_script.as_deref()
                    .unwrap_or(if recall { include_str!("home_recall_ui_probe.js") } else { include_str!("discovery_home_ui_probe.js") });
                if recall && std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
                    window.eval(capture_ack::script(script)).unwrap();
                } else {
                    window.eval(script).unwrap();
                }
            }
        })
        .run(context)
        .expect("discovery home proof");
}
