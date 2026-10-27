//! Production FM page and real libmpv, with a fail-closed read-only API adapter.
use cumulus_core::{
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{FmSource, PlayerAction},
    },
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
    sync::{Arc, Mutex},
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

struct Reads {
    api: Arc<dyn Api>,
    calls: Mutex<Vec<Value>>,
    output: PathBuf,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if !matches!(
                path,
                "/api/w/nuser/account/get"
                    | "/api/music-vip-membership/client/vip/info"
                    | "/api/user/playlist"
                    | "/api/song/like/get"
                    | "/api/v3/discovery/recommend/songs"
                    | "/api/v3/song/detail"
                    | "/api/song/enhance/player/url/v1"
                    | "/api/song/lyric/v1"
                    | "/api/v1/radio/get"
                    | "/api/v3/radio/trash/get"
                    | "/api/music/dislike/get"
            ) {
                fs::write(
                    self.output.join("blocked-endpoint.json"),
                    serde_json::to_vec(&json!({"path":path}))?,
                )?;
                return Err(cumulus_core::Error::State(
                    "non-read endpoint in FM UI proof",
                ));
            }
            {
                let mut calls = self.calls.lock().unwrap();
                // Only URL request parameters are recorded, never authorized URLs.
                calls.push(json!({"path":path,"params": if path == "/api/song/enhance/player/url/v1" {payload.clone()} else {Value::Null}}));
                fs::write(
                    self.output.join("api-calls.json"),
                    serde_json::to_vec_pretty(&*calls)?,
                )?;
            }
            self.api.request(path, payload, cache).await
        })
    }
}
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("proof root"));
    let profile =
        PathBuf::from(std::env::var_os("CUMULUS_FM_PROFILE").expect("explicit PC profile"));
    let reopen = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    if !reopen {
        fs::create_dir(&root).expect("new proof root");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let output = root.join(if reopen { "reopen" } else { "first" });
    fs::create_dir(&output).unwrap();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        let credentials = Arc::new(Credentials::open(Arc::new(StateStore::open(&profile)?), ApiOrigin::Pc)?);
        let api = Arc::new(Reads { api: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?), calls: Mutex::default(), output: output.clone() });
        let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
        let session = Arc::new(AccountSession::new(api, Some(registry.store())));
        let client = MusicClient::with_session(session, registry);
        app.manage(cumulus_desktop::configure_native_audio(client, &app.path().resource_dir()?, true));
        if small { app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?; }
        app.add_capability(r#"{"identifier":"fm-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle = app.handle().clone();
        app.listen("fm-proof", move |event| {
            let mut value: Value = serde_json::from_str(event.payload()).unwrap();
            let stage = value["stage"].as_str().unwrap_or("").to_owned();
            if !stage.starts_with("fm_") && !["complete","failed"].contains(&stage.as_str()) { return; }
            let handle = handle.clone(); let output = output.clone();
            tauri::async_runtime::spawn(async move {
                let client = handle.state::<MusicClient>();
                let epoch = client.account().unwrap().epoch;
                let observed = async {
                    let player = client.subscribe_playback(&epoch).await?.current()?;
                    value["nativePlayer"] = serde_json::to_value(&player)?;
                    value["nativeQueue"] = serde_json::to_value(client.player_queue(&epoch).await?)?;
                    value["nativeFm"] = serde_json::to_value(client.fm_view(&epoch, FmSource::Personal, false).await?)?;
                    Ok::<_, cumulus_core::Error>(())
                }.await;
                if let Err(e) = observed {
                    value["nativeError"] = json!(e.to_string());
                    fs::write(output.join("failed.json"), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                    handle.exit(1); return;
                }
                if ["complete","failed"].contains(&stage.as_str()) {
                    let _ = client.player_action(&epoch, PlayerAction::Pause { paused:true }).await;
                }
                fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                if ["complete","failed"].contains(&stage.as_str()) { handle.exit(if stage=="complete" {0} else {1}); }
                else {handle.get_webview_window("main").unwrap().eval(format!("window.__fmAck={};",json!(stage))).unwrap();}
            });
        });
        Ok(())
    }).on_page_load(move |webview,payload| {
        if payload.event()==PageLoadEvent::Finished {
            webview.eval(format!("window.__fmReopen={reopen};")).unwrap();
            webview.eval(include_str!("fm_ui_probe.js")).unwrap();
        }
    }).run(tauri::generate_context!()).expect("FM UI proof");
}
