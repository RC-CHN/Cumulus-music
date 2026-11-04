//! Production Tauri/WebKit homepage using only the synthetic read fixture.
#[path = "support/assets.rs"]
mod assets;
#[path = "../../core/tests/support/discovery_home.rs"]
mod fixture;

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
    let mut context = assets::context();
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
            let fixture = Arc::new(fixture::Fixture::default());
            let session = Arc::new(AccountSession::new(fixture.clone(), Some(registry.store())));
            session.adopt(fixture.clone(), json!({"userId":42,"nickname":"发现测试账号"}), &session.token()?)?;
            app.manage(MusicClient::with_session(session, registry));
            app.add_capability(r#"{"identifier":"discovery-home-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let webview = root.join("webview");
            app.listen("discovery-home-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).expect("probe payload");
                let stage = value["stage"].as_str().expect("probe stage").to_owned();
                match stage.as_str() {
                    "mode" => {
                        let mode = value["mode"].as_u64().filter(|v| *v <= 3).expect("probe fixture mode");
                        fixture.mode.store(mode as u8, Ordering::SeqCst);
                    }
                    "complete" | "failed" | "home" | "dark" | "retry" | "privacy" => {
                        value["paths"] = json!(fixture.calls.lock().unwrap().iter().map(|(path, _, _)| path).collect::<Vec<_>>());
                        fs::write(root.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                        if stage == "complete" || stage == "failed" {
                            handle.exit(i32::from(stage == "failed"));
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
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished && window.label() == "main" {
                let override_script = std::env::var_os("CUMULUS_DISCOVERY_HOME_PROOF_SCRIPT")
                    .map(|path| fs::read_to_string(path).expect("local synthetic probe script"));
                window.eval(override_script.as_deref()
                    .unwrap_or(include_str!("discovery_home_ui_probe.js"))).unwrap();
            }
        })
        .run(context)
        .expect("discovery home proof");
}
