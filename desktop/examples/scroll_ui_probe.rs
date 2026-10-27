//! Production UI/IPC with a large synthetic library, no account or network.
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
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

struct Library;
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            Ok(match path {
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"profile":{"userId":2,"nickname":"滚动性能测试"}})
                }
                "/api/user/playlist" => {
                    json!({"code":200,"more":false,"playlist":(1..=300).map(|i|json!({"id":i,"userId":2,"name":format!("合成歌单 {i}"),"trackCount":2000})).collect::<Vec<_>>()})
                }
                "/api/song/like/get" => json!({"code":200,"ids":[3,7,100]}),
                "/api/v3/discovery/recommend/songs" => {
                    json!({"code":200,"data":{"dailySongs":(1..=2000).map(|i|json!({"id":i,"name":format!("合成歌曲 {i} · Scroll benchmark"),"ar":[{"name":"Cumulus"}],"al":{"name":"合成专辑"},"dt":235000,"fee":i%3})).collect::<Vec<_>>()}})
                }
                _ => {
                    return Err(cumulus_core::Error::Response(
                        "unexpected scroll fixture endpoint",
                    ));
                }
            })
        })
    }
}
fn main() {
    // SAFETY: first operation in this standalone test process, before any worker.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof directory"));
    fs::create_dir(&root).expect("proof directory must be new");
    let closing = root.clone();
    let controls = std::env::var_os("CUMULUS_PROOF_WINDOW_CONTROLS").is_some();
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
            let session = Arc::new(AccountSession::new(Arc::new(Library), Some(registry.store())));
            app.manage(MusicClient::with_session(session, registry));
            if std::env::var_os("CUMULUS_PROOF_SMALL").is_some() {
                app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0, 540.0))?;
            }
            app.add_capability(r#"{"identifier":"scroll-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("scroll-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).expect("proof JSON");
                if value["stage"] == "restore" {
                    let window = handle.get_webview_window("main").unwrap();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        let minimized = window.is_minimized().unwrap();
                        window.unminimize().unwrap();
                        window.set_focus().unwrap();
                        window.eval(format!("window.__restored={minimized};")).unwrap();
                    });
                    return;
                }
                value["nativeDecorations"] = json!(handle.get_webview_window("main").unwrap().is_decorated().unwrap());
                fs::write(root.join("result.json"), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                if value["ok"] != true || !controls { handle.exit(if value["ok"] == true {0} else {1}); }
            });
            Ok(())
        })
        .on_window_event(move |_, event| {
            if controls && matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                let path = closing.join("result.json");
                let mut value: Value = serde_json::from_slice(&fs::read(&path).expect("proof before close")).unwrap();
                value["closeRequested"] = json!(true);
                fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            }
        })
        .on_page_load(move |webview, payload| {
            if payload.event() == PageLoadEvent::Finished && webview.label() == "main" {
                #[cfg(target_os = "linux")]
                webview.with_webview(|view| {
                    use webkit2gtk::{SettingsExt, WebViewExt};
                    eprintln!("scroll probe: GPU policy {:?}", view.inner().settings().unwrap().hardware_acceleration_policy());
                }).unwrap();
                if controls { webview.eval("window.__testWindowControls=true;").unwrap(); }
                let script = std::env::var_os("CUMULUS_SCROLL_SCRIPT")
                    .map(|path| fs::read_to_string(path).expect("explicit scroll script"));
                webview.eval(script.as_deref().unwrap_or(include_str!("scroll_ui_probe.js"))).unwrap();
            }
        })
        .run(assets::context()).expect("scroll UI proof");
}
