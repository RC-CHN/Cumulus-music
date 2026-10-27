//! Real production WebView/IPC/core; deterministic API, IM and audio fixtures.
//! This is a local UI proof and must never be labeled a live peer/audio proof.
#[allow(dead_code, unused_imports)]
#[path = "../../core/tests/client_together.rs"]
mod fixture;
use cumulus_core::listen_together::{RoomMessage, native_im::ImEvent};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn send(im: &fixture::im::Device, kind: u64, content: Value) {
    let message = RoomMessage::from_attachment(
        &json!({"content":{"type":kind,"content":content}}).to_string(),
    )
    .unwrap()
    .unwrap();
    im.controls
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .send(ImEvent::Business {
            messages: vec![message],
        })
        .unwrap();
}
fn rights(id: &str, reason: &str) -> Value {
    json!({"songId":id,"playable":reason.is_empty(),"unplayableType":reason,"unplayableUserIds":if reason.is_empty(){json!([])}else{json!([2])},"unplayableOuterIds":[],"start":40,"end":50})
}
fn main() {
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new explicit fixture root"));
    std::fs::create_dir(&root).expect("fresh root");
    let api = Arc::new(fixture::Backend::default());
    api.shell.store(true, Ordering::SeqCst);
    let (_, client) = fixture::world(&root.join("state"), api.clone());
    let im = Arc::new(fixture::im::Device::default());
    let client = client.with_together_im_factory(Arc::new(fixture::im::Factory(im.clone())));
    let ending = Arc::new(AtomicBool::new(false));
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        app.manage(client);
        if std::env::var_os("CUMULUS_PROOF_SMALL").is_some(){app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.,540.))?;}
        app.add_capability(r#"{"identifier":"shared-queue-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen("shared-queue-proof",move|event|{
            let value:Value=serde_json::from_str(event.payload()).unwrap();let stage=value["stage"].as_str().unwrap_or("");
            match stage {
                "inject-peer"=>{api.state.lock().unwrap().peer=true;send(&im,20002,json!({"roomId":"test-room"}));return;},
                "inject-queue"=>{
                    let mut state=api.state.lock().unwrap();
                    state.playlist=Some(json!({"displayList":{"result":["91","94","95"],"changed":true},"randomList":{"result":["95","91","94"],"changed":true},"version":[{"userId":1,"version":20}],"playMode":"RANDOM","replace":true}));
                    state.play=Some(json!({"roomId":"test-room","commandType":"PAUSE","progress":47000,"playStatus":"PAUSE","formerSongId":"91","targetSongId":"94","serverSeq":20,"clientSeq":20,"userId":2}));drop(state);
                    *api.common.lock().unwrap()=Some(vec![rights("91","vip"),rights("94","trial"),rights("95","")]);
                    send(&im,20001,json!({"roomId":"test-room","version":[{"userId":1,"version":20}]}));return;
                },
                "inject-blocked"=>{
                    *api.common.lock().unwrap()=Some(vec![rights("91","vip"),rights("94","vip"),rights("95","vip")]);
                    send(&im,20001,json!({"roomId":"test-room","version":[{"userId":1,"version":20}]}));return;
                },
                "inject-preview-only"=>{
                    api.state.lock().unwrap().play=Some(json!({"roomId":"test-room","commandType":"PAUSE","progress":47000,"playStatus":"PAUSE","formerSongId":"91","targetSongId":"91","serverSeq":200,"clientSeq":200,"userId":2}));
                    *api.common.lock().unwrap()=Some(vec![rights("91","trial"),rights("92",""),rights("93","")]);
                    send(&im,20012,json!({"roomId":"test-room"}));return;
                },
                "inject-end"|"inject-lost-end"=>{
                    api.drop_end.store(stage=="inject-lost-end",Ordering::SeqCst);
                    send(&im,20003,json!({"roomId":"test-room"}));return;
                },
                "shared_preview"|"shared_blocked"|"shared_exit"=>{},
                "lifecycle_preview"|"lifecycle_auto"|"lifecycle_pending"|"lifecycle_recovered"=>{},
                "complete"|"failed"=>{},
                _=>return,
            }
            std::fs::write(root.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if matches!(stage,"complete"|"failed")&&!ending.swap(true,Ordering::SeqCst){
                let handle=handle.clone();let root=root.clone();let ok=stage=="complete";let api=api.clone();let im=im.clone();
                tauri::async_runtime::spawn(async move{
                    let client=handle.state::<cumulus_core::client::MusicClient>();
                    let clean=client.close_together().await.is_ok()&&!client.has_together_cleanup().unwrap_or(true);
                    let end_count=api.calls.lock().unwrap().iter().filter(|(path,_)|path.ends_with("end/v2")).count();
                    std::fs::write(root.join("host-final.json"),serde_json::to_vec_pretty(&json!({"ui_ok":ok,"local_restored":clean,"fixture_api":true,"fixture_im":true,"fixture_audio":true,"real_peer_verified":false,"end_requests":end_count,"im_closed":im.closed.load(Ordering::SeqCst)})).unwrap()).unwrap();
                    handle.exit(if ok&&clean{0}else{1});
                });
            }
        });Ok(())
    }).on_page_load(|window,event|{if event.event()==PageLoadEvent::Finished{window.eval(if std::env::var_os("CUMULUS_LIFECYCLE_PROOF").is_some(){include_str!("together_lifecycle_ui_probe.js")}else{include_str!("shared_queue_ui_probe.js")}).unwrap();}})
        .run(tauri::generate_context!()).expect("production WebView with local fixtures");
}
