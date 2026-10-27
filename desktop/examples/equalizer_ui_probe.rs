//! Real production IPC/WebView plus libmpv, with generated media and no network.
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::LocalCommand},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
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
fn media(root: &Path) -> std::io::Result<()> {
    fs::create_dir(root.join("media"))?;
    let mut file = BufWriter::new(fs::File::create_new(root.join("media/均衡器验证.wav"))?);
    let rate = 48000u32;
    let frames = rate * 90;
    let bytes = frames * 2 * 4;
    for part in [
        b"RIFF".to_vec(),
        (bytes + 36).to_le_bytes().to_vec(),
        b"WAVEfmt ".to_vec(),
        16u32.to_le_bytes().to_vec(),
        3u16.to_le_bytes().to_vec(),
        2u16.to_le_bytes().to_vec(),
        rate.to_le_bytes().to_vec(),
        (rate * 8).to_le_bytes().to_vec(),
        8u16.to_le_bytes().to_vec(),
        32u16.to_le_bytes().to_vec(),
        b"data".to_vec(),
        bytes.to_le_bytes().to_vec(),
    ] {
        file.write_all(&part)?;
    }
    for frame in 0..frames {
        for freq in [440.0, 1357.0] {
            let v =
                ((frame as f64 * freq * std::f64::consts::TAU / rate as f64).sin() * 0.02) as f32;
            file.write_all(&v.to_le_bytes())?;
        }
    }
    file.flush()
}
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit proof root"));
    let reopen = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let missing = std::env::var_os("CUMULUS_PROOF_MISSING").is_some();
    if !reopen {
        fs::create_dir(&root).expect("new proof root");
        media(&root).unwrap();
    }
    let output = root.join(if reopen { "reopen" } else { "first" });
    fs::create_dir(&output).expect("new run output");
    let setup = root.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        let registry=ProfileRegistry::open(&setup.join("state"),ApiOrigin::Pc)?;
        let session=Arc::new(AccountSession::new(Arc::new(Offline),Some(registry.store())));
        let client=MusicClient::with_session(session,registry);
        let client=cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true);
        tauri::async_runtime::block_on(async {
            client.start_local_library().await?;
            if !reopen {client.local_command(LocalCommand::Watch {path:setup.join("media").to_string_lossy().into_owned()}).await?;}
            Ok::<(),cumulus_core::Error>(())
        })?;
        app.manage(client);
        if reopen {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"equalizer-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen("equalizer-proof",move|event|{
            let value:Value=serde_json::from_str(event.payload()).unwrap();let stage=value["stage"].as_str().unwrap_or("").to_owned();
            if !["eq_initial","eq_enabled","eq_custom","eq_off","eq_saved","eq_reopened","eq_reopened_playing","eq_missing","eq_disabled_recovery","complete","failed"].contains(&stage.as_str()) {return;}
            let handle=handle.clone();let output=output.clone();
            tauri::async_runtime::spawn(async move {
                let client=handle.state::<MusicClient>();
                let mut data=value.clone();
                if let Some(epoch)=value["epoch"].as_str() {
                    match client.subscribe_playback(epoch).await.and_then(|s|s.current()) {
                        Ok(state)=>{
                            let native=serde_json::to_value(state).unwrap();
                            let enabled=value["expect"]["enabled"].as_bool();
                            let gains=value["expect"]["gains"].as_array();
                            let valid=enabled.is_none_or(|v|native["equalizer"]["settings"]["enabled"]==v)
                                && gains.is_none_or(|g|g.iter().zip(native["equalizer"]["settings"]["gains"].as_array().unwrap()).all(|(a,b)|(a.as_f64().unwrap()-b.as_f64().unwrap()).abs()<1e-5));
                            data["native"]=native;
                            if !valid {data["native_mismatch"]=json!(true);fs::write(output.join("failed.json"),serde_json::to_vec_pretty(&data).unwrap()).unwrap();handle.exit(1);return;}
                        }
                        Err(error)=>{data["native_error"]=json!(error.to_string());fs::write(output.join("failed.json"),serde_json::to_vec_pretty(&data).unwrap()).unwrap();handle.exit(1);return;}
                    }
                }
                fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&data).unwrap()).unwrap();
                if stage=="complete" || stage=="failed" {handle.exit(if stage=="complete"{0}else{1});}
                else {handle.get_webview_window("main").unwrap().eval(format!("window.__eqAck={};",json!(stage))).unwrap();}
            });
        });Ok(())
    }).on_page_load(move|webview,payload|{
        if payload.event()==PageLoadEvent::Finished {
            webview.eval(format!("window.__eqProof={};",json!({"reopen":reopen,"missing":missing}))).unwrap();
            webview.eval(include_str!("equalizer_ui_probe.js")).unwrap();
        }
    }).run(tauri::generate_context!()).expect("equalizer UI proof");
}
