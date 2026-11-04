//! Real desktop WebView with a closed account and user blacklist transport.
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
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

const LIST: &str = "/api/blacklist/get";
const ADD: &str = "/api/blacklist/add";
const DELETE: &str = "/api/blacklist/delete";
const TARGET: &str = "/api/w/v1/user/detail/73";

struct Library {
    base: catalog_fixture::Library,
    calls: Mutex<Vec<Value>>,
    blocked: AtomicBool,
    first_read: AtomicBool,
    lost_read: AtomicBool,
    deletions: AtomicUsize,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            base: Default::default(),
            calls: Default::default(),
            blocked: AtomicBool::new(true),
            first_read: AtomicBool::new(true),
            lost_read: Default::default(),
            deletions: Default::default(),
        }
    }
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if matches!(path, LIST | ADD | DELETE | TARGET) {
                assert!(!cache);
                self.calls.lock().unwrap().push(json!([path, body]));
            }
            if path.starts_with("/api/report/")
                || path.starts_with("/api/user/follow/")
                || path.starts_with("/api/user/delfollow/")
                || path.starts_with("/api/music-blacklist/")
            {
                panic!("unexpected unrelated synthetic mutation: {path}");
            }
            Ok(match path {
                LIST => {
                    assert_eq!(body, json!({"userid":"2"}));
                    if self.first_read.swap(false, Ordering::SeqCst)
                        || self.lost_read.swap(false, Ordering::SeqCst)
                    {
                        return Err(Error::Transport("synthetic list reply lost".into()));
                    }
                    let mut rows = Vec::new();
                    if self.blocked.load(Ordering::SeqCst) {
                        rows.push(json!({"userId":73,"nickname":"合成用户 73","signature":"测试用户黑名单，可从这里打开资料或解除。"}));
                    }
                    rows.extend((100..=144).map(|id| json!({"userId":id,"nickName":format!("合成用户 {id}"),
                        "signature":"长列表只挂载可见卡片。","accountStatus":if id==100 {40}else{0}})));
                    json!({"code":200,"data":rows})
                }
                ADD => {
                    assert_eq!(body, json!({"userid":"73"}));
                    assert!(!self.blocked.swap(true, Ordering::SeqCst));
                    json!({"code":200})
                }
                DELETE => {
                    assert_eq!(body, json!({"userid":"73"}));
                    assert!(self.blocked.swap(false, Ordering::SeqCst));
                    if self.deletions.fetch_add(1, Ordering::SeqCst) == 1 {
                        self.lost_read.store(true, Ordering::SeqCst);
                        return Err(Error::Transport("synthetic delete reply lost".into()));
                    }
                    json!({"code":200})
                }
                TARGET => {
                    assert_eq!(body, json!({"all":true,"userId":"73"}));
                    json!({"code":200,"profile":{"userId":73,"nickname":"合成用户 73","followed":false,
                        "blacklist":self.blocked.load(Ordering::SeqCst),"inBlacklist":false}})
                }
                "/api/djradio/get/create/sub/count" => {
                    json!({"code":200,"data":{"createCount":0,"subCount":0}})
                }
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
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
        || include_str!("user_blacklist_ui_probe.js").to_owned(),
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
            app.add_capability(r#"{"identifier":"user-blacklist-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or_default().to_owned();
                if !(["complete", "failed"].contains(&stage.as_str()) ||
                    stage.starts_with("ux_") && stage.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) {
                    return;
                }
                if stage == "complete" || stage == "failed" {
                    value["calls"] = json!(api.calls.lock().unwrap().clone());
                    value["targetBlocked"] = json!(api.blocked.load(Ordering::SeqCst));
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
        .run(context).expect("user blacklist proof");
}
