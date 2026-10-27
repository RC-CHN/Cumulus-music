//! Real production MV social and related-song UI. Explicit isolated proof directory.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn intent(root: &Path, id: &str) -> cumulus_core::Result<()> {
    use cumulus_core::{
        Error,
        account::{AccountSession, credentials::Credentials},
        mv::MvService,
        protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
        storage::StateStore,
    };
    let profile = PathBuf::from(std::env::var_os("CUMULUS_SESSION_DIR").expect("explicit profile"));
    tauri::async_runtime::block_on(async {
        let credentials = Arc::new(Credentials::open(
            Arc::new(StateStore::open(profile)?),
            ApiOrigin::Pc,
        )?);
        let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
        let session = Arc::new(AccountSession::new(
            api,
            Some(Arc::new(StateStore::open(root.join("guard"))?)),
        ));
        session.refresh().await?;
        if MvService::new(session.clone())?
            .like_state_for(&session.token()?, id)
            .await?
            .liked
        {
            return Err(Error::State("preserve already liked MV"));
        }
        let value = serde_json::json!({"owner":session.token()?.uid,"id":id,"before":false});
        let mut file = fs::File::create_new(root.join("restore-intent.json"))?;
        file.write_all(&serde_json::to_vec_pretty(&value)?)?;
        file.sync_all()?;
        Ok(())
    })
}
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof directory"));
    fs::create_dir(&root).expect("proof must not exist");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let input: serde_json::Value = serde_json::from_slice(
        &fs::read(PathBuf::from(
            std::env::var_os("CUMULUS_PROOF_SAMPLE").expect("explicit sample"),
        ))
        .unwrap(),
    )
    .unwrap();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    if !small {
        intent(&root, input["id"].as_str().expect("MV id")).expect("durable reversible intent");
    }
    let setup = root.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app|{
        let profile=PathBuf::from(std::env::var_os("CUMULUS_SESSION_DIR").expect("explicit profile"));
        let client=cumulus_core::client::MusicClient::open(&setup.join("state"),Some(&profile))?
          .with_audio_factory(||Ok(Box::new(cumulus_core::playback::engine::MpvEngine::open(cumulus_core::playback::engine::MpvConfig{null_output:true,..Default::default()})?)))
          .with_video_factory(Arc::new(||Ok(Box::new(cumulus_core::playback::engine::MpvVideoEngine::open(cumulus_core::playback::engine::MpvConfig{null_output:true,..Default::default()},640,360)?))));
        app.manage(client);if small{app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"mv-social-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();let output=setup.clone();app.listen("mv-social-proof",move |event|{
            let value:serde_json::Value=serde_json::from_str(event.payload()).expect("proof JSON");let stage=value["stage"].as_str().unwrap_or("");
            if !["social_detail","social_liked","social_restored","social_related","social_resumed","social_recommendation","complete","failed"].contains(&stage){return;}
            fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if stage=="complete"||stage=="failed"{handle.exit(if stage=="complete"{0}else{1});}
        });Ok(())
    }).on_page_load(move |webview,payload|{
        if payload.event()==PageLoadEvent::Finished{let input=serde_json::json!({"sample":input,"mutate":!small});webview.eval(format!("window.__mvSocialProof={input};")).unwrap();webview.eval(include_str!("mv_social_ui_probe.js")).unwrap();}
    }).run(tauri::generate_context!()).expect("run MV social proof");
}
