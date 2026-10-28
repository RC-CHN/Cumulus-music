//! Actual production close prompt + native tray, driven by a private synthetic
//! panel. Music requests use only an in-process synthetic fixture.
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
    client::MusicClient,
    playback::PlaybackQueue,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn main() {
    // SAFETY: before GUI and worker creation.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated root"));
    fs::create_dir(&root).expect("never overwrite evidence");
    let output = root.clone();
    let exit_output = root.clone();
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let mut queue = PlaybackQueue::new(Some(store.clone()), "2").unwrap();
    queue
        .replace(
            vec![json!({"id":2,"dt":30000,"song":{"id":2,"name":"合成托盘播放验证","dt":30000}})],
            0,
            json!({"kind":"fixture"}),
        )
        .unwrap();
    store.put("2", "playback", "position", &json!({"entry_id":queue.selected().unwrap().entry_id,"seconds":12.5,"state":"paused","timeline_version":1})).unwrap();
    let library = Arc::new(relay_fixture::Library::default());
    library.mode("empty");
    let session = Arc::new(AccountSession::new(library, Some(store.clone())));
    let wave = relay_fixture::wave(&root).unwrap();
    let client = MusicClient::with_session(session, registry)
        .with_audio_factory(move || Ok(Box::new(relay_fixture::NativeAudio::open(wave.clone())?)));
    let mut context = assets::context();
    let config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    let app=cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        app.manage(client);
        app.add_capability(r#"{"identifier":"tray-ui-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen("ux-proof",move |event| {
            let mut value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("").to_owned();
            if stage=="resize" {handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0)).unwrap();return;}
            if stage=="corrupt" {store.put("local-device","window-behavior-preferences-v1","current",&json!({"synthetic":"damaged"})).unwrap();return;}
            if ["reject-write","allow-write"].contains(&stage.as_str()) {
                let db=rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
                db.execute_batch(if stage=="reject-write" {"CREATE TRIGGER reject_window BEFORE INSERT ON state WHEN NEW.kind='window-behavior-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;"}else{"DROP TRIGGER reject_window;"}).unwrap();return;
            }
            if !stage.chars().all(|c|c.is_ascii_alphanumeric()||c=='_'){return;}
            value["lifecycle"]=serde_json::to_value(cumulus_desktop::lifecycle::view(&handle).unwrap()).unwrap();
            value["visible"]=json!(handle.get_webview_window("main").unwrap().is_visible().unwrap());
            let client=handle.state::<MusicClient>();let epoch=client.account().unwrap().epoch;
            value["preferences"]=serde_json::to_value(client.window_preferences(&epoch).unwrap().value).unwrap();
            if stage == "hidden_first" {
                let handle=handle.clone();let output=output.clone();
                tauri::async_runtime::spawn(async move {
                    let client=handle.state::<MusicClient>();
                    let updates=client.subscribe_playback(&epoch).await.unwrap();
                    let before=updates.current().unwrap();
                    tokio::time::sleep(std::time::Duration::from_millis(450)).await;
                    let after=updates.current().unwrap();
                    let hidden=handle.get_webview_window("main").unwrap().is_visible().ok()==Some(false);
                    let advance=after.position.unwrap_or_default()-before.position.unwrap_or_default();
                    let valid=hidden && before.state==cumulus_core::playback::controller::PlaybackStatus::Playing && after.state==cumulus_core::playback::controller::PlaybackStatus::Playing && before.play_id==after.play_id && advance>0.2;
                    value["backgroundPlayback"]=json!({"verified":valid,"advance":advance,"decoder":"libmpv","output":"null","samePlayId":before.play_id==after.play_id});
                    if !valid {value["ok"]=json!(false);value["error"]=json!("native background playback did not advance");}
                    fs::write(output.join(if valid {"hidden_first.json"} else {"failed.json"}),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                    if !valid {handle.exit(1);}
                });
                return;
            }
            fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if stage=="failed" {handle.exit(1);}
        });
        tauri::WebviewWindowBuilder::from_config(app,&config)?.data_directory(root.join("webview")).build()?;
        cumulus_desktop::lifecycle::install(app.handle())?;
        fs::write(root.join("ready.json"),serde_json::to_vec(&json!({"trayBuilt":app.tray_by_id("cumulus-main").is_some(),"visible":true})).unwrap())?;
        Ok(())
    }).on_page_load(|view,payload| {
        if payload.event()==PageLoadEvent::Finished && view.label()=="main" {view.eval(include_str!("tray_ui_probe.js")).unwrap();}
    }).build(context).unwrap();
    app.run(move |_, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            fs::write(exit_output.join("exited.json"), b"{\"nativeExit\":true}").unwrap();
        }
    });
}
