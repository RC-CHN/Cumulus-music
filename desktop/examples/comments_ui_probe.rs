//! Production comments UI proof with private, durable reversible-like intent.
use std::{fs, io::Write, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn intent(root: &std::path::Path, sample: &serde_json::Value) -> cumulus_core::Result<()> {
    use cumulus_core::{
        Error,
        account::{AccountSession, credentials::Credentials},
        comments::{CommentResource, Comments},
        protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
        storage::StateStore,
    };
    let profile =
        PathBuf::from(std::env::var_os("CUMULUS_SESSION_DIR").expect("explicit PC profile"));
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
        let resource: CommentResource = serde_json::from_value(sample["resource"].clone())?;
        let id = sample["comment_id"]
            .as_str()
            .ok_or(Error::InvalidInput("explicit candidate missing"))?;
        if Comments::new(session.clone())?
            .target_for(&session.token()?, &resource, id)
            .await?
            .liked
            != Some(false)
        {
            return Err(Error::State("preserve already liked comment"));
        }
        let value = serde_json::json!({"owner":session.token()?.uid,"resource":resource,"comment_id":id,"before_liked":false});
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
    let sample: serde_json::Value = serde_json::from_slice(
        &fs::read(PathBuf::from(
            std::env::var_os("CUMULUS_PROOF_SAMPLE").expect("explicit comment sample"),
        ))
        .unwrap(),
    )
    .unwrap();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    if !small {
        intent(&root, &sample).expect("read-only preflight and durable intent");
    }
    let setup = root.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app|{
   let profile=PathBuf::from(std::env::var_os("CUMULUS_SESSION_DIR").expect("explicit PC profile"));
   let client=cumulus_core::client::MusicClient::open(&setup.join("state"),Some(&profile))?
     .with_audio_factory(||Ok(Box::new(cumulus_core::playback::engine::MpvEngine::open(cumulus_core::playback::engine::MpvConfig{null_output:true,..Default::default()})?)))
     .with_video_factory(Arc::new(||Ok(Box::new(cumulus_core::playback::engine::MpvVideoEngine::open(cumulus_core::playback::engine::MpvConfig{null_output:true,..Default::default()},640,360)?))));
   app.manage(client);if small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
   app.add_capability(r#"{"identifier":"comments-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
   let handle=app.handle().clone();let output=setup.clone();
   app.listen("comments-proof",move |event|{
     let value:serde_json::Value=serde_json::from_str(event.payload()).expect("proof JSON");let stage=value["stage"].as_str().unwrap_or("");
     if !["comments_latest","comments_next","comments_hot","comments_target","comments_liked","comments_restored","comments_playlist","comments_mv","complete","failed"].contains(&stage){return;}
     fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
     if stage=="complete"||stage=="failed"{handle.exit(if stage=="complete"{0}else{1});}
   });Ok(())
 }).on_page_load(move |webview,payload|{
   if payload.event()==PageLoadEvent::Finished {let input=serde_json::json!({"sample":sample,"mutate":!small});webview.eval(format!("window.__commentsProof={input};")).unwrap();webview.eval(include_str!("comments_ui_probe.js")).unwrap();}
 }).run(tauri::generate_context!()).expect("run comments proof");
}
