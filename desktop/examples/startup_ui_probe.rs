//! Actual production UI/IPC and libmpv across process restarts. Every account,
//! HTTP response and media byte is synthetic; startup preferences use real SQLite.
#[path = "support/assets.rs"]
mod assets;
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
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::PlayerAction},
    playback::PlaybackQueue,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    // SAFETY: before GTK/runtime or any worker thread.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    let phase = std::env::var("CUMULUS_STARTUP_PHASE").expect("proof phase");
    assert!(
        [
            "initial",
            "autoplay-zero",
            "autoplay-resume",
            "repair",
            "repaired-reopen"
        ]
        .contains(&phase.as_str())
    );
    fs::create_dir_all(&root).unwrap();
    let output = root.join(&phase);
    fs::create_dir(&output).expect("never overwrite evidence");
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    if phase == "initial" {
        assert!(store.get("2", "playback", "queue").unwrap().is_none());
        let mut queue = PlaybackQueue::new(Some(store.clone()), "2").unwrap();
        queue
            .replace(
                vec![json!({"id":2,"dt":30000,"song":{"id":2,"name":"合成启动验证","dt":30000}})],
                0,
                json!({"kind":"fixture"}),
            )
            .unwrap();
        store.put("2", "playback", "position", &json!({"entry_id":queue.selected().unwrap().entry_id,"seconds":12.5,"state":"paused","timeline_version":1})).unwrap();
    }
    let api = Arc::new(relay_fixture::Library::default());
    api.mode("empty");
    let session = Arc::new(AccountSession::new(api, Some(store.clone())));
    let wave = relay_fixture::wave(&root).unwrap();
    let client = MusicClient::with_session(session, registry)
        .with_audio_factory(move || Ok(Box::new(relay_fixture::NativeAudio::open(wave.clone())?)));
    let mut context = assets::context();
    let config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    let page_phase = phase.clone();
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            app.manage(client);
            app.add_capability(r#"{"identifier":"startup-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let data = root.join("state/core.sqlite3");
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or("").to_owned();
                if stage == "resize" {
                    handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(value["width"].as_f64().unwrap(), value["height"].as_f64().unwrap())).unwrap();
                    return;
                }
                if stage == "corrupt" && phase == "repair" {
                    store.put("local-device", "playback-startup-preferences-v1", "current", &json!({"settings":"damaged synthetic data"})).unwrap();
                    return;
                }
                if ["reject-write", "allow-write"].contains(&stage.as_str()) && phase == "initial" {
                    let connection = rusqlite::Connection::open(&data).unwrap();
                    connection.execute_batch(if stage == "reject-write" {
                        "CREATE TRIGGER reject_startup BEFORE INSERT ON state WHEN NEW.kind='playback-startup-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;"
                    } else { "DROP TRIGGER reject_startup;" }).unwrap();
                    return;
                }
                if !(stage == "complete" || stage == "failed" || stage.starts_with("ux_") && stage.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) { return; }
                let handle = handle.clone(); let output = output.clone();
                tauri::async_runtime::spawn(async move {
                    let client = handle.state::<MusicClient>();
                    let epoch = client.account().unwrap().epoch;
                    value["nativePlayer"] = serde_json::to_value(client.subscribe_playback(&epoch).await.unwrap().current().unwrap()).unwrap();
                    value["preferences"] = serde_json::to_value(client.startup_preferences(&epoch).unwrap().value).unwrap();
                    if stage == "complete" || stage == "failed" {
                        // Explicitly persist the current native clock before exit.
                        let _ = client.player_action(&epoch, PlayerAction::Stop).await;
                    }
                    fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                    if stage == "complete" || stage == "failed" { handle.exit(if stage == "complete" {0} else {1}); }
                });
            });
            tauri::WebviewWindowBuilder::from_config(app, &config)?.data_directory(root.join("webview")).build()?;
            Ok(())
        })
        .on_page_load(move |view, payload| {
            if payload.event() == PageLoadEvent::Finished && view.label() == "main" {
                view.eval(format!("window.__CUMULUS_STARTUP_PHASE__={};", serde_json::to_string(&page_phase).unwrap())).unwrap();
                view.eval(include_str!("startup_ui_probe.js")).unwrap();
            }
        })
        .run(context).expect("startup proof");
}
