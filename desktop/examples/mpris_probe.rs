//! Linux system-bus consumer proof: native Tauri/GTK + synthetic PCM/libmpv.
//! File commands exist only in this example and cannot enter production IPC.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
#[cfg(target_os = "linux")]
mod proof {
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
        time::Duration,
    };
    use tauri::Manager;
    fn write(path: &Path, value: &Value) {
        let next = path.with_extension("pending");
        std::fs::write(&next, serde_json::to_vec(value).unwrap()).unwrap();
        std::fs::rename(next, path).unwrap();
    }
    pub fn run() {
        // SAFETY: process entry before native worker or GTK initialization.
        unsafe {
            cumulus_desktop::prepare_graphics();
        }
        let root =
            PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
        std::fs::create_dir(&root).unwrap();
        let api = Arc::new(super::relay_fixture::Library::default());
        let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
            .unwrap();
        let wave = super::relay_fixture::wave(&root).unwrap();
        let engines = Arc::new(AtomicUsize::new(0));
        let opened = engines.clone();
        let client =
            MusicClient::with_session(session.clone(), registry).with_audio_factory(move || {
                let engine = super::relay_fixture::NativeAudio::open(wave.clone())?;
                opened.fetch_add(1, Ordering::AcqRel);
                Ok(Box::new(engine))
            });
        let mut context = tauri::generate_context!();
        context.config_mut().app.windows[0].create = false;
        cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
            app.manage(client);
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External("about:blank".parse().unwrap()))
                .data_directory(root.join("webview")).inner_size(760.0,540.0).build()?;
            cumulus_desktop::system_media::install(app.handle());
            let app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
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
                        write(&root.join("state.json"), &json!({"status":format!("{:?}",view.state),"position":view.position,"volume":view.volume,"speed":view.speed,"id":view.item.map(|i|i.id),"engines":engines.load(Ordering::Acquire),"visible":app.get_webview_window("main").unwrap().is_visible().unwrap()}));
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            });
            Ok(())
        }).run(context).unwrap();
    }
}
#[cfg(target_os = "linux")]
fn main() {
    proof::run();
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("MPRIS proof requires Linux; it does not validate Windows SMTC.");
    std::process::exit(78);
}
