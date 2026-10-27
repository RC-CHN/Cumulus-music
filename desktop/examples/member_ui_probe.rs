//! Production membership DOM/IPC and native audio; API access is read-only.
use cumulus_core::{
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{MemberQuery, PlayerAction},
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
                    | "/api/vipnewcenter/app/viptab/module/get"
                    | "/api/vipnewcenter/app/resource/centertop/carousel/list"
                    | "/api/vipnewcenter/app/resource/common/list"
                    | "/api/vipnewcenter/app/viptab/recommend/song/list"
                    | "/api/vipnewcenter/app/viptab/ranking/list"
                    | "/api/vipnewcenter/app/resource/plus/recomend/album"
                    | "/api/vipnewcenter/app/viptab/recommend/playList/list"
                    | "/api/album/v3/detail"
                    | "/api/v6/playlist/detail"
                    | "/api/djradio/v3/get"
                    | "/api/v6/dj/program/byradio"
                    | "/api/dj/user/setting/resumeplay/get"
                    | "/api/content/my/liked/voice"
                    | "/api/resource/commentInfo/list"
                    | "/api/podcast/common/single/block/get"
                    | "/api/discovery/recommend/songs/history/recent"
                    | "/api/discovery/recommend/songs/history/detail"
            ) && !path
                .strip_prefix("/api/v1/resource/comments/R_SO_4_")
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
            {
                fs::write(
                    self.output.join("blocked-endpoint.json"),
                    serde_json::to_vec(&json!({"path":path}))?,
                )?;
                return Err(cumulus_core::Error::State(
                    "non-read endpoint in member proof",
                ));
            }
            let index = {
                let mut calls = self.calls.lock().unwrap();
                calls.push(json!({"path":path,"params":payload}));
                fs::write(
                    self.output.join("api-calls.json"),
                    serde_json::to_vec_pretty(&*calls)?,
                )?;
                calls.len()
            };
            let value = self.api.request(path, payload.clone(), cache).await?;
            if path.starts_with("/api/vipnewcenter/")
                || path.starts_with("/api/discovery/recommend/songs/history/")
                || matches!(
                    path,
                    "/api/album/v3/detail"
                        | "/api/v6/playlist/detail"
                        | "/api/djradio/v3/get"
                        | "/api/v6/dj/program/byradio"
                )
                || path == "/api/music-vip-membership/client/vip/info"
            {
                fs::write(
                    self.output.join(format!("official-{index:03}.json")),
                    serde_json::to_vec_pretty(
                        &json!({"path":path,"params":payload,"response":value}),
                    )?,
                )?;
            }
            Ok(value)
        })
    }
}
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("proof root"));
    let profile =
        PathBuf::from(std::env::var_os("CUMULUS_MEMBER_PROFILE").expect("explicit profile"));
    let links = std::env::var_os("CUMULUS_MEMBER_LINKS").is_some();
    let reopen = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    if !reopen {
        fs::create_dir(&root).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let output = root.join(if reopen { "reopen" } else { "first" });
    fs::create_dir(&output).unwrap();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
  let credentials=Arc::new(Credentials::open(Arc::new(StateStore::open(&profile)?),ApiOrigin::Pc)?);
  let api=Arc::new(Reads{api:Arc::new(MusicHttpClient::new(credentials,ClientProfile::default())?),calls:Mutex::default(),output:output.clone()});
  let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
  let session=Arc::new(AccountSession::new(api,Some(registry.store())));
  let client=MusicClient::with_session(session,registry);
  app.manage(cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true));
  if small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
  app.add_capability(r#"{"identifier":"member-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
  let handle=app.handle().clone();
  app.listen("member-proof",move|event|{
   let mut value:Value=serde_json::from_str(event.payload()).unwrap();let stage=value["stage"].as_str().unwrap_or("").to_owned();
   if !stage.starts_with("member_")&&!["complete","failed"].contains(&stage.as_str()){return;}
   let handle=handle.clone();let output=output.clone();
   tauri::async_runtime::spawn(async move{
    let client=handle.state::<MusicClient>();let epoch=client.account().unwrap().epoch;
    let observed=async {
     value["nativePlayer"]=serde_json::to_value(client.subscribe_playback(&epoch).await?.current()?)?;
     value["nativeQueue"]=serde_json::to_value(client.player_queue(&epoch).await?)?;
     let query=serde_json::from_value(value["query"].clone()).unwrap_or(MemberQuery::Home);
     value["nativeMember"]=serde_json::to_value(client.member_page(&epoch,query,false).await?)?;
     value["nativeRights"]=serde_json::to_value(client.member_rights(&epoch,false).await?)?;
     Ok::<_,cumulus_core::Error>(())
    }.await;
    if let Err(e)=observed{value["nativeError"]=json!(e.to_string());if stage!="failed"{fs::write(output.join("failed.json"),serde_json::to_vec_pretty(&value).unwrap()).unwrap();handle.exit(1);return;}}
    if ["complete","failed"].contains(&stage.as_str()){let _=client.player_action(&epoch,PlayerAction::Pause{paused:true}).await;}
    fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    if ["complete","failed"].contains(&stage.as_str()){handle.exit(if stage=="complete"{0}else{1});}
    else{handle.get_webview_window("main").unwrap().eval(format!("window.__memberAck={};",json!(stage))).unwrap();}
   });
  });Ok(())
 }).on_page_load(move|webview,payload|{if payload.event()==PageLoadEvent::Finished{webview.eval(format!("window.__memberReopen={reopen};window.__memberLinks={links};")).unwrap();webview.eval(include_str!("member_ui_probe.js")).unwrap();}}).run(tauri::generate_context!()).expect("member UI proof");
}
