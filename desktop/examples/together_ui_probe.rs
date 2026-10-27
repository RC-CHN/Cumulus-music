//! Production WebView and libmpv; only a new isolated own room can be mutated.
#[path = "support/together.rs"]
mod support;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn finish(handle: tauri::AppHandle, harness: Arc<support::Harness>, ui_ok: bool, closed: bool) {
    tauri::async_runtime::spawn(async move {
        let restored = harness.restore().await;
        let client = handle.state::<cumulus_core::client::MusicClient>();
        let local = client.close_together().await;
        let binding = client.has_together_cleanup().unwrap_or(true);
        support::save(&harness.root,"host-final.json",&serde_json::json!({"ui_ok":ui_ok,"room_restored":restored.is_ok(),"local_restored":local.is_ok(),"binding":binding,"native_window_closed":closed})).unwrap();
        handle.exit(if ui_ok && restored.is_ok() && local.is_ok() && !binding {
            0
        } else {
            1
        });
    });
}
fn main() {
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit new proof directory"));
    let profile =
        PathBuf::from(std::env::var_os("CUMULUS_SESSION_DIR").expect("authorized PC profile"));
    let restore = std::env::args().any(|s| s == "--restore");
    if !restore {
        fs::create_dir(&root).expect("fresh root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let harness = Arc::new(
        tauri::async_runtime::block_on(support::Harness::open(&root, &profile, restore))
            .expect("isolated preflight"),
    );
    if restore {
        tauri::async_runtime::block_on(harness.restore()).expect("restore proof room");
        return;
    }
    let close = std::env::var_os("CUMULUS_PROOF_CLOSE").is_some();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    let im = std::env::var_os("CUMULUS_NIM_DIR").is_some();
    let input = serde_json::json!({"close":close,"small":small,"im":im});
    let ending = Arc::new(AtomicBool::new(false));
    let ready = Arc::new(AtomicBool::new(false));
    let setup = harness.clone();
    let setup_ending = ending.clone();
    let setup_ready = ready.clone();
    let app=cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        let client=cumulus_core::client::MusicClient::with_session(setup.session.clone(),setup.registry.clone()).with_audio_factory(||Ok(Box::new(cumulus_core::playback::engine::MpvEngine::open(cumulus_core::playback::engine::MpvConfig{null_output:true,..Default::default()})?)));
        let client=cumulus_desktop::configure_native_im(client,&app.path().resource_dir()?);
        app.manage(client);
        if small{app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.,540.))?;}
        app.add_capability(r#"{"identifier":"together-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();let guard=setup.clone();
        app.listen("together-proof",move|event|{
            let mut v:serde_json::Value=serde_json::from_str(event.payload()).unwrap();let name=v["stage"].as_str().unwrap_or("").to_owned();
            v["requestCounts"]=guard.guard.proof_counts();
            match name.as_str() {
                "inject-play-loss"=>{guard.guard.drop_play.store(true,Ordering::SeqCst);return;},
                "close-request"=>{if v["dropAck"]==true{guard.guard.drop_end.store(true,Ordering::SeqCst);}handle.get_webview_window("main").unwrap().close().unwrap();return;},
                "close-final-ready"=>{setup_ready.store(true,Ordering::SeqCst);},
                "together_im_connected"|"together_im_before_reconnect"|"together_im_reconnected"=>{},
                "together_empty"|"together_created"|"together_uncertain"|"together_reconciled"|"together_zero_seek"|"together_controls"|"together_queue"|"together_exit"|"together_close_failed"|"together_close_reconciled"|"complete"|"failed"=>{},
                _=>return,
            }
            support::save(&guard.root,&format!("{name}.json"),&v).unwrap();
            if matches!(name.as_str(),"complete"|"failed")&&!setup_ending.swap(true,Ordering::SeqCst){finish(handle.clone(),guard.clone(),name=="complete",false);}
        });Ok(())
    }).on_page_load(move|window,payload|{if payload.event()==PageLoadEvent::Finished{window.eval(format!("window.__togetherProof={input};")).unwrap();window.eval(include_str!("together_ui_probe.js")).unwrap();}}).build(tauri::generate_context!()).expect("production WebView");
    app.run(move |handle, event| {
        if let tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
            if !ending.swap(true, Ordering::SeqCst) {
                finish(
                    handle.clone(),
                    harness.clone(),
                    ready.load(Ordering::SeqCst),
                    true,
                );
            }
        }
    });
}
