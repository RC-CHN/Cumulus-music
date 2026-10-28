//! Native tray protocol probe. No account, player, media or network API is opened.
//! The external private D-Bus host drives the actual exported menu callbacks.
#[path = "support/assets.rs"]
mod assets;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
fn main() {
    // SAFETY: process entry, before GTK and other threads.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    fs::create_dir(&root).expect("never overwrite evidence");
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut context = assets::context();
    let mut config = context.config().app.windows[0].clone();
    config.url = tauri::WebviewUrl::External("about:blank".parse().unwrap());
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        let window = tauri::WebviewWindowBuilder::from_config(app, &config)?
            .data_directory(root.join("webview")).build()?;
        let show = MenuItem::with_id(app,"proof-show","显示 Cumulus",true,None::<&str>)?;
        let hide = MenuItem::with_id(app,"proof-hide","隐藏窗口",true,None::<&str>)?;
        let quit = MenuItem::with_id(app,"proof-quit","退出验证",true,None::<&str>)?;
        let menu = Menu::with_items(app,&[&show,&hide,&quit])?;
        let report = root.clone();
        let tray = TrayIconBuilder::with_id("cumulus-proof")
            .icon(app.default_window_icon().expect("default icon").clone())
            .tooltip("Cumulus native tray proof")
            .menu(&menu)
            .on_menu_event(move |app,event| {
                let Some(window)=app.get_webview_window("main") else { return; };
                let id=event.id.as_ref();
                let result = match id {
                    "proof-show" => window.show().and_then(|_|window.unminimize()).and_then(|_|window.set_focus()),
                    "proof-hide" => window.hide(),
                    "proof-quit" => Ok(()),
                    _=>return,
                };
                events.lock().unwrap().push(id.into());
                let app = app.clone();
                let report = report.clone();
                let events = events.clone();
                let id = id.to_owned();
                tauri::async_runtime::spawn(async move {
                    // A window-manager request returning is not a visibility
                    // acknowledgement. Let the GUI process its native events.
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let value=json!({"event":id,"ok":result.is_ok(),"visible":window.is_visible().unwrap(),"events":events.lock().unwrap().clone(),"noAccountOrMediaInitialized":true});
                    fs::write(report.join(format!("{id}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                    if id=="proof-quit" { app.exit(0); }
                });
            }).build(app)?;
        fs::write(root.join("ready.json"),serde_json::to_vec_pretty(&json!({"trayBuilt":true,"id":tray.id().as_ref(),"visible":window.is_visible()?,"hostAvailableNotAssumed":true})).unwrap())?;
        // AppHandle retains the registered tray; dropping this clone must not
        // unregister it. The external host verifies its exported D-Bus object.
        Ok(())
    }).run(context).expect("native tray proof");
}
