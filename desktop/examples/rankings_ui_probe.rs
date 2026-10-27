//! Production ranking UI, explicit existing account; reads and local playback only.
use cumulus_core::client::{MusicClient, dto::PlayerAction};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof root"));
    let profile = PathBuf::from(
        std::env::var_os("CUMULUS_RANKING_PROFILE").expect("explicit existing PC profile"),
    );
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    fs::create_dir(&root).expect("new proof root");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        let client=MusicClient::open(&root.join("state"),Some(&profile))?;
        let client=cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true);
        app.manage(client);
        if small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"ranking-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen("ranking-proof",move|event|{
            let value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("").to_owned();
            if !stage.starts_with("rank_") && !["complete","failed"].contains(&stage.as_str()) {return;}
            let handle=handle.clone();let output=output.clone();
            tauri::async_runtime::spawn(async move {
                let client=handle.state::<MusicClient>();let epoch=client.account().unwrap().epoch;
                let mut value=value;
                if stage=="rank_playing" || stage=="rank_album_playing" {
                    let player=client.subscribe_playback(&epoch).await.unwrap().current().unwrap();
                    value["nativePlayer"]=serde_json::to_value(&player).unwrap();
                    if player.position.is_none_or(|v|v<=0.2) || player.queue_len==0 || player.error.is_some() || value["tracks"].as_u64().is_some_and(|n|n as usize!=player.queue_len) {
                        value["ok"]=json!(false);fs::write(output.join("failed.json"),serde_json::to_vec_pretty(&value).unwrap()).unwrap();handle.exit(1);return;
                    }
                }
                if ["complete","failed"].contains(&stage.as_str()) {let _=client.player_action(&epoch,PlayerAction::Stop).await;}
                fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                if ["complete","failed"].contains(&stage.as_str()) {handle.exit(if stage=="complete"{0}else{1});}
                else {handle.get_webview_window("main").unwrap().eval(format!("window.__rankAck={};",json!(stage))).unwrap();}
            });
        });Ok(())
    }).on_page_load(|webview,payload|{if payload.event()==PageLoadEvent::Finished {webview.eval(include_str!("rankings_ui_probe.js")).unwrap();}})
      .run(tauri::generate_context!()).expect("ranking UI proof");
}
