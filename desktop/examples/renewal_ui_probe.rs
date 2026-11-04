//! Production renewal pump, UI and IPC with synthetic in-process music HTTP.
#[path = "support/auth.rs"]
mod fixture;
use cumulus_core::{
    Error,
    account::credentials::Credentials,
    client::MusicClient,
    protocol::{
        api::ApiFuture,
        http::{ApiOrigin, ClientProfile, HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Listener, Manager, webview::PageLoadEvent};

fn acknowledge_stage(handle: &AppHandle, output: &std::path::Path, stage: String) {
    let ack = format!("window.__authAck={};", json!(stage));
    if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() != Ok("1") {
        handle
            .get_webview_window("main")
            .unwrap()
            .eval(ack)
            .unwrap();
        return;
    }
    let marker = output.join(format!("{stage}.captured"));
    let handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        let captured = tokio::time::timeout(std::time::Duration::from_secs(8), async {
            while !marker.is_file() {
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            }
        })
        .await
        .is_ok();
        if !captured
            || handle
                .get_webview_window("main")
                .is_none_or(|window| window.eval(ack).is_err())
        {
            handle.exit(1);
        }
    });
}
#[derive(Default)]
struct Service {
    inner: fixture::Service,
    rotations: AtomicUsize,
    refuse_identity_once: AtomicBool,
}
impl HttpBackend for Service {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let form =
                std::str::from_utf8(&plan.body).map_err(|_| Error::Response("fixture form"))?;
            let params = form
                .split('&')
                .find_map(|p| p.strip_prefix("params="))
                .ok_or(Error::Response("fixture EAPI"))?;
            let (path, payload) = eapi::unwrap(params)?;
            if path == "/api/w/nuser/account/get"
                && self.refuse_identity_once.swap(false, Ordering::SeqCst)
            {
                *self.inner.paths.lock().unwrap().entry(path).or_default() += 1;
                return Ok(fixture::reply(json!({"code":301}), None));
            }
            if path != "/api/login/token/refresh/v1" {
                return self.inner.post(plan).await;
            }
            *self.inner.paths.lock().unwrap().entry(path).or_default() += 1;
            if payload["ydDeviceToken"] != "" || payload["_scver"] != "1" {
                return Err(Error::Response("fixture refresh fields"));
            }
            if self.rotations.fetch_add(1, Ordering::SeqCst) == 0 {
                self.refuse_identity_once.store(true, Ordering::SeqCst);
                Ok(fixture::reply(json!({"code":200}), Some(2)))
            } else {
                Err(Error::Transport("synthetic response lost".into()))
            }
        })
    }
}
fn main() {
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new isolated proof root"));
    fs::create_dir(&root).unwrap();
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let profile = root.join("profile");
    let credentials =
        Credentials::open(Arc::new(StateStore::open(&profile).unwrap()), ApiOrigin::Pc).unwrap();
    let target = ApiOrigin::Pc.url();
    let before = credentials.request_credentials(&target).unwrap();
    credentials
        .record_response(
            &target,
            &["MUSIC_U=fixture2; Path=/; Secure".into()],
            None,
            &before.revision,
        )
        .unwrap();
    let service = Arc::new(Service::default());
    let clock_offset = Arc::new(AtomicU64::new(0));
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app| {
  let client=MusicClient::open_with_http_backend(&root.join("state"),Some(&profile),service.clone())?;
  let client=cumulus_desktop::renewal::configure(client,&ClientProfile::default()).with_session_renewal_clock({
   let offset=clock_offset.clone();Arc::new(move|| Ok(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_|Error::State("fixture clock"))?.as_secs()+offset.load(Ordering::SeqCst)))
  });
  app.manage(client);cumulus_desktop::renewal::start(app.handle());
  if std::env::var_os("CUMULUS_PROOF_SMALL").is_some() { app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?; }
  app.add_capability(r#"{"identifier":"renewal-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
  let clock_handle=app.handle().clone();
  app.listen("renewal-clock-proof",move|event| {
   let action: String=serde_json::from_str(event.payload()).unwrap();
   let offset=match action.as_str() {"reset"=>0,"advance"=>86401,"advance_again"=>172802,_=>panic!("unexpected fixture clock action")};
   clock_offset.store(offset,Ordering::SeqCst);
   clock_handle.get_webview_window("main").unwrap().eval(format!("window.__renewalClockAck={};",json!(action))).unwrap();
  });
  let handle=app.handle().clone();
  app.listen("auth-proof",move|event| {
   let mut value:Value=serde_json::from_str(event.payload()).unwrap();
   let stage=value["stage"].as_str().unwrap_or("").to_owned();
   if !stage.starts_with("renewal_") && !["complete","failed"].contains(&stage.as_str()) { return }
   let client=handle.state::<MusicClient>();let account=client.account().unwrap();
   let status=client.session_renewal(&account.epoch).unwrap();
   value["nativeRenewal"]=serde_json::to_value(status).unwrap();
   value["accountStatus"]=serde_json::to_value(account.status).unwrap();
   value["fixtureRequests"]=json!(*service.inner.paths.lock().unwrap());
   value["realMusicNetworkRequests"]=json!(0);
   value["rotationRequests"]=json!(service.rotations.load(Ordering::SeqCst));
   if stage=="complete" && value["rotationRequests"]!=2 {value["ok"]=json!(false);}
   fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
   if stage=="complete"||stage=="failed" {handle.exit(if stage=="complete"&&value["ok"]==true {0}else{1});}
   else {acknowledge_stage(&handle,&output,stage);}
  });Ok(())
 }).on_page_load(|webview,payload| {if payload.event()==PageLoadEvent::Finished&&webview.label()=="main" {webview.eval(include_str!("renewal_ui_probe.js")).unwrap();}})
 .run(tauri::generate_context!()).expect("renewal UI proof");
}
