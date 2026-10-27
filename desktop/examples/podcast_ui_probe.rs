//! Production WebView, typed IPC, live account reads and isolated local downloads.
//! No remote mutations. Explicit profile and a fresh private proof directory.
use std::{fs, path::PathBuf};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn main() {
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit proof directory"));
    let reopen = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    if !reopen {
        fs::create_dir(&root).expect("fresh proof directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let sample: serde_json::Value = serde_json::from_slice(
        &fs::read(PathBuf::from(
            std::env::var_os("CUMULUS_PROOF_SAMPLE").expect("explicit program sample"),
        ))
        .unwrap(),
    )
    .unwrap();
    let radio = sample["radio"]["id"]
        .to_string()
        .trim_matches('"')
        .to_owned();
    let voice = sample["id"].to_string().trim_matches('"').to_owned();
    let input = serde_json::json!({"radio":radio,"voice":voice,"root":root,"reopen":reopen});
    let setup = root.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
  let profile=PathBuf::from(std::env::var_os("CUMULUS_SESSION_DIR").expect("authorized PC profile"));
  let client=cumulus_core::client::MusicClient::open(&setup.join("state"),Some(&profile))?.with_audio_factory(||Ok(Box::new(cumulus_core::playback::engine::MpvEngine::open(cumulus_core::playback::engine::MpvConfig{null_output:true,..Default::default()})?)));
  app.manage(client);
  if reopen{app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
  app.add_capability(r#"{"identifier":"podcast-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
  let handle=app.handle().clone();app.listen("podcast-proof",move|event|{
   let value:serde_json::Value=serde_json::from_str(event.payload()).unwrap();let stage=value["stage"].as_str().unwrap_or("");
   if !["podcast_library","podcast_empty","podcast_detail","podcast_playing","podcast_comments","podcast_download","podcast_local","podcast_reopened","complete","failed"].contains(&stage){return;}
   let suffix=if reopen{"-reopened"}else{""};
   fs::write(setup.join(format!("{stage}{suffix}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
   if stage=="complete"||stage=="failed"{handle.exit(if stage=="complete"{0}else{1});}
  });Ok(())
 }).on_page_load(move|webview,payload|{
  if payload.event()==PageLoadEvent::Finished{webview.eval(format!("window.__podcastProof={input};")).unwrap();webview.eval(include_str!("podcast_ui_probe.js")).unwrap();}
 }).run(tauri::generate_context!()).expect("podcast UI proof");
}
