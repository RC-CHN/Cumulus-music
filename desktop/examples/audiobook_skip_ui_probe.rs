//! Real WebView and libmpv with only synthetic podcast authority and media.
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
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    podcast::AudiobookSkip,
    protocol::{
        api::{Api, ApiFuture, PodcastProgressPermission, identifier},
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

const SWITCH: &str = "/api/voice/alg/skip/switch/get";
const INTERVALS: &str = "/api/voice/alg/skip/info/get";
const CHECKPOINT: &str = "/api/dj/playrecord/upload";
const MEDIA: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";

struct Library {
    base: relay_fixture::Library,
    duration_ms: u32,
    reads: Mutex<Vec<Value>>,
    progress: Mutex<Vec<Value>>,
}

fn radio(id: u64) -> Value {
    let (name, count, category, category_id) = match id {
        8 => ("合成有声书", 2, "文学出版", 10001),
        9 => ("合成普通播客", 1, "音乐", 2),
        _ => panic!("unknown synthetic radio"),
    };
    json!({"id":id,"name":name,"programCount":count,"subed":true,"category":category,"categoryId":category_id})
}

impl Library {
    fn voice(&self, id: u64) -> Value {
        let source = match id {
            501 | 502 => 8,
            601 => 9,
            _ => panic!("unknown synthetic voice"),
        };
        json!({"id":id,"name":format!("合成声音 {id}"),"duration":self.duration_ms,
            "mainSong":{"id":id+1000,"duration":self.duration_ms},"radio":radio(source)})
    }
}

fn number(value: &Value) -> u64 {
    identifier(value).unwrap().parse().unwrap()
}

impl Api for Library {
    fn podcast_progress_revision(&self) -> Result<Option<String>> {
        Ok(Some("audiobook-fixture-revision".into()))
    }

    fn upload_podcast_progress<'a>(
        &'a self,
        payload: Value,
        revision: &'a str,
        allowed: &'a PodcastProgressPermission,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            assert_eq!(revision, "audiobook-fixture-revision");
            if !allowed()? {
                return Err(Error::Cancelled);
            }
            self.progress.lock().unwrap().push(payload);
            Ok(())
        })
    }

    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if path.contains("/skip/")
                || matches!(
                    path,
                    CHECKPOINT
                        | "/api/djradio/sub"
                        | "/api/djradio/unsub"
                        | "/api/dj/user/setting/resumeplay/set"
                )
                || path.ends_with("/submit")
                || path.ends_with("/update")
            {
                assert!(
                    matches!(path, SWITCH | INTERVALS),
                    "unexpected fixture write: {path}"
                );
                assert!(!cache);
                self.reads.lock().unwrap().push(json!([path, body]));
            }
            Ok(match path {
                "/api/my/podcast/page/list" => {
                    let id = match body["selectType"].as_u64() {
                        Some(1) => 9,
                        Some(2) => 8,
                        _ => panic!("unexpected collection kind"),
                    };
                    json!({"code":200,"data":{"radioIdList":[id],"myPodcastVOList":[radio(id)]}})
                }
                "/api/content/my/liked/voice" => {
                    json!({"code":200,"data":{"data":[],"hasMore":false}})
                }
                "/api/djradio/v3/get" => json!({"code":200,"data":radio(number(&body["id"]))}),
                "/api/v6/dj/program/byradio" => {
                    let mut programs = match number(&body["radioId"]) {
                        8 => vec![self.voice(501), self.voice(502)],
                        9 => vec![self.voice(601)],
                        _ => panic!("unexpected radio program page"),
                    };
                    assert_eq!(body["offset"], 0);
                    let ascending = body["asc"].as_bool().unwrap();
                    if !ascending {
                        programs.reverse();
                    }
                    json!({"code":200,"data":{"count":programs.len(),"programs":programs,"more":false,"asc":ascending}})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":0}})
                }
                "/api/dj/program/detail" => {
                    json!({"code":200,"program":self.voice(number(&body["id"]))})
                }
                "/api/dj/play/url/check" => {
                    let id = number(&body["voiceId"]);
                    assert!(matches!(id, 501 | 502 | 601));
                    json!({"code":200,"data":{"voiceId":id,"songId":id+1000,"canPlay":true}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(body["ids"].as_str().unwrap())?;
                    let id = number(&ids[0]);
                    assert!(matches!(id, 1501 | 1502 | 1601));
                    json!({"code":200,"data":[{"id":ids[0],"url":MEDIA,"code":200,"md5":"fixture","level":"standard","expi":120}]})
                }
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
                "/api/link/position/show/resource" => {
                    json!({"code":200,"data":{"commonResourceList":[]}})
                }
                SWITCH => {
                    assert_eq!(body, json!({"radioId":8}));
                    json!({"code":200,"data":true})
                }
                INTERVALS => {
                    let ids = body["programIdsStr"].as_array().unwrap();
                    assert_eq!(ids.len(), 1);
                    let id = ids[0].as_str().unwrap();
                    assert!(matches!(id, "501" | "502"));
                    json!({"code":200,"data":[{"programId":id,"skipBeginDuration":45,"skipEndDuration":15}]})
                }
                _ => return self.base.request(path, body, cache).await,
            })
        })
    }
}

#[cfg(target_os = "linux")]
fn locale(category: libc::c_int) -> String {
    // SAFETY: null queries the initialized locale; copy the returned C string.
    let value = unsafe { libc::setlocale(category, std::ptr::null()) };
    assert!(!value.is_null());
    unsafe { std::ffi::CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned()
}

fn main() {
    // SAFETY: before GTK/runtime initialization and any worker thread.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    let seconds = match std::env::var("CUMULUS_AUDIOBOOK_SKIP_CASE").as_deref() {
        Ok("long") => 220,
        Ok("short") => 199,
        _ => panic!("audiobook synthetic case required"),
    };
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .unwrap();
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let api = Arc::new(Library {
        base: Default::default(),
        duration_ms: seconds * 1000,
        reads: Default::default(),
        progress: Default::default(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    let wave = relay_fixture::wave_seconds(&root, seconds).unwrap();
    let client = MusicClient::with_session(session, registry)
        .with_audio_factory(move || Ok(Box::new(relay_fixture::NativeAudio::open(wave.clone())?)));
    let mut context = assets::context();
    let mut config = context.config().app.windows[0].clone();
    config.width = 1024.0;
    config.height = 700.0;
    context.config_mut().app.windows[0].create = false;
    let source = std::env::var_os("CUMULUS_UX_SCRIPT").map_or_else(
        || include_str!("audiobook_skip_ui_probe.js").to_owned(),
        |path| fs::read_to_string(path).expect("explicit synthetic script"),
    );
    let source = format!("window.__audiobookDuration={seconds};\n{source}");
    let source = if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
        capture_ack::script(&source)
    } else {
        source
    };
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            fs::write(output.join("locale.json"), serde_json::to_vec(&json!({
                "numeric":locale(libc::LC_NUMERIC),
                "messages":locale(libc::LC_MESSAGES),
                "characters":locale(libc::LC_CTYPE),
            }))?)?;
            app.manage(client);
            app.add_capability(r#"{"identifier":"audiobook-skip-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or("").to_owned();
                if stage == "checkpoint_poll" {
                    let nonce = value["nonce"].as_u64().unwrap();
                    let completed = api.progress.lock().unwrap().iter().any(|row| {
                        row["programId"] == 501 && row["isListened"] == true
                    });
                    handle.get_webview_window("main").unwrap().eval(format!(
                        "window.__audiobookCheckpoint = [{nonce}, {completed}];"
                    )).unwrap();
                    return;
                }
                if !(["complete", "failed"].contains(&stage.as_str())
                    || stage.starts_with("ux_") && stage.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')) {
                    return;
                }
                value["reads"] = json!(api.reads.lock().unwrap().clone());
                value["checkpoints"] = json!(api.progress.lock().unwrap().clone());
                value["localBookSetting"] = json!(AudiobookSkip::read(&store, "2", "8").unwrap());
                value["localOrdinarySetting"] = json!(AudiobookSkip::read(&store, "2", "9").unwrap());
                value["localOtherAccountSetting"] = json!(AudiobookSkip::read(&store, "3", "8").unwrap());
                fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                let handle = handle.clone();
                let output = output.clone();
                tauri::async_runtime::spawn(async move {
                    if stage == "complete" || stage == "failed" {
                        handle.exit(if stage == "complete" {0} else {1});
                    } else if capture_ack::acknowledge(&handle, &output, &stage, true).await {
                        handle.get_webview_window("main").unwrap().eval(format!(
                            "window.__audiobookStage = {};",
                            json!(stage)
                        )).unwrap();
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
        .run(context).expect("audiobook skip proof");
}
