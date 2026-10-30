//! Production intercepted-login policy and UI; only music HTTP is synthetic.
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
    legacy: bool,
    inner: fixture::Service,
    validations: AtomicUsize,
    searches: AtomicUsize,
}
impl HttpBackend for Service {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let form: std::collections::BTreeMap<_, _> = url_fields(&plan.body)?;
            let (path, payload) =
                eapi::unwrap(form.get("params").ok_or(Error::Response("fixture EAPI"))?)?;
            // The search box loads its default hint while the account is restored.
            // Only the two submitted result requests exercise login interception;
            // the hint must not consume the first validation before user input.
            if path == "/api/search/default/keyword/get" {
                return Ok(fixture::reply(
                    json!({"code":200,"data":{"keywords":[],"refreshTime":2500}}),
                    None,
                ));
            }
            if path == "/api/search/pc/complex/page" {
                self.searches.fetch_add(1, Ordering::SeqCst);
                return Ok(fixture::reply(json!({"code":301}), None));
            }
            if self.legacy && path == "/api/rtrs/abt/front/expinfo/list" {
                return Ok(fixture::reply(
                    json!({"code":200,"data":[
                        {"expName":"PH-PC-loginJsk","expGroupName":"c"},
                        {"expName":"PH-PC-newLoginBenefit","expGroupName":"c"}
                    ]}),
                    None,
                ));
            }
            if path == "/api/middle/account/token/refresh" {
                let valid = if self.legacy {
                    payload.get("fromPage").is_none()
                        && payload.get("fromAPI").is_none()
                        && !plan
                            .headers
                            .keys()
                            .any(|k| k.eq_ignore_ascii_case("x-login-chain-id"))
                } else {
                    payload["fromPage"] == "JSK"
                        && payload["fromAPI"] == "/api/search/pc/complex/page"
                };
                if !valid {
                    return Err(Error::Response("fixture middle parameters"));
                }
                let count = self.validations.fetch_add(1, Ordering::SeqCst);
                return Ok(fixture::reply(
                    if count == 0 {
                        json!({"code":200,"data":{"action":1}})
                    } else {
                        json!({"code":200,"data":{"action":2,"followAction":1,"title":"请重新确认登录","message":"登录状态已失效，请重新登录"}})
                    },
                    None,
                ));
            }
            self.inner.post(plan).await
        })
    }
}
fn url_fields(body: &[u8]) -> Result<std::collections::BTreeMap<String, String>, Error> {
    let form = std::str::from_utf8(body).map_err(|_| Error::Response("fixture form"))?;
    let params = form
        .split('&')
        .find_map(|p| p.strip_prefix("params="))
        .ok_or(Error::Response("fixture EAPI"))?;
    Ok(std::collections::BTreeMap::from([(
        "params".into(),
        params.into(),
    )]))
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
    let legacy = std::env::var_os("CUMULUS_PROOF_LEGACY").is_some();
    let service = Arc::new(Service {
        legacy,
        ..Default::default()
    });
    let queue_store = Arc::new(StateStore::open(root.join("state")).unwrap());
    if legacy {
        for owner in ["2", "3", "local-guest"] {
            let mut queue =
                cumulus_core::playback::PlaybackQueue::new(Some(queue_store.clone()), owner)
                    .unwrap();
            queue
                .replace(
                    vec![json!({"id":10,"name":"验证队列"})],
                    0,
                    json!({"kind":"playlist"}),
                )
                .unwrap();
        }
    }
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app| {
  let client=MusicClient::open_with_http_backend(&root.join("state"),Some(&profile),service.clone())?;
  let client=cumulus_desktop::renewal::configure(client,&ClientProfile::default());
  app.manage(client);cumulus_desktop::renewal::start(app.handle());
  if std::env::var_os("CUMULUS_PROOF_SMALL").is_some() { app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?; }
  app.add_capability(r#"{"identifier":"interception-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
  let handle=app.handle().clone();
  app.listen("auth-proof",move|event| {
   let mut value:Value=serde_json::from_str(event.payload()).unwrap();
   let stage=value["stage"].as_str().unwrap_or("").to_owned();
   if !stage.starts_with("interception_") && !["complete","failed"].contains(&stage.as_str()) { return }
   let client=handle.state::<MusicClient>();let account=client.account().unwrap();
   let status=client.session_renewal(&account.epoch).unwrap();
   value["nativeRenewal"]=serde_json::to_value(status).unwrap();
   value["fixtureRequests"]=json!(*service.inner.paths.lock().unwrap());
   value["realMusicNetworkRequests"]=json!(0);
   value["nativeAccount"]=serde_json::to_value(account).unwrap();
   value["legacyPolicy"]=json!(legacy);
   if legacy {value["queueCounts"]=json!(["2","3","local-guest"].map(|owner|cumulus_core::playback::PlaybackQueue::new(Some(queue_store.clone()),owner).unwrap().len()));}
   value["validationRequests"]=json!(service.validations.load(Ordering::SeqCst));
   value["searchRequests"]=json!(service.searches.load(Ordering::SeqCst));
   if stage=="complete" && (value["validationRequests"]!=2||value["searchRequests"]!=2) {value["ok"]=json!(false);}
   fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
   if stage=="complete"||stage=="failed" {handle.exit(if stage=="complete"&&value["ok"]==true {0}else{1});}
   else {handle.get_webview_window("main").unwrap().eval(format!("window.__authAck={};",json!(stage))).unwrap();}
  });Ok(())
 }).on_page_load(|webview,payload| {if payload.event()==PageLoadEvent::Finished&&webview.label()=="main" {webview.eval(include_str!("login_interception_ui_probe.js")).unwrap();}})
 .run(tauri::generate_context!()).expect("login interception UI proof");
}
