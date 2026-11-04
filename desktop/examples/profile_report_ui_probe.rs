//! Real desktop WebView with a closed user-report transport.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;

use cumulus_core::{
    Error,
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

const REPORT: &str = "/api/report/reportuser";

#[derive(Default)]
struct Library {
    base: catalog_fixture::Library,
    calls: Mutex<Vec<Value>>,
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            Ok(match path {
                REPORT => {
                    assert!(!cache);
                    self.calls.lock().unwrap().push(body.clone());
                    if body["userid"] == "75" {
                        return Err(Error::Transport("synthetic report reply lost".into()));
                    }
                    assert_eq!(body["userid"], "73");
                    json!({"code":200})
                }
                "/api/v6/playlist/detail" if matches!(body["id"].as_str(), Some("31" | "33")) => {
                    let (id, uid) = if body["id"] == "31" {
                        (31, 73)
                    } else {
                        (33, 75)
                    };
                    json!({"code":200,"playlist":{"id":id,"userId":uid,"name":format!("合成举报歌单 {id}"),
                        "creator":{"userId":uid,"nickname":format!("合成举报用户 {uid}")},"trackCount":0,
                        "privacy":0,"trackIds":[],"tracks":[]}})
                }
                "/api/w/v1/user/detail/73" | "/api/w/v1/user/detail/75" => {
                    let uid = path.rsplit('/').next().unwrap();
                    assert_eq!(body, json!({"all":true,"userId":uid}));
                    assert!(!cache);
                    json!({"code":200,"profile":{"userId":uid,"nickname":format!("合成举报用户 {uid}"),
                        "followed":false,"blacklist":false,"inBlacklist":false,"accountStatus":0}})
                }
                "/api/djradio/get/create/sub/count" => {
                    json!({"code":200,"data":{"createCount":0,"subCount":0}})
                }
                _ if path.starts_with("/api/report/")
                    || path.starts_with("/api/user/follow/")
                    || path.starts_with("/api/user/delfollow/")
                    || path.starts_with("/api/blacklist/") =>
                {
                    panic!("unexpected synthetic mutation: {path}");
                }
                _ => return self.base.request(path, body, cache).await,
            })
        })
    }
}
fn main() {
    // SAFETY: prepare GTK graphics before creating any runtime threads.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .unwrap();
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Library::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = MusicClient::with_session(session, registry);
    let mut context = assets::context();
    let mut config = context.config().app.windows[0].clone();
    config.width = 960.0;
    config.height = 640.0;
    context.config_mut().app.windows[0].create = false;
    let source = std::env::var_os("CUMULUS_UX_SCRIPT").map_or_else(
        || include_str!("profile_report_ui_probe.js").to_owned(),
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
            app.add_capability(r#"{"identifier":"profile-report-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or_default().to_owned();
                if !(["complete", "failed"].contains(&stage.as_str()) ||
                    stage.starts_with("ux_") && stage.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) {
                    return;
                }
                value["reportCount"] = json!(api.calls.lock().unwrap().len());
                if stage == "complete" || stage == "failed" {
                    value["calls"] = json!(api.calls.lock().unwrap().clone());
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
        .run(context).expect("profile report proof");
}
