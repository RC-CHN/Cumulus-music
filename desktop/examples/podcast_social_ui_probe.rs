//! Live isolated reversible subscription, voice-like and resume setting proof.
#[path = "support/podcast_social.rs"]
mod support;
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit proof root"));
    let profile =
        PathBuf::from(std::env::var_os("CUMULUS_SESSION_DIR").expect("authorized PC profile"));
    let restore = std::env::args().any(|v| v == "--restore");
    if !restore {
        fs::create_dir(&root).expect("fresh proof root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let radio = std::env::var("CUMULUS_RADIO_ID").unwrap_or_default();
    let sample: serde_json::Value = if !restore {
        serde_json::from_slice(
            &fs::read(PathBuf::from(
                std::env::var_os("CUMULUS_PROOF_SAMPLE").expect("program sample"),
            ))
            .unwrap(),
        )
        .unwrap()
    } else {
        serde_json::Value::Null
    };
    let voice = sample["id"].to_string().trim_matches('"').to_owned();
    let resume_radio = sample["radio"]["id"]
        .to_string()
        .trim_matches('"')
        .to_owned();
    let harness = Arc::new(
        tauri::async_runtime::block_on(support::Harness::open(
            &root,
            &profile,
            &radio,
            &voice,
            &resume_radio,
            restore,
        ))
        .expect("durable isolated preflight"),
    );
    if restore {
        tauri::async_runtime::block_on(harness.restore()).expect("restore baseline");
        return;
    }
    let input = serde_json::json!({"radio":harness.plan.radio,"voice":harness.plan.voice,"resumeRadio":harness.plan.resume_radio,"resumeBefore":harness.plan.resume_before,"dropAck":std::env::var_os("CUMULUS_PROOF_DROP_ACK").is_some()});
    let state = harness.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
  let client=cumulus_core::client::MusicClient::with_session(state.session.clone(),state.registry.clone()).with_audio_factory(||Ok(Box::new(cumulus_core::playback::engine::MpvEngine::open(cumulus_core::playback::engine::MpvConfig{null_output:true,..Default::default()})?)));
  app.manage(client);app.add_capability(r#"{"identifier":"podcast-social-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
  let handle=app.handle().clone();let output=state.root.clone();let guard=state.clone();let ending=std::sync::atomic::AtomicBool::new(false);app.listen("podcast-social-proof",move|event|{
   let v:serde_json::Value=serde_json::from_str(event.payload()).unwrap();let name=v["stage"].as_str().unwrap_or("");
   if !["social_subscribed","social_unsubscribed","social_liked","social_like_uncertain","social_unliked","social_resume_on","social_resume_position","social_resume_off","complete","failed"].contains(&name){return;}
   support::save(&output,&format!("{name}.json"),&v).unwrap();
   if (name=="complete"||name=="failed")&&!ending.swap(true,std::sync::atomic::Ordering::SeqCst){
    let guard=guard.clone();let handle=handle.clone();let ui_ok=name=="complete";
    tauri::async_runtime::spawn(async move {
     let restored=guard.restore().await;
     if let Err(error)=&restored {support::save(&guard.root,"restore-error.json",&serde_json::json!({"restore_complete":false,"error":error.to_string()})).unwrap();}
     support::save(&guard.root,"host-final.json",&serde_json::json!({"ui_ok":ui_ok,"baseline_restored":restored.is_ok()})).unwrap();
     handle.exit(if ui_ok&&restored.is_ok(){0}else{1});
    });
   }
  });Ok(())
 }).on_page_load(move|window,payload|{if payload.event()==PageLoadEvent::Finished{window.eval(format!("window.__podcastSocial={input};")).unwrap();window.eval(include_str!("podcast_social_ui_probe.js")).unwrap();}}).run(tauri::generate_context!()).expect("production WebView");
}
