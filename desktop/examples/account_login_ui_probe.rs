//! Production account/login UI and IPC; synthetic HTTP, isolated native credentials.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/auth.rs"]
mod auth;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/account_login_fixture.rs"]
mod fixture;
use cumulus_core::client::MusicClient;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    // SAFETY: first operation, before GTK or any runtime/worker starts.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated output root"));
    fs::create_dir(&root).expect("new proof root");
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let avatar = std::env::var_os("CUMULUS_PROOF_ACCOUNT_AVATAR").is_some();
    let service = Arc::new(fixture::Service::new(avatar));
    let mut context = assets::context();
    if avatar {
        use tauri::utils::config::Csp;
        let security = &mut context.config_mut().app.security;
        let Some(Csp::Policy(policy)) = security.csp.take() else {
            panic!("expected application CSP");
        };
        // Fixture avatar URLs exercise the native correlation rule, but this
        // WebView never fetches them from the real image CDN.
        security.csp = Some(Csp::Policy(
            policy.replace(" https://*.music.126.net https://*.music.163.com", ""),
        ));
    }
    let baseline = std::env::var_os("CUMULUS_PROOF_BASELINE").is_some();
    let account_detail = std::env::var_os("CUMULUS_PROOF_ACCOUNT_DETAIL").is_some();
    let profile_edit = std::env::var_os("CUMULUS_PROOF_PROFILE_EDIT").is_some();
    let active_switch = std::env::var_os("CUMULUS_PROOF_ACTIVE_SWITCH").is_some();
    let quick_login = std::env::var_os("CUMULUS_PROOF_QUICK_LOGIN").is_some();
    let device_removal = std::env::var_os("CUMULUS_PROOF_DEVICE_REMOVAL").is_some();
    let browser_service = service.clone();
    cumulus_desktop::configure(tauri::Builder::default())
        .manage(cumulus_desktop::LoginDocumentBrowser::with_launcher(move |document| browser_service.open_document(document)))
        .setup(move |app| {
            let mut client = MusicClient::open_with_http_backend(&root.join("state"), None, service.clone())?;
            if avatar {
                let source = root.join("synthetic-avatar.bmp");
                fs::write(&source, fixture::avatar_source())?;
                app.manage(cumulus_desktop::AccountAvatarPicker::with_picker(move || Some(source.clone())));
                client = client.with_account_avatar_backend(service.clone());
            }
            let client = cumulus_desktop::renewal::configure(client, &cumulus_core::protocol::http::ClientProfile::default());
            app.manage(cumulus_desktop::configure_native_audio(client, &app.path().resource_dir()?, true));
            cumulus_desktop::renewal::start(app.handle());
            app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(1120.0, 760.0))?;
            app.add_capability(r#"{"identifier":"account-login-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let fixture = service.clone();
            app.listen("account-login-fixture", move |event| {
                let value: Value = serde_json::from_str(event.payload()).unwrap();
                fixture.command(value["mode"].as_str().unwrap());
                handle.get_webview_window("main").unwrap().eval(format!("window.__accountLoginPending={};window.__accountLoginAck={};", fixture.pending(), value["id"])).unwrap();
            });
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or("").to_owned();
                if stage == "resize" {
                    let scale = value["scale"].as_f64().unwrap();
                    assert!(scale.is_finite() && scale > 0.0 && scale <= 4.0);
                    handle.get_webview_window("main").unwrap().set_size(tauri::PhysicalSize::new((value["width"].as_f64().unwrap()*scale).round() as u32, (value["height"].as_f64().unwrap()*scale).round() as u32)).unwrap();
                    return;
                }
                if !stage.starts_with("ux_") && !["complete", "failed"].contains(&stage.as_str()) { return; }
                let client = handle.state::<MusicClient>();
                let account = client.account().unwrap();
                value["nativeAccount"] = serde_json::to_value(&account).unwrap();
                value["sessionRenewal"] = serde_json::to_value(client.session_renewal(&account.epoch).unwrap()).unwrap();
                value["fixture"] = service.snapshot();
                value["savedAccounts"] = serde_json::to_value(client.saved_accounts(&account.epoch).unwrap()).unwrap();
                value["realMusicRequests"] = json!(0);
                if device_removal {
                    value["nativeRemoval"] = serde_json::to_value(client.login_device_removal(&account.epoch).ok()).unwrap();
                }
                let correct = value["expectNickname"].as_str().is_none_or(|name| account.nickname.as_deref() == Some(name))
                    && (!value["expectGuest"].as_bool().unwrap_or(false) || account.status != cumulus_core::account::SessionStatus::Authenticated);
                if !correct { value["nativeMismatch"] = json!(true); }
                let name = if correct { stage.as_str() } else { "failed" };
                fs::write(output.join(format!("{name}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                if !correct || stage == "failed" || stage == "complete" {
                    handle.exit(if correct && stage == "complete" { 0 } else { 1 });
                } else if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() != Ok("1") {
                    handle.get_webview_window("main").unwrap().eval(format!("window.__CUMULUS_PROOF_ACK__={};", json!(stage))).unwrap();
                } else {
                    let handle = handle.clone(); let output = output.clone();
                    tauri::async_runtime::spawn(async move {
                        capture_ack::acknowledge(&handle, &output, &stage, value["captureHandshake"] == true).await;
                    });
                }
            });
            Ok(())
        })
        .on_page_load(move |webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                webview.eval(format!("window.__accountLoginBaseline={baseline};")).unwrap();
                webview.eval(format!("window.__accountDetailProof={account_detail};")).unwrap();
                webview.eval(format!("window.__profileEditProof={profile_edit};")).unwrap();
                webview.eval(format!("window.__accountAvatarProof={avatar};")).unwrap();
                webview.eval(format!("window.__deviceRemovalProof={device_removal};")).unwrap();
                webview.eval(format!("window.__quickLoginProof={quick_login};")).unwrap();
                webview.eval(format!("window.__activeSwitchProof={active_switch};")).unwrap();
                let script = if std::env::var("CUMULUS_PROOF_LOGIN_ENTRY").as_deref() == Ok("1") {
                    include_str!("login_entry_ui_probe.js")
                } else {
                    include_str!("account_login_ui_probe.js")
                };
                webview.eval(capture_ack::script(script)).unwrap();
            }
        })
        .run(context)
        .expect("account/login proof");
}
