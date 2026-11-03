//! A single production WebView/IPC flow against a closed synthetic server.
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/main_flow_fixture.rs"]
mod fixture;

use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    // SAFETY: precedes WebView and worker creation.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof root"));
    fs::create_dir(&root).expect("proof root must not exist");
    let root = root.canonicalize().expect("absolute proof root");
    let out = root.join("first");
    fs::create_dir(&out).unwrap();
    let downloads = root.join("media");
    fs::create_dir(&downloads).unwrap();
    let path = serde_json::to_string(&downloads).unwrap();
    let seed = root.join("synthetic.flac");
    fs::write(&seed, fixture::media::DATA).unwrap();
    let mut context = tauri::generate_context!();
    use tauri::utils::config::Csp;
    let security = &mut context.config_mut().app.security;
    let Some(Csp::Policy(policy)) = security.csp.take() else {
        panic!("expected application CSP")
    };
    security.csp = Some(Csp::Policy(
        policy.replace(" https://*.music.126.net https://*.music.163.com", ""),
    ));
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let fixture = Arc::new(fixture::Service::default());
            let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
            let session = Arc::new(AccountSession::new(fixture.clone(), Some(registry.store())));
            let audio_path = downloads.clone();
            let audio_fixture = fixture.clone();
            let client = MusicClient::with_session(session, registry)
                .with_download_backend(fixture.clone())
                .with_audio_factory(move || Ok(Box::new(fixture::Audio::open(seed.clone(), audio_path.clone(), audio_fixture.clone())?)));
            app.manage(client);
            app.add_capability(r#"{"identifier":"synthetic-main-flow-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let window = app.get_webview_window("main").unwrap();
            window.set_size(tauri::LogicalSize::new(1120.0,760.0))?;
            app.listen("ux-proof", move |event| {
                let Ok(mut value) = serde_json::from_str::<Value>(event.payload()) else { return };
                let Some(stage) = value["stage"].as_str().map(str::to_owned) else { return };
                if stage == "disconnect" {
                    fixture.offline();
                    handle.get_webview_window("main").unwrap().eval("window.__mainFlowOffline=true;").unwrap();
                    return;
                }
                if !["ux_recovered","ux_daily","ux_search","ux_playback","ux_playlist_created","ux_playlist_edited","ux_playlist_cleaned","ux_download_complete","ux_local_offline_playback","complete","failed"].contains(&stage.as_str()) { return; }
                let client = handle.state::<MusicClient>();
                value["authenticated"] = json!(client.account().is_ok_and(|v| v.status == cumulus_core::account::SessionStatus::Authenticated));
                value["fixture"] = fixture.facts();
                fs::write(out.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                if stage == "complete" || stage == "failed" {
                    handle.exit(if stage == "complete" {0} else {1});
                } else if stage.starts_with("ux_") {
                    let handle = handle.clone();
                    let out = out.clone();
                    let handshake = value["captureHandshake"].as_bool() == Some(true);
                    tauri::async_runtime::spawn(async move {
                        capture_ack::acknowledge(&handle, &out, &stage, handshake).await;
                    });
                }
            });
            Ok(())
        })
        .on_page_load(move |webview, payload| {
            if payload.event()==PageLoadEvent::Finished {
                webview.eval(format!("window.__mainFlowDownloadPath={path};")).unwrap();
                let source = include_str!("main_flow_ui_probe.js");
                if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
                    webview.eval(capture_ack::script(source)).unwrap();
                } else {
                    webview.eval(source).unwrap();
                }
            }
        })
        .run(context).expect("synthetic main flow");
}
