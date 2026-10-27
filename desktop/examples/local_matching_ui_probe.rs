//! Real account, production IPC and WebView. Only generated proof media is
//! scanned. The first metadata lookup is delayed to exercise pause/resume UI.
use cumulus_core::{
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::{MusicClient, dto::LocalCommand},
    playback::engine::{MpvConfig, MpvEngine},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Listener, Manager, webview::PageLoadEvent};
struct Delayed {
    inner: Arc<dyn Api>,
    first: AtomicBool,
}
impl Api for Delayed {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if path == "/api/search/match/new" && self.first.swap(false, Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
            }
            self.inner.request(path, payload, cache).await
        })
    }
}
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit proof root"));
    let profile = PathBuf::from(
        std::env::var_os("CUMULUS_SESSION_DIR").expect("explicit authorized PC profile"),
    );
    let source = PathBuf::from(
        std::env::var_os("CUMULUS_MATCH_FIXTURE").expect("explicit generated media directory"),
    );
    let reopen = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let sample: Value = serde_json::from_slice(
        &fs::read(
            source
                .parent()
                .expect("proof media parent")
                .join("proof-song.json"),
        )
        .expect("explicit proof song identity"),
    )
    .unwrap();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    if !reopen {
        fs::create_dir(&root).expect("new proof directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::create_dir(root.join("media")).unwrap();
        for file in ["plain.flac", "tagged.flac", "tagged.ncm"] {
            fs::copy(source.join(file), root.join("media").join(file)).unwrap();
        }
    }
    let setup = root.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        let registry=ProfileRegistry::open(&setup.join("state"),ApiOrigin::Pc)?;
        let credentials=Arc::new(Credentials::open(Arc::new(StateStore::open(&profile)?),ApiOrigin::Pc)?);
        let api=Arc::new(Delayed {inner:Arc::new(MusicHttpClient::new(credentials,ClientProfile::default())?),first:AtomicBool::new(!reopen)});
        let session=Arc::new(AccountSession::new(api,Some(registry.store())));
        let client=MusicClient::with_session(session,registry).with_audio_factory(||Ok(Box::new(MpvEngine::open(MpvConfig {null_output:true,..Default::default()})?)));
        tauri::async_runtime::block_on(client.start_local_library())?;
        if !reopen {tauri::async_runtime::block_on(client.local_command(LocalCommand::Watch{path:setup.join("media").to_string_lossy().into_owned()}))?;}
        app.manage(client);
        if small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"matching-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();app.listen("matching-proof",move|event|{
            let value:Value=serde_json::from_str(event.payload()).unwrap();let stage=value["stage"].as_str().unwrap_or("");
            if !["matching_paused","matching_auto","matching_search","matching_linked","matching_comments","matching_reopened","matching_playback","complete","failed"].contains(&stage){return;}
            let suffix=if reopen{"-reopened"}else{""};
            fs::write(setup.join(format!("{stage}{suffix}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if stage=="complete"||stage=="failed"{handle.exit(if stage=="complete"{0}else{1});}
        });Ok(())
    }).on_page_load(move|webview,payload|{
        if payload.event()==PageLoadEvent::Finished {
            webview.eval(format!("window.__matchingProof={};",json!({"reopen":reopen,"songId":sample["id"]}))).unwrap();
            webview.eval(include_str!("local_matching_ui_probe.js")).unwrap();
        }
    }).run(tauri::generate_context!()).expect("local matching UI proof");
}
