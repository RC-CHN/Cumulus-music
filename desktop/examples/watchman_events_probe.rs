//! Actual WebView input -> production registered IPC -> Rust sampler. No network.
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
    watchman::{Config, EntropyClock, SystemEntropyClock, descriptors, encode_field, encode_text},
};
use cumulus_desktop::watchman::{EventHost, start_events, stop_events};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
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
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof directory"));
    fs::create_dir(&root).unwrap();
    let generation = Arc::new(Mutex::new((0u64, 0u64)));
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app|{
        let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
        let session=Arc::new(AccountSession::new(Arc::new(Offline),Some(registry.store())));
        app.manage(MusicClient::with_session(session,registry));
        app.add_capability(r#"{"identifier":"watchman-events-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen("watchman-events-proof",move|event|{
            let run=||->Result<(),Box<dyn std::error::Error>>{
                let data:Value=serde_json::from_str(event.payload())?;
                let window=handle.get_webview_window("main").unwrap();
                let config=Config::from_conf("testProduct","",&encode_text("{}")?)?;
                let mut generations=generation.lock().unwrap();
                match data["stage"].as_str().unwrap_or("") {
                    "ready"=>{generations.0=start_events(&window,&config,data["cors"].as_bool().unwrap())?;window.eval("window.__watchmanEventsProof.begin();")?;fs::write(root.join("ready.json"),serde_json::to_vec_pretty(&data)?)?;}
                    "captured"=>{
                        let host=handle.state::<EventHost>();let received=host.received(generations.0)?;
                        let events=host.drain(generations.0,SystemEntropyClock.now_ms()?)?;
                        let keys=events.iter().filter(|e|e.key=="_keydown" && e.value[2]==2 && e.value[3]=="watchman-proof-input").count();
                        let clicks=events.iter().filter(|e|e.key=="_click" && e.value[2]==2).count();
                        assert!(keys>=2 && clicks>=2 && data["typedLength"].as_u64().unwrap()>=8);
                        let table=descriptors()?;
                        let fields:Vec<_>=events.iter().map(|e|json!({"key":e.key,"value":e.value,"bytes":encode_field(table.iter().find(|f|f.key==e.key).unwrap(),&e.value,false).unwrap()})).collect();
                        fs::write(root.join("captured.json"),serde_json::to_vec_pretty(&json!({"ok":true,"receivedFrames":received,"trustedKeySamples":keys,"trustedClickSamples":clicks,"typedLength":data["typedLength"],"fields":fields}))?)?;
                        stop_events(&window)?;
                        generations.1=start_events(&window,&config,true)?;
                        window.eval(format!("window.__watchmanEventsProof.restart({});",generations.0))?;
                    }
                    "restarted"=>{
                        assert_eq!(data["staleRejected"],true);
                        let host=handle.state::<EventHost>();assert!(host.drain(generations.0,SystemEntropyClock.now_ms()?).is_err());
                        let events=host.drain(generations.1,SystemEntropyClock.now_ms()?)?;
                        assert!(events.iter().any(|e|e.key=="_click" && e.value[2]==3 && e.value[5]=="watchman-proof-button"));
                        stop_events(&window)?;
                        fs::write(root.join("complete.json"),serde_json::to_vec_pretty(&json!({"ok":true,"realWebview":true,"trustedOsInput":true,"programmaticInputMarkedUntrusted":true,"staleGenerationRejected":true,"networkRequests":0,"inputContentsCollected":false,"sdkTokenRequests":0}))?)?;
                        let handle=handle.clone();tauri::async_runtime::spawn(async move{tokio::time::sleep(std::time::Duration::from_secs(2)).await;handle.exit(0);});
                    }
                    "failed"=>return Err("JavaScript probe failed".into()),
                    _=>return Err("unexpected probe stage".into()),
                }Ok(())
            };
            if let Err(e)=run(){fs::write(root.join("failed.json"),json!({"error":e.to_string()}).to_string()).unwrap();handle.exit(1);}
        });Ok(())
    }).on_page_load(|webview, payload| {
        if payload.event() == PageLoadEvent::Started
            && let Some(host) = webview.try_state::<EventHost>()
        {
            let _ = host.stop();
        } else if payload.event() == PageLoadEvent::Finished {
            webview.eval(include_str!("watchman_events_probe.js")).unwrap();
        }
    })
    .run(tauri::generate_context!()).expect("actual WebView proof");
}
