//! Production UI/IPC with synthetic music HTTP and real official CAPTCHA SDK.
//! Opens and cancels the widget; never solves CAPTCHA or reads a real account.
#[path = "support/auth.rs"]
mod fixture;
use cumulus_core::client::MusicClient;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn main() {
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit new proof root"));
    fs::create_dir(&root).expect("new root");
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let service = Arc::new(fixture::Service::default());
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        app.manage(MusicClient::open_with_http_backend(&root.join("state"), None, service.clone())?);
        app.add_capability(r#"{"identifier":"captcha-proof","windows":["main","captcha-*"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle = app.handle().clone();
        app.listen("captcha-proof", move |event| {
            let mut value: Value = serde_json::from_str(event.payload()).unwrap();
            let stage = value["stage"].as_str().unwrap_or("").to_owned();
            if !["captcha_window_again", "captcha_closed", "captcha_diagnostic", "captcha_form", "captcha_window", "captcha_return", "complete", "failed"].contains(&stage.as_str()) { return; }
            value["musicRequests"] = json!(*service.paths.lock().unwrap());
            value["realAccountUsed"] = json!(false);
            value["realMusicNetworkRequests"] = json!(0);
            value["captchaWindows"] = json!(handle.webview_windows().keys().filter(|s| s.starts_with("captcha-")).count());
            if stage == "complete" && (service.paths.lock().unwrap().get("/api/w/login") != Some(&1) || value["captchaWindows"] != 0) {
                value["ok"] = json!(false);
            }
            fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if stage == "complete" || stage == "failed" { handle.exit(if stage == "complete" && value["ok"] == true { 0 } else { 1 }); }
            else {
                for (_, w) in handle.webview_windows() { let _ = w.eval(format!("window.__captchaAck={};", json!(stage))); }
            }
        });
        Ok(())
    }).on_page_load(|webview, payload| {
        if payload.event() != PageLoadEvent::Finished { return; }
        if webview.label() == "main" { webview.eval(include_str!("captcha_ui_probe.js")).unwrap(); }
        else if webview.label().starts_with("captcha-") { webview.eval(format!("window.__captchaSecond={};", !webview.label().ends_with("-1"))).unwrap();
            webview.eval(include_str!("captcha_window_probe.js")).unwrap(); }
    }).run(tauri::generate_context!()).expect("CAPTCHA probe");
}
