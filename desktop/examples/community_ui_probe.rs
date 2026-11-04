//! Actual desktop WebView with a closed synthetic community read transport.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;
#[path = "support/community_fixture.rs"]
mod community_fixture;
#[path = "support/profile_follow_fixture.rs"]
mod profile_follow_fixture;
#[path = "support/profile_notes_fixture.rs"]
mod profile_notes_fixture;
#[path = "support/profile_playlists_fixture.rs"]
mod profile_playlists_fixture;
#[path = "support/profile_podcasts_fixture.rs"]
mod profile_podcasts_fixture;

#[path = "support/community_delete_fixture.rs"]
mod community_delete_fixture;
#[path = "support/community_report_fixture.rs"]
mod community_report_fixture;
#[path = "support/community_topic_fixture.rs"]
mod community_topic_fixture;

use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::http::ApiOrigin,
};
use serde_json::Value;
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    // SAFETY: called before the runtime initializes GTK or creates threads.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("private proof root"));
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .unwrap();
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let mut context = assets::context();
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
            let api = Arc::new(community_fixture::Library::default());
            let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
            app.manage(MusicClient::with_session(session, registry));
            app.add_capability(
                r#"{"identifier":"community-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#,
            )?;
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut payload: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = payload["stage"].as_str().unwrap_or_default().to_owned();
                if stage == "resize" {
                    handle.get_webview_window("main").unwrap()
                        .set_size(tauri::LogicalSize::new(760.0, 540.0)).unwrap();
                    return;
                }
                if stage == "profile-notes-deny" && profile_notes_fixture::Library::enabled() {
                    api.block_profile_notes();
                    handle.get_webview_window("main").unwrap()
                        .eval("window.__profileNotesBlocked = true;").unwrap();
                    return;
                }
                if !(["complete", "failed"].contains(&stage.as_str())
                    || stage.starts_with("ux_") && stage.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
                {
                    return;
                }
                payload["native"] = api.report();
                let handle = handle.clone();
                let output = output.clone();
                tauri::async_runtime::spawn(async move {
                    fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&payload).unwrap()).unwrap();
                    if !capture_ack::acknowledge(&handle, &output, &stage, payload["captureHandshake"] == true).await {
                        return;
                    }
                    if ["complete", "failed"].contains(&stage.as_str()) {
                        handle.exit(if stage == "complete" { 0 } else { 1 });
                    }
                });
            });
            tauri::WebviewWindowBuilder::from_config(app, &window_config)?
                .data_directory(root.join("webview"))
                .build()?;
            Ok(())
        })
        .on_page_load(|view, page| {
            if page.event() != PageLoadEvent::Finished || view.label() != "main" {
                return;
            }
            let script = std::env::var_os("CUMULUS_UX_SCRIPT")
                .map(|path| fs::read_to_string(path).unwrap());
            let script = script.as_deref().unwrap_or(include_str!("community_ui_probe.js"));
            if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
                view.eval(capture_ack::script(script)).unwrap();
            } else {
                view.eval(script).unwrap();
            }
        })
        .run(context)
        .expect("synthetic community WebView");
}
