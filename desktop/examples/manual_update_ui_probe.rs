//! Production UI/IPC and browser handoff with a closed release transport.
#[path = "support/assets.rs"]
mod assets;
#[path = "../../core/tests/support/manual_updates.rs"]
mod update_fixture;

use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

struct SignedOut;
impl Api for SignedOut {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            Ok(match path {
                "/api/w/nuser/account/get" => json!({"code":301}),
                _ => json!({"code":200,"data":[]}),
            })
        })
    }
}

fn main() {
    // SAFETY: first operation before GTK and the WebView runtime.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("private proof directory"));
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .expect("new proof directory");
    let mut context = assets::context();
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
            let session = Arc::new(AccountSession::new(Arc::new(SignedOut), Some(registry.store())));
            app.manage(MusicClient::with_session(session, registry));
            let fixture = update_fixture::Fixture {
                mode: Default::default(), reads: Default::default(), installed: env!("CARGO_PKG_VERSION"),
            };
            #[cfg(not(target_os = "windows"))]
            let adapters = cumulus_desktop::UpdateAdapters::with_source(fixture.clone());
            #[cfg(target_os = "windows")]
            let adapters = {
                // Keep the Windows CI browser closed. The real Tauri command
                // still passes only its validated ReleasePage to this adapter.
                use cumulus_core::updates::UpdateFailure;
                use std::io::Write;
                let browser_root = root.clone();
                cumulus_desktop::UpdateAdapters::with_adapters(
                    fixture.clone(),
                    move |page| {
                        let mut log = fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(browser_root.join("browser.log"))
                            .map_err(|_| UpdateFailure::Browser)?;
                        writeln!(log, "1\n{}", page.url()).map_err(|_| UpdateFailure::Browser)?;
                        if browser_root.join("browser-fail").is_file() {
                            Err(UpdateFailure::Browser)
                        } else {
                            Ok(())
                        }
                    },
                )
            };
            let package_name = "Cumulus-999.1.0-linux-x64.deb";
            let good = root.join("good").join(package_name);
            let bad = root.join("bad").join(package_name);
            fs::create_dir(good.parent().expect("good directory"))?;
            fs::create_dir(bad.parent().expect("bad directory"))?;
            let mut bytes = update_fixture::synthetic_package(package_name);
            fs::write(&good, &bytes)?;
            *bytes.last_mut().expect("synthetic package is not empty") ^= 1;
            fs::write(&bad, bytes)?;
            let package_mode = Arc::new(AtomicU8::new(0));
            let picker_mode = package_mode.clone();
            app.manage(adapters.with_package_picker(move || match picker_mode.load(Ordering::SeqCst) {
                    0 => Some(good.clone()),
                    1 => Some(bad.clone()),
                    _ => None,
                }));
            app.add_capability(r#"{"identifier":"update-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let webview = root.join("webview");
            app.listen("update-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).expect("probe event");
                let stage = value["stage"].as_str().expect("proof stage").to_owned();
                match stage.as_str() {
                    "mode" => {
                        let mode = value["mode"].as_u64().filter(|mode| *mode <= 6).expect("proof mode");
                        fixture.mode.store(mode as u8, Ordering::SeqCst);
                    }
                    "package-mode" => {
                        let mode = value["mode"].as_u64().filter(|mode| *mode <= 2).expect("picker mode");
                        package_mode.store(mode as u8, Ordering::SeqCst);
                    }
                    "browser" => {
                        let marker = root.join("browser-fail");
                        if value["fail"] == true {
                            fs::write(marker, b"synthetic launch failure").unwrap();
                        } else if marker.is_file() {
                            fs::remove_file(marker).unwrap();
                        }
                    }
                    "settings" | "current" | "available" | "package" | "network_error" | "absent" | "complete" | "failed" => {
                        value["metadataReads"] = json!(fixture.reads.load(Ordering::SeqCst));
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
            if window.label() == "main" && payload.event() == PageLoadEvent::Finished {
                let override_script = std::env::var_os("CUMULUS_MANUAL_UPDATE_PROOF_SCRIPT")
                    .map(|path| fs::read_to_string(path).expect("local synthetic proof script"));
                window.eval(override_script.as_deref().unwrap_or(include_str!("manual_update_ui_probe.js"))).unwrap();
            }
        })
        .run(context)
        .expect("manual update proof");
}
