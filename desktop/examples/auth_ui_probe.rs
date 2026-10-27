//! Full production login UI/IPC and real credential persistence, synthetic HTTP.
#[path = "support/auth.rs"]
mod fixture;
use cumulus_core::client::MusicClient;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit output root"));
    let reopen = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    if !reopen {
        fs::create_dir(&root).expect("new isolated root");
    }
    let output = root.join(if reopen { "reopen" } else { "first" });
    fs::create_dir(&output).expect("new run");
    let service = Arc::new(fixture::Service::default());
    let setup = root.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        let client=MusicClient::open_with_http_backend(&setup.join("state"),None,service.clone())?;
        let client=cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true);
        app.manage(client);
        if reopen || small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"auth-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();let service=service.clone();
        app.listen("auth-proof",move|event|{
            let mut value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("").to_owned();
            if !stage.starts_with("auth_") && !["complete","failed"].contains(&stage.as_str()){return;}
            let client=handle.state::<MusicClient>();
            let account=client.account().unwrap();
            let expected=value["expectNickname"].as_str();
            let guest=value["expectGuest"].as_bool().unwrap_or(false);
            let correct=expected.is_none_or(|s|account.nickname.as_deref()==Some(s))
                && (!guest || account.status!=cumulus_core::account::SessionStatus::Authenticated);
            value["nativeAccount"]=serde_json::to_value(&account).unwrap();
            value["savedAccounts"]=serde_json::to_value(client.saved_accounts(&account.epoch).unwrap()).unwrap();
            value["fixtureRequests"]=json!(*service.paths.lock().unwrap());
            value["networkRequests"]=json!(0);
            if !correct {value["nativeMismatch"]=json!(true);fs::write(output.join("failed.json"),serde_json::to_vec_pretty(&value).unwrap()).unwrap();handle.exit(1);return;}
            if stage=="complete" && !reopen {
                let paths=service.paths.lock().unwrap();
                if paths.get("/api/logout")!=Some(&1) || paths.get("/api/sms/captcha/sent")!=Some(&1) || paths.get("/api/user/2fa/securityEmail/send")!=Some(&1) {
                    value["unexpectedMutationCounts"]=json!(true);fs::write(output.join("failed.json"),serde_json::to_vec_pretty(&value).unwrap()).unwrap();handle.exit(1);return;
                }
            }
            fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if stage=="complete" || stage=="failed" {handle.exit(if stage=="complete"{0}else{1});}
            else {handle.get_webview_window("main").unwrap().eval(format!("window.__authAck={};",json!(stage))).unwrap();}
        });Ok(())
    }).on_page_load(move|webview,payload|{
        if payload.event()==PageLoadEvent::Finished {
            webview.eval(format!("window.__authProof={};",json!({"reopen":reopen}))).unwrap();
            webview.eval(include_str!("auth_ui_probe.js")).unwrap();
        }
    }).run(tauri::generate_context!()).expect("auth UI proof");
}
