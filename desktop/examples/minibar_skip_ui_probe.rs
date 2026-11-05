//! The real main PlayerBar, count/browser lifecycle, native player and null mpv.
//! Cohort authority and HTTP replies are sealed synthetic fixtures.
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
    Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, pc_experiments::OrdinarySkipAssignment},
    playback::{PlaybackMode, PlaybackQueue},
    protocol::{
        api::{Api, ApiFuture, identifier},
        http::ApiOrigin,
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

const MEDIA: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";
const REVISION: &str = "minibar-proof-only";
const URL: &str = "/api/song/enhance/player/url/v1";

struct ProofApi {
    base: fixture::Library,
    group: &'static str,
    trace: Mutex<Vec<Value>>,
}

impl Api for ProofApi {
    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        Ok(Some(REVISION.into()))
    }

    fn verified_pc_experiment_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, PcExperimentGroups> {
        Box::pin(async move {
            assert_eq!(revision, REVISION);
            self.trace
                .lock()
                .unwrap()
                .push(json!({"kind":"verified_assignment","group":self.group}));
            PcExperimentGroups::refreshed(&json!({"code":200,"data":[
                {"expName":"PH-PC-skipSongs2","expGroupName":self.group}
            ]}))
        })
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            Ok(match path {
                "/api/pl/count" => {
                    assert!(!cache);
                    assert_eq!(args, json!({}));
                    self.trace
                        .lock()
                        .unwrap()
                        .push(json!({"kind":"count","path":path}));
                    json!({"code":200,"msg":1,"comment":0,"forward":0,"notice":0})
                }
                URL => {
                    assert!(!cache);
                    let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().unwrap())?;
                    assert_eq!(ids.len(), 1);
                    let id = identifier(&ids[0])?;
                    assert!(matches!(id.as_str(), "1" | "2" | "3"));
                    self.trace
                        .lock()
                        .unwrap()
                        .push(json!({"kind":"authorization","path":path,"id":id}));
                    // Row 2's catalog privilege is denied and must not reach
                    // this URL path in either verified group. The runner
                    // independently asserts the full authorization sequence.
                    json!({"code":200,"data":[{"id":ids[0],"url":MEDIA,"code":200,"md5":"fixture","level":"standard","expi":120}]})
                }
                "/api/v3/song/detail" | "/api/rtrs/abt/front/expinfo/list" => {
                    self.trace
                        .lock()
                        .unwrap()
                        .push(json!({"kind":"ordinary_request","path":path}));
                    return self.base.request(path, args, cache).await;
                }
                _ => return self.base.request(path, args, cache).await,
            })
        })
    }
}

fn song(id: u64) -> Value {
    let playable = id != 2;
    json!({"id":id,"song":{"id":id,"name":format!("合成底栏歌曲 {id}"),"dt":30000,
        "privilege":{"fee":0,"flag":0,"pl":if playable {128000} else {0},
            "st":if playable {0} else {-200}}}})
}

#[cfg(target_os = "linux")]
fn locale(category: libc::c_int) -> String {
    // SAFETY: a null argument queries the initialized locale; copy its C string.
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
    let (group, expected) = match std::env::var("CUMULUS_MINIBAR_SKIP_GROUP").as_deref() {
        Ok("t1") => ("t1", OrdinarySkipAssignment::Enabled),
        Ok("c") => ("c", OrdinarySkipAssignment::Control),
        _ => panic!("a sealed experiment group is required"),
    };
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .unwrap();
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let mut queue = PlaybackQueue::new(Some(store.clone()), "2").unwrap();
    queue.set_mode(PlaybackMode::SingleLoop).unwrap();
    queue
        .replace((1..=3).map(song).collect(), 0, json!({"kind":"playlist"}))
        .unwrap();
    let api = Arc::new(ProofApi {
        base: Default::default(),
        group,
        trace: Default::default(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session
        .adopt(
            api.clone(),
            json!({"userId":2,"nickname":"合成底栏账号"}),
            &session.token().unwrap(),
        )
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let wave = relay_fixture::wave(&root).unwrap();
    let client = MusicClient::with_session(session, registry)
        .with_audio_factory(move || Ok(Box::new(relay_fixture::NativeAudio::open(wave.clone())?)));
    let mut context = assets::context();
    let mut config = context.config().app.windows[0].clone();
    config.width = 1024.0;
    config.height = 700.0;
    context.config_mut().app.windows[0].create = false;
    let source = std::env::var_os("CUMULUS_UX_SCRIPT").map_or_else(
        || include_str!("minibar_skip_ui_probe.js").to_owned(),
        |path| fs::read_to_string(path).expect("explicit sealed probe script"),
    );
    let source = format!("window.__minibarProofGroup={};\n{source}", json!(group));
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
            assert_eq!(
                tauri::async_runtime::block_on(client.official_pc_ordinary_skip_assignment(&epoch))?,
                expected
            );
            app.manage(client);
            cumulus_desktop::start_message_count_polling(app.handle())?;
            app.add_capability(r#"{"identifier":"minibar-skip-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or("").to_owned();
                if !matches!(stage.as_str(), "ux_01_before" | "ux_02_after" | "complete" | "failed") {
                    return;
                }
                value["trace"] = json!(*api.trace.lock().unwrap());
                fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                let handle = handle.clone();
                let output = output.clone();
                tauri::async_runtime::spawn(async move {
                    if matches!(stage.as_str(), "complete" | "failed") {
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
        .run(context).expect("minibar skip proof");
}
