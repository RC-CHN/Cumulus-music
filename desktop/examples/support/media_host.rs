// Shared synthetic host for native system-media protocol consumers.
// Only these examples accept file commands. Production has no test IPC.
#[path = "media_engine.rs"]
mod observed;
use cumulus_core::protocol::api::{Api, ApiFuture};
#[derive(Default)]
struct MediaLibrary(super::relay_fixture::Library);
impl Api for MediaLibrary {
    fn local_device_id(&self) -> cumulus_core::Result<Option<String>> {
        self.0.local_device_id()
    }
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let value = self.0.request(path, args, cache).await?;
            #[cfg(target_os = "windows")]
            let value = {
                let mut value = value;
                if path == "/api/v3/song/detail"
                    && let Some(songs) = value["songs"].as_array_mut()
                {
                    for song in songs {
                        let title = song["name"].as_str().unwrap_or("");
                        song["name"] =
                            json!(format!("CumulusSMTCProof-{}: {title}", std::process::id()));
                    }
                }
                value
            };
            Ok(value)
        })
    }
}
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{PlaySelection, PlayerAction},
    },
    membership::Quality,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Manager;
fn write(path: &Path, value: &Value) {
    let next = path.with_extension("pending");
    std::fs::write(&next, serde_json::to_vec(value).unwrap()).unwrap();
    std::fs::rename(next, path).unwrap();
}
pub fn run() {
    let started = Instant::now();
    eprintln!("native system media proof: process_entry");
    // SAFETY: process entry before native worker or GTK initialization.
    unsafe {
        cumulus_desktop::prepare_graphics();
    }
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    std::fs::create_dir(&root).unwrap();
    let progress_root = root.clone();
    let progress = move |stage: &str| {
        let value = json!({"stage":stage,"elapsedMs":started.elapsed().as_millis()});
        eprintln!("native system media proof: {value}");
        write(&progress_root.join("progress.json"), &value);
    };
    progress("before_registry");
    let api = Arc::new(MediaLibrary::default());
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    progress("registry_opened");
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    progress("synthetic_account_ready");
    let wave = super::relay_fixture::wave(&root).unwrap();
    progress("pcm_generated");
    let engines = Arc::new(AtomicUsize::new(0));
    let opened = engines.clone();
    let measured = observed::Probe::default();
    let measured_engine = measured.clone();
    let client =
        MusicClient::with_session(session.clone(), registry).with_audio_factory(move || {
            let engine = super::relay_fixture::NativeAudio::open(wave.clone())?;
            opened.fetch_add(1, Ordering::AcqRel);
            Ok(Box::new(observed::Observed::new(
                engine,
                measured_engine.clone(),
            )))
        });
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows[0].create = false;
    progress("before_gui_run");
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        progress("gui_setup");
        app.manage(client);
        tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External("about:blank".parse().unwrap()))
            .data_directory(root.join("webview")).inner_size(760.0,540.0).build()?;
        progress("window_created");
        cumulus_desktop::system_media::install(app.handle());
        progress("system_media_installed");
        let app = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            progress("worker_ready");
            write(&root.join("ready.json"), &json!({"pid":std::process::id(),"service":format!("org.mpris.MediaPlayer2.Cumulus.instance{}",std::process::id())}));
            loop {
                let client = app.state::<MusicClient>();
                let epoch = client.account().unwrap().epoch;
                let request = root.join("command.json");
                if request.exists() {
                    let value: Value = serde_json::from_slice(&std::fs::read(&request).unwrap()).unwrap();
                    std::fs::remove_file(request).unwrap();
                    let id = value["id"].as_u64().unwrap();
                    let kind = value["kind"].as_str().unwrap();
                    let result: cumulus_core::Result<()> = match kind {
                        "start" => client.play_selection(&epoch, PlaySelection::Songs { ids:vec!["2".into(),"3".into()],index:0,quality:Quality::Standard }).await.map(|_|()),
                        "seek" => client.player_action(&epoch, PlayerAction::Seek {seconds:9.0}).await.map(|_|()),
                        "short_seek" => client.player_action(&epoch, PlayerAction::Seek {seconds:4.01}).await.map(|_|()),
                        "clear" => client.player_action(&epoch, PlayerAction::Clear).await.map(|_|()),
                        "media_off" | "media_on" => {
                            let value=cumulus_desktop::system_media::settings::read(&app,&epoch).unwrap().value;
                            cumulus_desktop::system_media::settings::change(&app,&epoch,cumulus_core::client::dto::SystemMediaPreferencesCommand::Change{expected_revision:value.revision.unwrap(),enabled:kind=="media_on"}).map(|_|())
                        }
                        "media_damage" => {
                            store.put("local-device","system-media-preferences-v1","current",&json!({"enabled":"invalid"})).unwrap();
                            cumulus_desktop::system_media::settings::read(&app,&epoch).map(|_|())
                        }
                        "media_repair" => cumulus_desktop::system_media::settings::change(&app,&epoch,cumulus_core::client::dto::SystemMediaPreferencesCommand::Repair).map(|_|()),
                        "media_reject" | "media_allow" => {
                            let sql=rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
                            sql.execute_batch(if kind=="media_reject" {"CREATE TRIGGER reject_media BEFORE INSERT ON state WHEN NEW.kind='system-media-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic media setting failure'); END;"} else {"DROP TRIGGER reject_media;"}).unwrap();
                            Ok(())
                        }
                        "switch" => session.adopt(api.clone(), json!({"userId":3}), &session.token().unwrap()).map(|_|()),
                        "reject" | "allow" => {
                            let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
                            sql.execute_batch(if kind=="reject" {"CREATE TRIGGER reject_mpris BEFORE INSERT ON state WHEN NEW.kind='playback' AND NEW.key='preferences' BEGIN SELECT RAISE(ABORT,'synthetic media preference failure'); END;"} else {"DROP TRIGGER reject_mpris;"}).unwrap();
                            Ok(())
                        }
                        "hide" => {app.get_webview_window("main").unwrap().hide().unwrap(); Ok(())}
                        "exit" => { write(&root.join(format!("ack-{id}.json")), &json!({"ok":true})); app.exit(0); break; }
                        _ => Err(cumulus_core::Error::InvalidInput("unknown proof control")),
                    };
                    write(&root.join(format!("ack-{id}.json")), &json!({"ok":result.is_ok()}));
                }
                if let Ok(view) = client.native_media_details() {
                    write(&root.join("state.json"), &json!({"status":format!("{:?}",view.state),"position":view.position,"volume":view.volume,"speed":view.speed,"id":view.item.map(|i|i.id),"mode":view.mode,"engines":engines.load(Ordering::Acquire),"nativeSeeks":measured.lock().unwrap().clone(),"visible":app.get_webview_window("main").unwrap().is_visible().unwrap()}));
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
        Ok(())
    }).run(context).unwrap();
}
