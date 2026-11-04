//! Production desktop WebView and libmpv with a closed synthetic podcast transport.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;

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
        atomic::{AtomicUsize, Ordering},
    },
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

const RECENT: &str = "/api/social/my/recent/update/voice/list/v1";
const CURSOR: u64 = 1_700_000_000_123;
const MEDIA: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";

#[derive(Default)]
struct Library {
    base: relay_fixture::Library,
    first: AtomicUsize,
    next: AtomicUsize,
    calls: Mutex<Vec<Value>>,
}

fn radio() -> Value {
    json!({"id":8,"name":"合成播客","programCount":1,"subed":true})
}
fn voice(id: u64) -> Value {
    json!({"voiceId":id,"voiceName":format!("合成单集 {id}"),"mainSong":{"id":1000+id,"duration":30000},
        "radio":radio(),"duration":30000,"listenerCount":12000+id,"djPlayRecordVo":{"listenLocation":12000,"isListened":id==502},
        "disPlayStatus":if id==505 {"REMOVED"}else{"NORMAL"}})
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if path.contains("clear")
                || path.contains("submit")
                || path == "/api/djradio/my/radio/recommend"
            {
                panic!("unexpected synthetic remote action: {path}");
            }
            if matches!(
                path,
                RECENT
                    | "/api/dj/program/detail"
                    | "/api/dj/play/url/check"
                    | "/api/song/enhance/player/url/v1"
            ) {
                self.calls.lock().unwrap().push(json!([path, body]));
            }
            Ok(match path {
                RECENT if body == json!({}) => match self.first.fetch_add(1, Ordering::SeqCst) {
                    0 => return Err(Error::Transport("synthetic first failure".into())),
                    1 => {
                        let mut rows = (501..=518).map(voice).collect::<Vec<_>>();
                        rows[2].as_object_mut().unwrap().remove("mainSong");
                        json!({"code":200,"data":{"data":rows,"updateTime":CURSOR,"hasMore":true,"newVoiceCount":7}})
                    }
                    2 => json!({"code":200,"data":{"data":[],"hasMore":false}}),
                    _ => panic!("unexpected first-page replay"),
                },
                RECENT => {
                    assert_eq!(body, json!({"updateTime":CURSOR}));
                    match self.next.fetch_add(1, Ordering::SeqCst) {
                        0 => return Err(Error::Transport("synthetic continuation failure".into())),
                        1 => {
                            json!({"code":200,"data":{"data":[voice(518),voice(519),voice(520)],"hasMore":false,"newVoiceCount":99}})
                        }
                        _ => panic!("unexpected continuation replay"),
                    }
                }
                "/api/my/podcast/page/list" => {
                    json!({"code":200,"data":{"radioIdList":[8],"myPodcastVOList":[radio()]}})
                }
                "/api/content/my/liked/voice" => {
                    json!({"code":200,"data":{"data":[],"hasMore":false}})
                }
                "/api/djradio/v3/get" => json!({"code":200,"data":radio()}),
                "/api/v6/dj/program/byradio" => {
                    json!({"code":200,"data":{"programs":[voice(501)],"count":1,"more":false,"asc":true}})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":0}})
                }
                "/api/dj/program/detail" => {
                    let id = body["id"].as_str().unwrap().parse::<u64>().unwrap();
                    assert!(matches!(id, 501 | 503 | 504 | 506));
                    json!({"code":200,"program":voice(id)})
                }
                "/api/dj/play/url/check" => {
                    let id = body["voiceId"].as_str().unwrap().parse::<u64>().unwrap();
                    assert!(matches!(id, 501 | 503 | 504 | 506));
                    json!({"code":200,"data":{"voiceId":id,"songId":1000+id,"canPlay":true}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(body["ids"].as_str().unwrap())?;
                    assert!(
                        matches!(ids[0].as_u64(), Some(1501 | 1503 | 1504 | 1506))
                            || matches!(ids[0].as_str(), Some("1501" | "1503" | "1504" | "1506"))
                    );
                    json!({"code":200,"data":[{"id":ids[0],"url":MEDIA,"code":200,"md5":"fixture","level":"standard","expi":120}]})
                }
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
                "/api/link/position/show/resource" => {
                    json!({"code":200,"data":{"commonResourceList":[]}})
                }
                _ => return self.base.request(path, body, cache).await,
            })
        })
    }
}

fn main() {
    // SAFETY: before GTK/runtime initialization and any worker thread.
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
    let wave = relay_fixture::wave(&root).unwrap();
    let client = MusicClient::with_session(session, registry)
        .with_audio_factory(move || Ok(Box::new(relay_fixture::NativeAudio::open(wave.clone())?)));
    let mut context = assets::context();
    let mut config = context.config().app.windows[0].clone();
    config.width = 960.0;
    config.height = 640.0;
    context.config_mut().app.windows[0].create = false;
    let source = std::env::var_os("CUMULUS_UX_SCRIPT").map_or_else(
        || include_str!("podcast_recent_ui_probe.js").to_owned(),
        |path| fs::read_to_string(path).expect("explicit synthetic script"),
    );
    let source = if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
        capture_ack::script(&source)
    } else {
        source
    };
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            app.manage(client);
            app.add_capability(r#"{"identifier":"podcast-recent-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or("").to_owned();
                if !(["complete", "failed"].contains(&stage.as_str())
                    || stage.starts_with("ux_") && stage.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')) {
                    return;
                }
                if stage == "complete" {
                    value["calls"] = json!(api.calls.lock().unwrap().clone());
                }
                fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                let handle = handle.clone();
                let output = output.clone();
                tauri::async_runtime::spawn(async move {
                    if stage == "complete" || stage == "failed" {
                        handle.exit(if stage == "complete" {0} else {1});
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
        .run(context).expect("podcast recent proof");
}
