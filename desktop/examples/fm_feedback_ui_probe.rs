//! Real, isolated reversible FM song/artist feedback, through the production UI.
mod fm_feedback_support;
use cumulus_core::client::{
    MusicClient,
    dto::{FmSource, PlayerAction},
};
use fm_feedback_support::{Proof, save};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("proof root"));
    let profile = PathBuf::from(std::env::var_os("CUMULUS_FM_PROFILE").expect("explicit profile"));
    let restore = std::env::args().any(|arg| arg == "--restore");
    let lose = std::env::var_os("CUMULUS_FM_LOSE_ARTIST_REPLY").is_some();
    if !restore {
        fs::create_dir(&root).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let output = root.join(if restore { "recovery" } else { "first" });
    fs::create_dir(&output).unwrap();
    let (proof, client) = Proof::open(&root, &output, &profile, lose, restore).unwrap();
    if restore {
        let result = tauri::async_runtime::block_on(proof.cleanup());
        println!("{}", json!({"cleanup_ok":result.is_ok()}));
        if result.is_err() {
            std::process::exit(1);
        }
        return;
    }
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        app.manage(cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true));
        if std::env::var_os("CUMULUS_PROOF_SMALL").is_some(){app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"fm-feedback-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen("fm-feedback-proof",move|event|{
            let mut value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("").to_owned();
            if !stage.starts_with("feedback_")&&!["complete","failed"].contains(&stage.as_str()){return;}
            let handle=handle.clone();let output=output.clone();let proof=proof.clone();
            tauri::async_runtime::spawn(async move{
                let client=handle.state::<MusicClient>();
                let epoch=client.account().unwrap().epoch;
                let observed=async{
                    let player=client.subscribe_playback(&epoch).await?.current()?;
                    if stage=="feedback_ready" {
                        let plan=proof.prepare(player.entry_id.as_deref().ok_or(cumulus_core::Error::State("proof current entry missing"))?).await?;
                        value["target"]=json!({"songId":plan.song_id,"artistId":plan.artist_id,"reasonKey":plan.reason_key});
                        handle.get_webview_window("main").unwrap().eval(format!("window.__feedbackTarget={};",value["target"])).unwrap();
                    }
                    value["nativePlayer"]=serde_json::to_value(player)?;
                    value["nativeFm"]=serde_json::to_value(client.fm_view(&epoch,FmSource::Personal,false).await?)?;
                    value["observed"]=proof.observe().await?;
                    Ok::<_,cumulus_core::Error>(())
                }.await;
                let terminal=["complete","failed"].contains(&stage.as_str())||observed.is_err();
                if let Err(e)=observed {value["ok"]=json!(false);value["nativeError"]=json!(e.to_string());}
                if terminal {
                    let _=client.player_action(&epoch,PlayerAction::Pause{paused:true}).await;
                    let cleanup=proof.cleanup().await;
                    value["cleanupOk"]=json!(cleanup.is_ok());
                    if let Err(e)=cleanup {value["ok"]=json!(false);value["cleanupError"]=json!(e.to_string());}
                }
                let failed=terminal && (stage!="complete" || value["ok"]!=true);
                save(&output.join(format!("{}.json",if failed {"failed"} else {&stage})),&value).unwrap();
                if terminal {handle.exit(if failed{1}else{0});}
                else {handle.get_webview_window("main").unwrap().eval(format!("window.__feedbackAck={};",json!(stage))).unwrap();}
            });
        });Ok(())
    }).on_page_load(|webview,payload|{if payload.event()==PageLoadEvent::Finished{webview.eval(include_str!("fm_feedback_ui_probe.js")).unwrap();}})
    .run(tauri::generate_context!()).expect("FM feedback proof");
}
