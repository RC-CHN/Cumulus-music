//! Production shell paint/layout inspection without account or media traffic.
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    playback::engine::{MpvConfig, MpvEngine},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};
struct Offline;
impl Api for Offline {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/w/nuser/account/get");
            Ok(json!({"code":301}))
        })
    }
}
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof directory"));
    fs::create_dir(&root).expect("proof directory must be new");
    let setup = root.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        let registry=ProfileRegistry::open(&setup.join("state"),ApiOrigin::Pc)?;
        let session=Arc::new(AccountSession::new(Arc::new(Offline),Some(registry.store())));
        let client=MusicClient::with_session(session,registry).with_audio_factory(||Ok(Box::new(MpvEngine::open(MpvConfig {null_output:true,..Default::default()})?)));
        app.manage(client);
        app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;
        app.add_capability(r#"{"identifier":"shell-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();app.listen("shell-proof",move|event|{
            let value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("");
            if stage=="resize" {let width=value["width"].as_f64().unwrap();let height=value["height"].as_f64().unwrap();
                handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(width,height)).unwrap();return;}
            if !["shell_initial","shell_local","shell_wide","shell_narrow","shell_repaint","complete","failed"].contains(&stage){return;}
            fs::write(setup.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if stage=="complete"||stage=="failed"{handle.exit(if stage=="complete"{0}else{1});}
        });Ok(())
    }).on_page_load(|webview,payload|{
        if payload.event()==PageLoadEvent::Finished{webview.eval(include_str!("shell_ui_probe.js")).unwrap();}
    }).run(tauri::generate_context!()).expect("shell proof");
}
