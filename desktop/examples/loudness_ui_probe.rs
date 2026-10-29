//! Real settings IPC, local metadata, player actor and native filter with null AO.
#[path = "support/loudness_fixture.rs"]
mod fixture;
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::LocalCommand},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};
struct Offline;
impl Api for Offline {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/w/nuser/account/get");
            Ok(json!({"code":301}))
        })
    }
}
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    let mode = std::env::var("CUMULUS_LOUDNESS_CASE").unwrap_or_else(|_| "normal".into());
    let reopen = mode == "reopen";
    if !reopen {
        fs::create_dir(&root).unwrap();
        fixture::media(&root).unwrap();
    }
    let output = root.join(&mode);
    fs::create_dir(&output).unwrap();
    let setup = root.clone();
    let setup_mode = mode.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        let registry = ProfileRegistry::open(&setup.join("state"), ApiOrigin::Pc)?;
        if setup_mode == "damaged" {
            registry.store().put("local-device", "loudness-preferences-v1", "current", &json!({"enabled":"broken"}))?;
        }
        let session = Arc::new(AccountSession::new(Arc::new(Offline), Some(registry.store())));
        let client = MusicClient::with_session(session, registry);
        let client = cumulus_desktop::configure_native_audio(client, &app.path().resource_dir()?, true);
        tauri::async_runtime::block_on(async {
            client.start_local_library().await?;
            if !reopen { client.local_command(LocalCommand::Watch { path: setup.join("media").to_string_lossy().into_owned() }).await?; }
            Ok::<(), cumulus_core::Error>(())
        })?;
        app.manage(client);
        app.add_capability(r#"{"identifier":"loudness-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle = app.handle().clone();
        app.listen("loudness-proof", move |event| {
            let value: Value = serde_json::from_str(event.payload()).unwrap();
            let stage = value["stage"].as_str().unwrap_or("").to_owned();
            if stage == "resize" {
                let width = value["width"].as_f64().unwrap();
                let height = value["height"].as_f64().unwrap();
                handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(width, height)).unwrap();
                return;
            }
            if !stage.starts_with("loudness_") && !["complete", "failed"].contains(&stage.as_str()) { return; }
            let handle = handle.clone();
            let output = output.clone();
            tauri::async_runtime::spawn(async move {
                let mut data = value.clone();
                if let Some(epoch) = value["epoch"].as_str() {
                    let client = handle.state::<MusicClient>();
                    match client.subscribe_playback(epoch).await.and_then(|s| s.current()) {
                        Ok(snapshot) => {
                            let native = serde_json::to_value(snapshot).unwrap();
                            let expected = &value["expect"];
                            let valid = expected["enabled"].as_bool().is_none_or(|enabled| native["loudness"]["enabled"] == enabled)
                                && expected["status"].as_str().is_none_or(|status| native["loudness"]["status"] == status);
                            data["native"] = native;
                            if !valid { data["native_mismatch"] = json!(true); fs::write(output.join("failed.json"), serde_json::to_vec_pretty(&data).unwrap()).unwrap(); handle.exit(1); return; }
                        }
                        Err(error) => { data["native_error"] = json!(error.to_string()); fs::write(output.join("failed.json"), serde_json::to_vec_pretty(&data).unwrap()).unwrap(); handle.exit(1); return; }
                    }
                }
                fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&data).unwrap()).unwrap();
                if stage == "complete" || stage == "failed" { handle.exit(if stage == "complete" { 0 } else { 1 }); }
                else { handle.get_webview_window("main").unwrap().eval(format!("window.__loudnessAck={};", json!(stage))).unwrap(); }
            });
        });
        Ok(())
    }).on_page_load(move |webview, payload| {
        if payload.event() == PageLoadEvent::Finished {
            webview.eval(format!("window.__loudnessCase={};", json!(mode))).unwrap();
            webview.eval(include_str!("loudness_ui_probe.js")).unwrap();
        }
    }).run(tauri::generate_context!()).expect("loudness UI proof");
}
