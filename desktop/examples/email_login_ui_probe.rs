//! Actual production email verification UI/IPC; synthetic service only.
#[path = "support/auth.rs"]
mod auth;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/email_login_fixture.rs"]
mod fixture;
use cumulus_core::client::MusicClient;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn main() {
    // SAFETY: process entry, before GUI/runtime initialization.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    fs::create_dir(&root).unwrap();
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let service = Arc::new(fixture::Service::default());
    let legacy = std::env::var_os("CUMULUS_PROOF_LEGACY_EMAIL").is_some();
    let active_epoch = Arc::new(Mutex::new(None::<String>));
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        let client=MusicClient::open_with_http_backend(&root.join("state"),None,service.clone())?.with_login_device_name("Synthetic PC")?;
        let client=if legacy {client}else{client.with_official_login_policy()};
        app.manage(cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true));
        app.add_capability(r#"{"identifier":"email-login-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();let backend=service.clone();
        app.listen("email-login-fixture",move|event|{
            let v:Value=serde_json::from_str(event.payload()).unwrap();backend.command(v["mode"].as_str().unwrap());
            handle.get_webview_window("main").unwrap().eval(format!("window.__emailFixtureAck={};window.__emailFixtureSnapshot={};",v["id"],backend.snapshot())).unwrap();
        });
        let handle=app.handle().clone();
        app.listen("ux-proof",move|event|{
            let mut value:Value=serde_json::from_str(event.payload()).unwrap();let stage=value["stage"].as_str().unwrap_or("").to_owned();
            if stage=="resize" {let ratio=value["scale"].as_f64().unwrap();handle.get_webview_window("main").unwrap().set_size(tauri::PhysicalSize::new((value["width"].as_f64().unwrap()*ratio).round()as u32,(value["height"].as_f64().unwrap()*ratio).round()as u32)).unwrap();return;}
            if !stage.starts_with("ux_") && !["complete","failed"].contains(&stage.as_str()){return;}
            let client=handle.state::<MusicClient>();let account=client.account().unwrap();
            if value["rememberEpoch"]==true {*active_epoch.lock().unwrap()=Some(account.epoch.clone());}
            let correct=(!value["expectGuest"].as_bool().unwrap_or(false)||account.status!=cumulus_core::account::SessionStatus::Authenticated)
                && value["expectNickname"].as_str().is_none_or(|s|account.nickname.as_deref()==Some(s))
                && (value["preserveEpoch"]!=true||active_epoch.lock().unwrap().as_ref()==Some(&account.epoch));
            value["nativeAccount"]=serde_json::to_value(account).unwrap();value["fixture"]=service.snapshot();value["realMusicRequests"]=json!(0);
            if !correct {value["nativeMismatch"]=json!(true);}
            fs::write(output.join(format!("{}.json",if correct{stage.as_str()}else{"failed"})),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if !correct||stage=="failed"||stage=="complete" {handle.exit(if correct&&stage=="complete"{0}else{1});}
            else if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref()!=Ok("1") {
                handle.get_webview_window("main").unwrap().eval(format!("window.__CUMULUS_PROOF_ACK__={};",json!(stage))).unwrap();
            } else {let h=handle.clone();let o=output.clone();tauri::async_runtime::spawn(async move{capture_ack::acknowledge(&h,&o,&stage,value["captureHandshake"]==true).await;});}
        });Ok(())
    }).on_page_load(move|webview,payload|{if payload.event()==PageLoadEvent::Finished {webview.eval(format!("window.__emailLegacy={legacy};")).unwrap();webview.eval(capture_ack::script(include_str!("email_login_ui_probe.js"))).unwrap();}})
    .run(tauri::generate_context!()).expect("email login proof");
}
