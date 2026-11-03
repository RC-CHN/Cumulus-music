//! Production Vue/WebView/IPC; native synthetic HTTP, browser, CAPTCHA provider.
#[path = "support/auth.rs"]
mod auth;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/login_notice_fixture.rs"]
mod fixture;
use cumulus_core::client::MusicClient;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    // SAFETY: before GTK, runtime, or worker initialization.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    fs::create_dir(&root).expect("new proof root");
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let service = Arc::new(fixture::Service::default());
    let browser = service.clone();
    cumulus_desktop::configure(tauri::Builder::default())
        .manage(cumulus_desktop::LoginDocumentBrowser::with_launcher(move |document| browser.open_document(document)))
        .manage(cumulus_desktop::captcha::CaptchaProviderOverride::new(service.clone()))
        .setup(move |app| {
            let profile = cumulus_desktop::native_profile(&root.join("state"))?;
            let windows_declaration = profile.user_agent == cumulus_core::protocol::http::OFFICIAL_WINDOWS_USER_AGENT;
            let native_client_sign = profile.client_sign.is_some();
            let client = MusicClient::open_with_native_profile(&root.join("state"), None, profile.clone(), Some(service.clone()))?;
            let client = cumulus_desktop::renewal::configure(client, &profile);
            app.manage(cumulus_desktop::configure_native_audio(client, &app.path().resource_dir()?, true));
            cumulus_desktop::renewal::start(app.handle());
            app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(1120.0,760.0))?;
            app.add_capability(r#"{"identifier":"login-notice-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let control = service.clone();
            app.listen("login-notice-fixture", move |event| {
                let value: Value = serde_json::from_str(event.payload()).unwrap();
                let id = value["id"].as_u64().unwrap();
                control.command(value["mode"].as_str().unwrap()).unwrap();
                let ack = json!({"id":id,"fixture":control.snapshot()});
                handle.get_webview_window("main").unwrap().eval(format!("window.__loginNoticeFixture={ack};")).unwrap();
            });
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or("").to_owned();
                if stage == "resize" {
                    let scale = value["scale"].as_f64().unwrap();
                    assert!(scale.is_finite() && scale > 0.0 && scale <= 4.0);
                    handle.get_webview_window("main").unwrap().set_size(tauri::PhysicalSize::new(
                        (value["width"].as_f64().unwrap()*scale).round() as u32,
                        (value["height"].as_f64().unwrap()*scale).round() as u32,
                    )).unwrap();
                    return;
                }
                if !stage.starts_with("ux_") && !["complete","failed"].contains(&stage.as_str()) { return; }
                let client = handle.state::<MusicClient>();
                let account = client.account().unwrap();
                value["nativeAccount"] = serde_json::to_value(&account).unwrap();
                value["savedAccounts"] = serde_json::to_value(client.saved_accounts(&account.epoch).unwrap()).unwrap();
                value["fixture"] = service.snapshot();
                value["realMusicRequests"] = json!(0);
                value["nativeCaptchaProvider"] = json!(true);
                value["windowsDeclaration"] = json!(windows_declaration);
                value["nativeClientSignPresent"] = json!(native_client_sign);
                let correct = value["expectNickname"].as_str().is_none_or(|name|account.nickname.as_deref()==Some(name))
                    && (!value["expectGuest"].as_bool().unwrap_or(false) || account.status!=cumulus_core::account::SessionStatus::Authenticated);
                if !correct { value["nativeMismatch"] = json!(true); }
                fs::write(output.join(format!("{}.json",if correct {stage.as_str()} else {"failed"})),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                if !correct || stage == "failed" || stage == "complete" {
                    handle.exit(if correct && stage == "complete" {0} else {1});
                } else if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref()!=Ok("1") {
                    handle.get_webview_window("main").unwrap().eval(format!("window.__CUMULUS_PROOF_ACK__={};",json!(stage))).unwrap();
                } else {
                    let handle=handle.clone(); let output=output.clone();
                    tauri::async_runtime::spawn(async move {
                        capture_ack::acknowledge(&handle,&output,&stage,value["captureHandshake"]==true).await;
                    });
                }
            });
            Ok(())
        })
        .on_page_load(|webview,payload| {
            if payload.event()==PageLoadEvent::Finished && webview.label()=="main" {
                webview.eval(capture_ack::script(include_str!("login_notice_ui_probe.js"))).unwrap();
            }
        })
        .run(tauri::generate_context!())
        .expect("login notice proof");
}
