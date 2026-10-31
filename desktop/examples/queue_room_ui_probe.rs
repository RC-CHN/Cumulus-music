//! Production UI/IPC/actor/persistence, with synthetic API/IM/audio devices.
//! Never a real second-account or physical audio proof.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[allow(dead_code, unused_imports)]
#[path = "../../core/tests/client_together.rs"]
mod fixture;
use cumulus_core::{
    client::MusicClient,
    listen_together::{RoomMessage, native_im::ImEvent},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    // SAFETY: process-local setup before GTK/runtime or worker creation.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new isolated proof root"));
    fs::create_dir_all(&root).unwrap();
    let output = root.join("first");
    fs::create_dir(&output).expect("new proof output");
    let api = Arc::new(fixture::Backend::default());
    api.shell.store(true, Ordering::SeqCst);
    let (_, client) = fixture::world(&root.join("state"), api.clone());
    let im = Arc::new(fixture::im::Device::default());
    let client = client.with_together_im_factory(Arc::new(fixture::im::Factory(im.clone())));
    let mut context = assets::context();
    let config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        app.manage(client);
        app.add_capability(r#"{"identifier":"queue-room-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();let state_root=root.clone();
        app.listen("ux-proof",move|event|{
            let mut value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("").to_owned();
            if stage=="resize" {handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(value["width"].as_f64().unwrap(),value["height"].as_f64().unwrap())).unwrap();return;}
            let handle=handle.clone();let output=output.clone();let api=api.clone();let im=im.clone();let state_root=state_root.clone();
            tauri::async_runtime::spawn(async move{
                let client=handle.state::<MusicClient>();let epoch=client.account().unwrap().epoch;
                if stage=="fixture" {
                    match value["kind"].as_str().unwrap() {
                        "observe_room"|"replace_room"|"erase_room"=>{
                            {
                                let mut state=api.state.lock().unwrap();
                                state.inside=value["kind"]!="erase_room";
                                state.replacement_room=if value["kind"]=="replace_room" {Some("different-room".into())}else{None};
                            }
                            client.together_view(&epoch,true).await.unwrap();
                        },
                        "reject_queue"|"allow_queue"=>{
                            let sql=rusqlite::Connection::open(state_root.join("state/core.sqlite3")).unwrap();
                            sql.execute_batch(if value["kind"]=="reject_queue" {"CREATE TRIGGER reject_queue_room BEFORE INSERT ON state WHEN NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(ABORT,'synthetic queue failure'); END;"} else {"DROP TRIGGER reject_queue_room;"}).unwrap();
                        },
                        "hold_end"=>{api.end_started.store(false,Ordering::SeqCst);api.hold_end.store(true,Ordering::SeqCst);},
                        "release_end"=>{api.hold_end.store(false,Ordering::SeqCst);api.end_release.notify_one();},
                        "lose_end"=>api.drop_end.store(true,Ordering::SeqCst),
                        "peer"=>{
                            api.state.lock().unwrap().peer=true;
                            let message=RoomMessage::from_attachment(&json!({"content":{"type":20002,"content":{"roomId":"test-room"}}}).to_string()).unwrap().unwrap();
                            im.controls.lock().unwrap().last().unwrap().send(ImEvent::Business{messages:vec![message]}).unwrap();
                        },
                        other=>panic!("unknown queue-room fixture {other}"),
                    }
                    handle.get_webview_window("main").unwrap().eval("window.__queueRoomFixtureAck=true;").unwrap();return;
                }
                if !(["complete","failed"].contains(&stage.as_str()) || stage.starts_with("ux_") && stage.chars().all(|c|c.is_ascii_alphanumeric()||c=='_')) {return;}
                value["nativePlayer"]=serde_json::to_value(client.subscribe_playback(&epoch).await.unwrap().current().unwrap()).unwrap();
                value["nativeQueue"]=serde_json::to_value(client.player_queue(&epoch).await.unwrap()).unwrap();
                value["together"]=serde_json::to_value(client.together_view(&epoch,false).await.unwrap().value).unwrap();
                value["endRequests"]=json!(api.calls.lock().unwrap().iter().filter(|(p,_)|p.ends_with("end/v2")).count());
                value["endStarted"]=json!(api.end_started.load(Ordering::SeqCst));
                value["imClosed"]=json!(im.closed.load(Ordering::SeqCst));
                fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                if !capture_ack::acknowledge(&handle,&output,&stage,value["captureHandshake"]==true).await{return;}
                if ["complete","failed"].contains(&stage.as_str()) {
                    api.hold_end.store(false,Ordering::SeqCst);api.end_release.notify_one();
                    let clean=client.close_together().await.is_ok()&&!client.has_together_cleanup().unwrap_or(true);
                    fs::write(output.join("host-final.json"),serde_json::to_vec_pretty(&json!({"clean":clean,"fixtureApi":true,"fixtureIm":true,"fixtureAudio":true,"realMusicRequests":0,"realAccountWrites":0,"realPeerVerified":false,"physicalAudioOutputVerified":false})).unwrap()).unwrap();
                    handle.exit(if stage=="complete"&&clean{0}else{1});
                }
            });
        });
        tauri::WebviewWindowBuilder::from_config(app,&config)?.data_directory(root.join("webview")).build()?;
        Ok(())
    }).on_page_load(|window,event|{if event.event()==PageLoadEvent::Finished{
        let script=include_str!("queue_room_ui_probe.js");
        if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref()==Ok("1"){window.eval(capture_ack::script(script)).unwrap();}
        else {window.eval(script).unwrap();}
    }})
    .run(context).expect("queue-room production WebView");
}
