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
#[path = "support/podcast_scene_home_fixture.rs"]
mod scene_home_fixture;
#[path = "support/scene_palette_fixture.rs"]
mod scene_palette_fixture;
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
#[path = "support/voice_detail_fixture.rs"]
mod voice_detail_fixture;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::{
        api::{Api, ApiFuture, PodcastProgressPermission},
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

const PROGRESS: &str = "/api/dj/playrecord/upload";
const PROOF_REVISION: &str = "podcast-progress-fixture";
const RECENT: &str = "/api/social/my/recent/update/voice/list/v1";
const CURSOR: u64 = 1_700_000_000_123;
const MEDIA: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";

#[derive(Default)]
struct Library {
    base: relay_fixture::Library,
    first: AtomicUsize,
    next: AtomicUsize,
    calls: Mutex<Vec<Value>>,
    progress_enabled: bool,
    progress_uploads: Mutex<Vec<Value>>,
    scene_home: Option<scene_home_fixture::SceneHome>,
    scene_baseline: settings_fixture::Library,
    palette: Option<Arc<scene_palette_fixture::PaletteMedia>>,
    voice_detail: Option<voice_detail_fixture::VoiceDetail>,
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
    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        Ok(self
            .scene_home
            .as_ref()
            .map(|_| "scene-home-fixture-revision".to_owned()))
    }

    fn search_audio_toplist_assignment(&self) -> ApiFuture<'_, bool> {
        Box::pin(async { Ok(self.scene_home.is_some()) })
    }

    fn podcast_progress_revision(&self) -> Result<Option<String>> {
        Ok(self.progress_enabled.then(|| PROOF_REVISION.to_owned()))
    }

    fn upload_podcast_progress<'a>(
        &'a self,
        payload: Value,
        revision: &'a str,
        allowed: &'a PodcastProgressPermission,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            assert!(self.progress_enabled && revision == PROOF_REVISION);
            if !allowed()? {
                return Err(Error::StaleResponse);
            }
            assert!(payload.as_object().is_some_and(|body| body.len() == 3));
            assert!(matches!(
                payload["programId"].as_u64(),
                Some(501 | 503 | 504 | 506)
            ));
            assert!(payload["listenLocation"].as_f64().is_some_and(|position| {
                (0.0..=30_000.0).contains(&position) && position.fract() == 0.0
            }));
            assert!(payload["isListened"].is_boolean());
            self.progress_uploads
                .lock()
                .unwrap()
                .push(json!([PROGRESS, payload]));
            Ok(())
        })
    }

    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(voice) = &self.voice_detail {
                if let Some(response) = voice.profile(path, &body) {
                    return Ok(response);
                }
            }
            if let Some(scene) = &self.scene_home {
                return match scene.response(path, &body) {
                    Some(response) => {
                        if self.palette.is_some()
                            && matches!(
                                path,
                                "/api/voice/homepage/block/content" | "/api/dj/program/detail"
                            )
                        {
                            response.map(|response| {
                                let response = scene_palette_fixture::artwork_response(response);
                                if self.voice_detail.is_some() {
                                    voice_detail_fixture::enrich(response)
                                } else {
                                    response
                                }
                            })
                        } else {
                            response
                        }
                    }
                    None => {
                        scene_home_fixture::baseline(&self.scene_baseline, path, body, cache).await
                    }
                };
            }
            if path.contains("clear")
                || path.contains("submit")
                || matches!(
                    path,
                    PROGRESS | "/api/djradio/my/radio/recommend" | "/api/user/setting/update"
                )
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
                RECENT if self.progress_enabled => {
                    assert_eq!(body, json!({}));
                    let rows = [501, 503, 504, 506].map(voice);
                    json!({"code":200,"data":{"data":rows,"hasMore":false,"newVoiceCount":0}})
                }
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
                            || self.progress_enabled
                                && ids[0]
                                    .as_u64()
                                    .or_else(|| ids[0].as_str().and_then(|id| id.parse().ok()))
                                    .is_some_and(|id| (1..=40).contains(&id))
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

#[cfg(target_os = "linux")]
fn locale(category: libc::c_int) -> String {
    // SAFETY: null reads the initialized locale; copy the C string immediately.
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
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .unwrap();
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
    let progress_enabled = std::env::var_os("CUMULUS_PODCAST_PROGRESS_PROOF").is_some();
    let voice_detail_enabled = std::env::var_os("CUMULUS_VOICE_DETAIL_PROOF").is_some();
    let palette_case = std::env::var("CUMULUS_SCENE_FM_PALETTE_PROOF")
        .ok()
        .or_else(|| voice_detail_enabled.then(|| "header".to_owned()));
    let palette = palette_case
        .clone()
        .map(|case| Arc::new(scene_palette_fixture::PaletteMedia::new(case)));
    let palette_enabled = palette.is_some();
    let scene_home = std::env::var("CUMULUS_PODCAST_SCENE_HOME_PROOF")
        .ok()
        .or_else(|| palette_enabled.then(|| "automatic".to_owned()))
        .map(scene_home_fixture::SceneHome::new);
    let scene_enabled = scene_home.is_some();
    assert!(!(scene_enabled && progress_enabled));
    let api = Arc::new(Library {
        progress_enabled,
        scene_home,
        palette: palette.clone(),
        voice_detail: voice_detail_enabled.then(voice_detail_fixture::VoiceDetail::default),
        ..Default::default()
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let wave = relay_fixture::wave(&root).unwrap();
    let mut client = MusicClient::with_session(session, registry)
        .with_audio_factory(move || Ok(Box::new(relay_fixture::NativeAudio::open(wave.clone())?)));
    if let Some(palette) = palette {
        client = client.with_scene_fm_palette_backend(palette);
    }
    let mut context = assets::context();
    let mut config = context.config().app.windows[0].clone();
    config.width = 960.0;
    config.height = 640.0;
    context.config_mut().app.windows[0].create = false;
    let source = std::env::var_os("CUMULUS_UX_SCRIPT").map_or_else(
        || {
            if voice_detail_enabled {
                include_str!("voice_detail_ui_probe.js").to_owned()
            } else if palette_enabled {
                include_str!("scene_palette_ui_probe.js").to_owned()
            } else if scene_enabled {
                include_str!("podcast_scene_home_ui_probe.js").to_owned()
            } else if progress_enabled {
                include_str!("podcast_progress_ui_probe.js").to_owned()
            } else {
                include_str!("podcast_recent_ui_probe.js").to_owned()
            }
        },
        |path| fs::read_to_string(path).expect("explicit synthetic script"),
    );
    let source = if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
        capture_ack::script(&source)
    } else {
        source
    };
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            if progress_enabled || scene_enabled {
                fs::write(output.join("locale.json"), serde_json::to_vec(&json!({
                    "numeric":locale(libc::LC_NUMERIC),
                    "messages":locale(libc::LC_MESSAGES),
                    "characters":locale(libc::LC_CTYPE),
                }))?)?;
            }
            app.manage(client);
            app.add_capability(r#"{"identifier":"podcast-recent-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or("").to_owned();
                if progress_enabled && stage == "progress_poll" {
                    if let Some(nonce) = value["nonce"].as_u64() {
                        let count = api.progress_uploads.lock().unwrap().len();
                        handle.get_webview_window("main").unwrap().eval(format!(
                            "window.__podcastProgressCount = [{nonce}, {count}];",
                        )).unwrap();
                    }
                    return;
                }
                if !(["complete", "failed"].contains(&stage.as_str())
                    || stage.starts_with("ux_") && stage.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')) {
                    return;
                }
                if let Some(voice)=&api.voice_detail {
                    value["voiceDetailCalls"]=json!(voice.calls());
                }
                if let Some(palette) = &api.palette {
                    value["paletteRequests"] = palette.snapshot();
                }
                if let Some(scene) = &api.scene_home {
                    value["calls"] = json!(scene.calls());
                } else if stage == "complete" {
                    value["calls"] = json!(api.calls.lock().unwrap().clone());
                }
                if progress_enabled {
                    value["progressUploads"] = json!(api.progress_uploads.lock().unwrap().clone());
                }
                fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                let handle = handle.clone();
                let output = output.clone();
                tauri::async_runtime::spawn(async move {
                    if stage == "complete" || stage == "failed" {
                        handle.exit(if stage == "complete" {0} else {1});
                    } else if capture_ack::acknowledge(&handle, &output, &stage, true).await
                        && (progress_enabled || scene_enabled)
                    {
                        handle.get_webview_window("main").unwrap().eval(format!(
                            "window.__podcastProgressStage = {};",
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
                if palette_enabled {
                    view.eval(format!("window.__scenePaletteCase = {};", json!(palette_case))).unwrap();
                } else if scene_enabled {
                    let case = std::env::var("CUMULUS_PODCAST_SCENE_HOME_PROOF").unwrap();
                    view.eval(format!("window.__podcastSceneHomeCase = {};", json!(case))).unwrap();
                }
                view.eval(&source).unwrap();
            }
        })
        .run(context).expect("podcast recent proof");
}
