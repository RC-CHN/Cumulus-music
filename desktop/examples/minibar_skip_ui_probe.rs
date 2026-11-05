//! The real main PlayerBar, count/browser lifecycle, native player and null mpv.
//! Cohort authority and HTTP replies are sealed synthetic fixtures.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/natural_eof_fixture.rs"]
mod eof_fixture;
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
    playback::{PlaybackMode, PlaybackQueue, engine::AudioEngine},
    protocol::{
        api::{Api, ApiFuture, identifier},
        http::ApiOrigin,
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

const MEDIA: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";
const REVISION: &str = "minibar-proof-only";
const URL: &str = "/api/song/enhance/player/url/v1";

struct ProofApi {
    base: fixture::Library,
    group: &'static str,
    eof: Option<eof_fixture::Case>,
    trace: eof_fixture::Trace,
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
                    let source = if let Some(case) = self.eof {
                        case.url(&id)
                    } else {
                        // Existing manual proof checks no row 2 URL call.
                        Some(MEDIA)
                    };
                    match source {
                        Some(url) => {
                            json!({"code":200,"data":[{"id":ids[0],"url":url,"code":200,"md5":"fixture","level":"standard","expi":120}]})
                        }
                        None => json!({"code":200,"data":[{"id":ids[0],"code":403}]}),
                    }
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

fn song(id: u64, eof: Option<eof_fixture::Case>) -> Value {
    let playable = eof.map_or(id != 2, |case| case.playable(id));
    let (title, duration) = match eof {
        None => ("合成底栏歌曲", 30000),
        Some(_) if id == 1 => ("合成自然结束歌曲", eof_fixture::FIRST_SECONDS * 1000),
        Some(_) => ("合成自然结束歌曲", eof_fixture::TARGET_SECONDS * 1000),
    };
    json!({"id":id,"song":{"id":id,"name":format!("{title} {id}"),"dt":duration,
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
    let eof = eof_fixture::Case::from_env();
    let group = eof.map_or_else(
        || match std::env::var("CUMULUS_MINIBAR_SKIP_GROUP").as_deref() {
            Ok("t1") => "t1",
            Ok("c") => "c",
            _ => panic!("a sealed experiment group is required"),
        },
        eof_fixture::Case::group,
    );
    let expected = if group == "t1" {
        OrdinarySkipAssignment::Enabled
    } else {
        OrdinarySkipAssignment::Control
    };
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .unwrap();
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let mut queue = PlaybackQueue::new(Some(store.clone()), "2").unwrap();
    queue
        .set_mode(eof.map_or(PlaybackMode::SingleLoop, eof_fixture::Case::mode))
        .unwrap();
    let displayed = eof.map_or([1, 2, 3], eof_fixture::Case::displayed_rows);
    queue
        .replace(
            displayed.into_iter().map(|id| song(id, eof)).collect(),
            displayed.iter().position(|&id| id == 1).unwrap(),
            json!({"kind":"playlist"}),
        )
        .unwrap();
    if let Some(deck) = eof.and_then(eof_fixture::Case::random_deck) {
        let mut snapshot = queue.snapshot();
        snapshot.order = deck
            .map(|slot| snapshot.items[slot].entry_id.clone())
            .to_vec();
        store
            .put(
                "2",
                "playback",
                "queue",
                &serde_json::to_value(snapshot).unwrap(),
            )
            .unwrap();
    }
    let api = Arc::new(ProofApi {
        base: Default::default(),
        group,
        eof,
        trace: Default::default(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(
            api.clone(),
            json!({"userId":2,"nickname":"合成底栏账号"}),
            &session.token().unwrap(),
        )
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = MusicClient::with_session(session, registry);
    let client = if eof.is_some() {
        let media = eof_fixture::Media::generate(&root).unwrap();
        let trace = api.trace.clone();
        let media_store = store.clone();
        client.with_audio_factory(move || -> Result<Box<dyn AudioEngine>> {
            Ok(Box::new(eof_fixture::ObservedAudio::open(
                media.clone(),
                trace.clone(),
                media_store.clone(),
            )?))
        })
    } else {
        let wave = relay_fixture::wave(&root).unwrap();
        client.with_audio_factory(move || {
            Ok(Box::new(relay_fixture::NativeAudio::open(wave.clone())?))
        })
    };
    let mut context = assets::context();
    let mut config = context.config().app.windows[0].clone();
    config.width = 1024.0;
    config.height = 700.0;
    context.config_mut().app.windows[0].create = false;
    let source = std::env::var_os("CUMULUS_UX_SCRIPT").map_or_else(
        || {
            if eof.is_some() {
                include_str!("natural_eof_ui_probe.js").to_owned()
            } else {
                include_str!("minibar_skip_ui_probe.js").to_owned()
            }
        },
        |path| fs::read_to_string(path).expect("explicit sealed probe script"),
    );
    let source = format!(
        "window.__minibarProofGroup={};window.__ordinaryEofProofCase={};\n{source}",
        json!(group),
        json!(eof.map(eof_fixture::Case::name))
    );
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
                if !(matches!(stage.as_str(), "ux_01_before" | "ux_02_after" | "complete" | "failed")
                    || eof == Some(eof_fixture::Case::Control) && stage == "ux_03_control_next")
                {
                    return;
                }
                value["trace"] = json!(*api.trace.lock().unwrap());
                if eof.is_some() {
                    value["durablePosition"] = json!(store.get("2", "playback", "position").unwrap());
                }
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
