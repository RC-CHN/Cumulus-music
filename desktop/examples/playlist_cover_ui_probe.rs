//! Actual production cover DOM/IPC/native file picker with one isolated playlist.
mod cover_probe_support;
use cover_probe_support::{Proof, save};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("proof root"));
    let profile =
        PathBuf::from(std::env::var_os("CUMULUS_COVER_PROFILE").expect("explicit profile"));
    let reopen = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let hold = std::env::var_os("CUMULUS_COVER_HOLD").is_some();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    let cleanup_only = std::env::args().any(|v| v == "--cleanup");
    let output = root.join(if cleanup_only {
        "cleanup"
    } else if reopen {
        "reopen"
    } else {
        "first"
    });
    fs::create_dir(&output).expect("new evidence directory");
    let (proof, client) = tauri::async_runtime::block_on(Proof::open(
        root.clone(),
        output.clone(),
        &profile,
        reopen || cleanup_only,
    ))
    .expect("isolated cover setup; plan retained on failure");
    if cleanup_only {
        tauri::async_runtime::block_on(proof.cleanup()).expect("isolated cleanup");
        return;
    }
    let id = proof.plan["id"].clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        app.manage(cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true));
        if small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"cover-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle = app.handle().clone();
        app.listen("cover-proof",move |event| {
            let mut value: Value = serde_json::from_str(event.payload()).unwrap();
            let stage = value["stage"].as_str().unwrap_or("").to_owned();
            if !stage.starts_with("cover_") && !["complete","failed"].contains(&stage.as_str()) {return;}
            let proof=proof.clone(); let handle=handle.clone();
            tauri::async_runtime::spawn(async move {
                let observed = proof.observe(&mut value).await;
                if let Err(e)=observed {value["nativeError"]=json!(e.to_string());value["ok"]=json!(false);}
                let terminal = ["complete","failed"].contains(&stage.as_str());
                if terminal && !(hold && stage=="complete" && value["ok"]==true) {
                    let cleanup=proof.cleanup().await;
                    value["cleanupOk"]=json!(cleanup.is_ok());
                    if let Err(e)=cleanup {value["cleanupError"]=json!(e.to_string());value["ok"]=json!(false);}
                }
                // Copy a sanitized native journal independently of WebView DTOs.
                let owner=proof.plan["owner"].as_str().unwrap();
                let mut records=proof.store.values(owner,"playlist-covers-v1").unwrap();
                for row in records.values_mut() {for key in ["source","encoded"] {row[key]=json!(if row[key].as_str().is_some_and(|s|!s.is_empty()) {"<retained>"} else {""});} if row["allocation"].is_object(){row["allocation"]["token"]=json!("<redacted>");}}
                value["nativeJournal"]=json!(records);
                save(&proof.output.join(format!("{stage}.json")),&value).unwrap();
                if terminal {handle.exit(if stage=="complete"&&value["ok"]==true {0}else{1});}
                else {handle.get_webview_window("main").unwrap().eval(format!("window.__coverAck={};",json!(stage))).unwrap();}
            });
        });
        Ok(())
    }).on_page_load(move |webview,payload| {
        if payload.event()==PageLoadEvent::Finished {
            webview.eval(format!("window.__coverProof={{id:{id},reopen:{reopen},hold:{hold}}};")).unwrap();
            webview.eval(include_str!("playlist_cover_ui_probe.js")).unwrap();
        }
    }).run(tauri::generate_context!()).expect("cover UI proof");
}
