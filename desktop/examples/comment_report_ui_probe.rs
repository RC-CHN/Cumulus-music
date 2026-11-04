//! Real desktop WebView with a closed EAPI comment-report transport.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/comment_edit.rs"]
mod fixture;
use cumulus_core::client::MusicClient;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    // SAFETY: prepare GTK graphics before creating any runtime threads.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .unwrap();
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let service = Arc::new(fixture::Service::reporting(root.join("fixture.json")).unwrap());
    let client =
        MusicClient::open_with_http_backend(&root.join("state"), None, service.clone()).unwrap();
    let mut context = assets::context();
    let mut config = context.config().app.windows[0].clone();
    config.width = 960.0;
    config.height = 640.0;
    context.config_mut().app.windows[0].create = false;
    let source = std::env::var_os("CUMULUS_UX_SCRIPT").map_or_else(
        || include_str!("comment_report_ui_probe.js").to_owned(),
        |path| fs::read_to_string(path).expect("closed synthetic script"),
    );
    let source = if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
        capture_ack::script(&source)
    } else {
        source
    };
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            app.manage(client);
            app.add_capability(r#"{"identifier":"comment-report-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or_default().to_owned();
                if !(["complete", "failed"].contains(&stage.as_str()) ||
                    stage.starts_with("ux_") && stage.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) {
                    return;
                }
                let state = service.state.lock().unwrap();
                let reports = state.calls.iter().filter(|row| row["path"] == "/api/report/reportcomment").collect::<Vec<_>>();
                value["reportCount"] = json!(reports.len());
                if stage == "complete" || stage == "failed" {
                    value["calls"] = json!(reports);
                }
                fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                let output = output.clone();
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if stage == "complete" || stage == "failed" {
                        handle.exit(if stage == "complete" {0}else{1});
                    } else {
                        capture_ack::acknowledge(&handle, &output, &stage, true).await;
                    }
                });
            });
            tauri::WebviewWindowBuilder::from_config(app, &config)?.data_directory(root.join("webview")).build()?;
            Ok(())
        })
        .on_page_load(move |view, payload| {
            if payload.event() == PageLoadEvent::Finished && view.label() == "main" {
                view.eval(&source).unwrap();
            }
        })
        .run(context).expect("comment report proof");
}
