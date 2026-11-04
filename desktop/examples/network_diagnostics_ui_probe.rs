//! Production UI and IPC with an isolated signed-out fixture. Only actual
//! loopback diagnostics and local routing state are used.
#[path = "support/assets.rs"]
mod assets;
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
    sync::{Arc, Mutex},
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

#[derive(Default)]
struct ClosedApi(Mutex<Vec<String>>);
impl Api for ClosedApi {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.0.lock().unwrap().push(path.to_owned());
            Ok(match path {
                "/api/w/nuser/account/get" => json!({"code":301}),
                _ => json!({"code":200,"data":[]}),
            })
        })
    }
}

fn main() {
    // SAFETY: first operation, before GTK or the WebView runtime.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof directory"));
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .expect("new proof directory");
    let mut context = assets::context();
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let api = Arc::new(ClosedApi::default());
            let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
            let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
            app.manage(MusicClient::with_session(session, registry));
            app.add_capability(r#"{"identifier":"diagnostics-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let webview = root.join("webview");
            app.listen("diagnostics-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).expect("probe event");
                let stage = value["stage"].as_str().expect("probe stage").to_owned();
                if !["settings", "diagnostics", "complete", "failed"].contains(&stage.as_str()) {
                    return;
                }
                value["closedApiPaths"] = json!(api.0.lock().unwrap().as_slice());
                fs::write(root.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                if stage == "complete" || stage == "failed" {
                    handle.exit(i32::from(stage == "failed"));
                }
            });
            tauri::WebviewWindowBuilder::from_config(app, &window_config)?
                .data_directory(webview)
                .build()?;
            Ok(())
        })
        .on_page_load(|window, payload| {
            if window.label() == "main" && payload.event() == PageLoadEvent::Finished {
                let override_script = std::env::var_os("CUMULUS_NETWORK_DIAGNOSTICS_PROOF_SCRIPT")
                    .map(|path| fs::read_to_string(path).expect("local synthetic proof script"));
                window.eval(override_script.as_deref().unwrap_or(include_str!("network_diagnostics_ui_probe.js"))).unwrap();
            }
        })
        .run(context)
        .expect("network diagnostics proof");
}
