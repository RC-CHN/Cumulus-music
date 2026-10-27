//! Explicit real WebView MV proof. All media belongs to a new proof directory.
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn collection_intent(proof: &std::path::Path, id: &str) -> cumulus_core::Result<Vec<String>> {
    use cumulus_core::{
        account::{
            AccountSession,
            credentials::Credentials,
            resources::{AccountResources, ResourceKind},
        },
        protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
        storage::StateStore,
    };
    let profile =
        PathBuf::from(std::env::var_os("CUMULUS_SESSION_DIR").expect("explicit PC profile"));
    tauri::async_runtime::block_on(async {
        let credentials = Arc::new(Credentials::open(
            Arc::new(StateStore::open(&profile)?),
            ApiOrigin::Pc,
        )?);
        let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
        let session = Arc::new(AccountSession::new(
            api,
            Some(Arc::new(StateStore::open(proof.join("guard"))?)),
        ));
        session.refresh().await?;
        let rows = AccountResources::new(session.clone())?
            .refresh(ResourceKind::Mvs)
            .await?;
        let ids = rows
            .as_array()
            .ok_or(cumulus_core::Error::Response("MV baseline missing"))?
            .iter()
            .map(|row| cumulus_core::protocol::api::identifier(&row["id"]))
            .collect::<cumulus_core::Result<Vec<_>>>()?;
        if ids.iter().any(|v| v == id) {
            return Err(cumulus_core::Error::State(
                "preserve original MV membership",
            ));
        }
        let intent = serde_json::json!({"owner":session.token()?.uid,"id":id,"originally_present":false,"baseline_ids":ids});
        use std::io::Write;
        let mut file = fs::File::create_new(proof.join("restore-intent.json"))?;
        file.write_all(&serde_json::to_vec_pretty(&intent)?)?;
        file.sync_all()?;
        Ok(ids)
    })
}
fn main() {
    let proof = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof directory"));
    fs::create_dir(&proof).expect("proof must not already exist");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&proof, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::create_dir(proof.join("media")).unwrap();
    fs::create_dir(proof.join("media/relocated")).unwrap();
    let sample =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_SAMPLE_DETAIL").expect("explicit MV sample"));
    let detail: serde_json::Value = serde_json::from_slice(&fs::read(sample).unwrap()).unwrap();
    let id = detail["data"]["id"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| detail["data"]["id"].as_u64().map(|n| n.to_string()))
        .expect("sample id");
    let setup_proof = proof.clone();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    let collect = std::env::var_os("CUMULUS_PROOF_COLLECT").is_some();
    let baseline = if collect {
        Some(
            collection_intent(&proof, &id).expect("read-only preflight and durable restore intent"),
        )
    } else {
        None
    };
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let profile=PathBuf::from(std::env::var_os("CUMULUS_SESSION_DIR").expect("explicit authorized PC session"));
            let client=cumulus_core::client::MusicClient::open(&setup_proof.join("state"),Some(&profile))?
                .with_audio_factory(||Ok(Box::new(cumulus_core::playback::engine::MpvEngine::open(cumulus_core::playback::engine::MpvConfig{null_output:true,..Default::default()})?)))
                .with_video_factory(Arc::new(||Ok(Box::new(cumulus_core::playback::engine::MpvVideoEngine::open(cumulus_core::playback::engine::MpvConfig{null_output:true,..Default::default()},640,360)?))));
            app.manage(client);
            if small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
            app.add_capability(r#"{"identifier":"mv-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle=app.handle().clone();
            let output=setup_proof.clone();
            app.listen("mv-proof",move |event|{
                let value:serde_json::Value=serde_json::from_str(event.payload()).expect("proof JSON");
                let stage=value["stage"].as_str().unwrap_or("");
                if !["mv_collection","mv_playing","mv_controls","mv_fullscreen","mv_mini","mv_focus","mv_download","mv_local","mv_renamed","mv_moved","mv_migrated","mv_cleanup","mv_subscribed","mv_unsubscribed","complete","failed"].contains(&stage) {return;}
                fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                if stage=="complete" || stage=="failed" {handle.exit(if stage=="complete"{0}else{1});}
            });
            Ok(())
        })
        .on_page_load(move |webview,payload|{
            if payload.event()==PageLoadEvent::Finished {
                let input=serde_json::json!({"id":id,"root":proof,"download":!small&&!collect,"collect":collect,"baseline":baseline});
                webview.eval(format!("window.__mvProof={input};")).unwrap();
                webview.eval(include_str!("mv_ui_probe.js")).unwrap();
            }
        })
        .run(tauri::generate_context!()).expect("run MV WebView proof");
}
