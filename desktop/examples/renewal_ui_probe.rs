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
        atomic::{AtomicUsize, Ordering},
    },
};
use tauri::{Listener, Manager, webview::PageLoadEvent};
#[derive(Default)]
struct Service {
    inner: fixture::Service,
    rotations: AtomicUsize,
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
            if path != "/api/login/token/refresh/v1" {
                return self.inner.post(plan).await;
            }
            *self.inner.paths.lock().unwrap().entry(path).or_default() += 1;
            if payload["ydDeviceToken"] != "" || payload["_scver"] != "1" {
                return Err(Error::Response("fixture refresh fields"));
            }
            if self.rotations.fetch_add(1, Ordering::SeqCst) == 0 {
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
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app| {
  let client=MusicClient::open_with_http_backend(&root.join("state"),Some(&profile),service.clone())?;
  let client=cumulus_desktop::renewal::configure(client,&ClientProfile::default());
  app.manage(client);cumulus_desktop::renewal::start(app.handle());
  if std::env::var_os("CUMULUS_PROOF_SMALL").is_some() { app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?; }
  app.add_capability(r#"{"identifier":"renewal-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
  let handle=app.handle().clone();
  app.listen("auth-proof",move|event| {
   let mut value:Value=serde_json::from_str(event.payload()).unwrap();
   let stage=value["stage"].as_str().unwrap_or("").to_owned();
   if !stage.starts_with("renewal_") && !["complete","failed"].contains(&stage.as_str()) { return }
   let client=handle.state::<MusicClient>();let account=client.account().unwrap();
   let status=client.session_renewal(&account.epoch).unwrap();
   value["nativeRenewal"]=serde_json::to_value(status).unwrap();
   value["fixtureRequests"]=json!(*service.inner.paths.lock().unwrap());
   value["realMusicNetworkRequests"]=json!(0);
   value["rotationRequests"]=json!(service.rotations.load(Ordering::SeqCst));
   if stage=="complete" && value["rotationRequests"]!=2 {value["ok"]=json!(false);}
   fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
   if stage=="complete"||stage=="failed" {handle.exit(if stage=="complete"&&value["ok"]==true {0}else{1});}
   else {handle.get_webview_window("main").unwrap().eval(format!("window.__authAck={};",json!(stage))).unwrap();}
  });Ok(())
 }).on_page_load(|webview,payload| {if payload.event()==PageLoadEvent::Finished&&webview.label()=="main" {webview.eval(include_str!("renewal_ui_probe.js")).unwrap();}})
 .run(tauri::generate_context!()).expect("renewal UI proof");
}
