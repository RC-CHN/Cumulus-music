//! Explicit real-WebView integration probe; never part of startup.
//! Read-only by default; optional playlist proof mutates only new private resources.
//! cargo run -p cumulus-desktop --example ui_probe --features custom-protocol
//! Requires CUMULUS_DATA_DIR, CUMULUS_SESSION_DIR and CUMULUS_PROOF_DIR.
use std::{fs, path::PathBuf};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    let proof =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit proof directory"));
    fs::create_dir_all(&proof).expect("proof directory");
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let root = PathBuf::from(std::env::var_os("CUMULUS_DATA_DIR").expect("explicit app data"));
            let profile = PathBuf::from(std::env::var_os("CUMULUS_SESSION_DIR").expect("explicit existing session"));
            let mut client = cumulus_core::client::MusicClient::open(&root,Some(&profile))?;
            if std::env::var_os("CUMULUS_PROOF_DOWNLOAD").is_some() || std::env::var_os("CUMULUS_PROOF_CLOUD").is_some() || std::env::var_os("CUMULUS_PROOF_CLOUD_DOWNLOAD").is_some() {
                // Real libmpv decoding with explicit silent output in this test.
                client = client.with_audio_factory(|| Ok(Box::new(cumulus_core::playback::engine::MpvEngine::open(cumulus_core::playback::engine::MpvConfig { null_output: true, ..Default::default() })?)));
                if std::env::var_os("CUMULUS_PROOF_DOWNLOAD").is_some() || std::env::var_os("CUMULUS_PROOF_CLOUD_DOWNLOAD").is_some() {fs::create_dir(proof.join("media"))?;}
            }
            app.manage(client);
            if std::env::var_os("CUMULUS_PROOF_SMALL").is_some() {
                app.get_webview_window("main").expect("main window").set_size(tauri::LogicalSize::new(760.0,540.0))?;
            }
            // Event output is enabled only in this standalone test executable.
            app.add_capability(r#"{"identifier":"ui-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle=app.handle().clone();
            if std::env::var_os("CUMULUS_PROOF_CLOUD_FIXTURE").is_some() {
                let private_proof=proof.clone();
                app.listen("cloud-proof-private", move |event| {
                    let value: serde_json::Value=serde_json::from_str(event.payload()).expect("private cloud proof event");
                    fs::write(private_proof.join("private-cloud-recovery.json"),serde_json::to_vec_pretty(&value).unwrap()).expect("persist private recovery identity");
                });
            }
            app.listen("ui-proof",move |event| {
                let Ok(value)=serde_json::from_str::<serde_json::Value>(event.payload()) else {return;};
                let stage=value["stage"].as_str().unwrap_or("invalid");
                if !["daily","library","playlist","search","qr","playing","lyrics","queue","controls","playlist_create","playlist_edit","playlist_reorder","playlist_clear","playlist_cleanup","download_settings","cloud_download_options","download_picker","download_paused","download_complete","download_playing","download_rename","download_migrate","download_cleanup","cloud_uploaded","cloud_sort","cloud_search","cloud_playing","cloud_queue","cloud_lyrics","cloud_upload_dialog","cloud_picker","cloud_delete_cancel","cloud_tasks","cloud_roundtrip_picker","cloud_roundtrip_selected","cloud_roundtrip_uploaded","cloud_roundtrip_delete","cloud_roundtrip_cleanup","complete","failed"].contains(&stage){return;}
                fs::write(proof.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).expect("write proof");
                if stage=="complete" || stage=="failed" {handle.exit(if stage=="complete" {0} else {1});}
            });
            Ok(())
        })
        .on_page_load(|webview,payload| {
            if payload.event()==PageLoadEvent::Finished {
                if std::env::var_os("CUMULUS_PROOF_PLAYER").is_some(){
                    webview.eval(include_str!("player_probe.js")).expect("inject player probe");
                }
                if std::env::var_os("CUMULUS_PROOF_PLAYLIST").is_some(){
                    webview.eval(include_str!("playlist_probe.js")).expect("inject playlist probe");
                }
                if std::env::var_os("CUMULUS_PROOF_DOWNLOAD").is_some() || std::env::var_os("CUMULUS_PROOF_CLOUD_DOWNLOAD").is_some(){
                    let root=PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("proof directory"));
                    webview.eval(format!("window.__downloadProofRoot={};",serde_json::to_string(&root).expect("proof path"))).expect("inject download root");
                    if std::env::var_os("CUMULUS_PROOF_CLOUD_DOWNLOAD").is_some() {webview.eval("window.__cloudDownloadProof=true;").expect("inject cloud download mode");}
                    webview.eval(include_str!("download_probe.js")).expect("inject download probe");
                }
                if std::env::var_os("CUMULUS_PROOF_CLOUD").is_some(){
                    webview.eval(include_str!("cloud_probe.js")).expect("inject cloud probe");
                    if let Ok(fixture)=std::env::var("CUMULUS_PROOF_CLOUD_FIXTURE") {
                        let fixture: serde_json::Value=serde_json::from_str(&fixture).expect("explicit cloud fixture");
                        webview.eval(format!("window.__cloudProofFixture={};",serde_json::to_string(&fixture).unwrap())).expect("inject isolated fixture");
                        webview.eval(include_str!("cloud_mutation_probe.js")).expect("inject isolated cloud roundtrip");
                    }
                }
                webview.eval(include_str!("ui_probe.js")).expect("inject UI probe");
            }
        })
        .run(tauri::generate_context!())
        .expect("run real WebView probe");
}
