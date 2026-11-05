//! Real Tauri/Vue account restoration with a socket-free music HTTP fixture.
use cumulus_core::{
    Error,
    account::credentials::Credentials,
    client::MusicClient,
    protocol::{
        api::ApiFuture,
        http::{ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
    storage::StateStore,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

const IDENTITY: &str = "/api/w/nuser/account/get";
const ASSIGNMENT: &str = "/api/rtrs/abt/front/expinfo/list";
const EXPERIMENTS: &str = "PH-PC-loginJsk,PH-PC-newLoginBenefit";
const LEGACY: &str = "/api/login/token/refresh";
const JSK: &str = "/api/login/token/refresh/v1";
const COOKIE: &str = "MUSIC_U=legacy-renewal-fixture";
const STAGE: &str = "legacy_renewal_checked";

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Trace {
    identity_ms: Vec<u64>,
    assignment_ms: Vec<u64>,
    legacy_ms: Vec<u64>,
    legacy_wire_valid: bool,
    jsk_count: usize,
    unexpected_auth_count: usize,
}

struct Service {
    start: Instant,
    cohort: &'static str,
    trace: Mutex<Trace>,
}

impl Service {
    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

impl HttpBackend for Service {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let form = std::str::from_utf8(&plan.body)
                .map_err(|_| Error::Response("fixture requires EAPI form"))?;
            let params = form
                .split('&')
                .find_map(|pair| pair.strip_prefix("params="))
                .ok_or(Error::Response("fixture requires EAPI params"))?;
            let (path, data) = eapi::unwrap(params)?;
            let scoped = plan
                .headers
                .get("Cookie")
                .is_some_and(|value| value.split(';').any(|part| part.trim() == COOKIE));
            let now = self.elapsed_ms();
            let mut trace = self.trace.lock().map_err(|_| Error::LockPoisoned)?;
            let payload = match path.as_str() {
                IDENTITY if scoped => {
                    trace.identity_ms.push(now);
                    json!({
                        "code":200,
                        "account":{"id":42},
                        "profile":{"userId":42,"nickname":"合成续期账号"}
                    })
                }
                ASSIGNMENT if scoped && data["expNames"] == EXPERIMENTS => {
                    trace.assignment_ms.push(now);
                    json!({"code":200,"data":[
                        {"expName":"PH-PC-loginJsk","expGroupName":self.cohort},
                        {"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}
                    ]})
                }
                ASSIGNMENT => json!({"code":200,"data":[]}),
                LEGACY => {
                    trace.legacy_ms.push(now);
                    trace.legacy_wire_valid = scoped
                        && ApiOrigin::Pc.require_url(&plan.url).is_ok()
                        && plan.url.path() == "/eapi/login/token/refresh"
                        && !plan.headers.contains_key("x-login-chain-id")
                        && data.as_object().is_some_and(|fields| {
                            fields
                                .keys()
                                .all(|key| matches!(key.as_str(), "e_r" | "header"))
                        });
                    if !trace.legacy_wire_valid {
                        return Err(Error::Response("legacy request fixture boundary"));
                    }
                    json!({"code":200})
                }
                JSK => {
                    trace.jsk_count += 1;
                    return Err(Error::Response("fixture must not receive JSK rotation"));
                }
                IDENTITY | "/api/logout" | "/api/middle/account/token/refresh" => {
                    trace.unexpected_auth_count += 1;
                    return Err(Error::Response("unexpected fixture authentication route"));
                }
                path if path.starts_with("/api/login/") || path.starts_with("/api/register/") => {
                    trace.unexpected_auth_count += 1;
                    return Err(Error::Response("unexpected fixture authentication route"));
                }
                "/api/user/playlist" => json!({"code":200,"playlist":[],"more":false}),
                "/api/song/like/get" => json!({"code":200,"ids":[]}),
                "/api/v3/discovery/recommend/songs" => json!({"code":200,"data":{"dailySongs":[]}}),
                _ => return Err(Error::Response("unexpected fixture resource route")),
            };
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&payload)?,
                body_status: BodyStatus::Complete,
                set_cookies: Vec::new(),
                refresh_token: None,
                metadata: Default::default(),
            })
        })
    }
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new sealed proof root"));
    let cohort = match std::env::var("CUMULUS_PROOF_COHORT").as_deref() {
        Ok("legacy") => "c",
        Ok("jsk") => "t1",
        _ => panic!("select a sealed proof cohort"),
    };
    fs::create_dir(&root).expect("proof root must be new");
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let profile = root.join("profile");
    let store = Arc::new(StateStore::open(&profile).unwrap());
    let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
    let target = ApiOrigin::Pc.url();
    let before = credentials.request_credentials(&target).unwrap();
    credentials
        .record_response(
            &target,
            &[format!("{COOKIE}; Path=/; Secure; HttpOnly")],
            None,
            &before.revision,
        )
        .unwrap();
    let service = Arc::new(Service {
        start: Instant::now(),
        cohort,
        trace: Mutex::default(),
    });

    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let client = MusicClient::open_with_http_backend(
                &root.join("state"),
                Some(&profile),
                service.clone(),
            )?;
            app.manage(cumulus_desktop::renewal::configure(
                client,
                &ClientProfile::default(),
            ));
            cumulus_desktop::renewal::start(app.handle());
            cumulus_desktop::renewal::start_legacy(app.handle())?;
            app.add_capability(r#"{"identifier":"legacy-renewal-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("legacy-renewal-proof", move |event| {
                let dom: Value = serde_json::from_str(event.payload()).unwrap();
                let account = handle.state::<MusicClient>().account().unwrap();
                let state = serde_json::to_value(account.status).unwrap();
                let record = store.get("42", "login_refresh", "legacy-periodic").unwrap();
                let trace = service.trace.lock().unwrap();
                let checked_ms = service.elapsed_ms();
                let legacy = cohort == "c";
                let cohort_result = match (
                    trace.identity_ms.as_slice(),
                    trace.assignment_ms.as_slice(),
                    trace.legacy_ms.as_slice(),
                    record.as_ref(),
                ) {
                    ([first, second], [assigned], [dispatched], Some(value)) if legacy => {
                        trace.legacy_wire_valid
                            && dispatched.saturating_sub(*first) >= 3_000
                            && first <= assigned
                            && assigned <= dispatched
                            && dispatched <= second
                            && *second <= checked_ms
                            && value["pending"].is_null()
                            && value["last_success"].is_u64()
                    }
                    ([first], [assigned], [], None) if !legacy => first <= assigned,
                    _ => false,
                };
                let passed = dom["ok"] == true
                    && dom["renewalState"] == "missing_credentials"
                    && state == "authenticated"
                    && trace.jsk_count == 0
                    && trace.unexpected_auth_count == 0
                    && trace.identity_ms.first().is_some_and(|restored| {
                        checked_ms.saturating_sub(*restored) >= 4_000
                    })
                    && cohort_result;
                let report = json!({
                    "ok":passed,
                    "case":if legacy { "legacy" } else { "jsk" },
                    "dom":dom,
                    "nativeAccountStatus":state,
                    "trace":*trace,
                    "checkedMs":checked_ms,
                    "legacySuccessRecorded":record.as_ref().is_some_and(|v| v["last_success"].is_u64()),
                    "realMusicRequests":0,
                });
                let file = output.join(format!("{STAGE}.json"));
                fs::write(&file, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let captured = std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() != Ok("1")
                        || tokio::time::timeout(Duration::from_secs(8), async {
                            while !file.with_extension("captured").is_file() {
                                tokio::time::sleep(Duration::from_millis(40)).await;
                            }
                        })
                        .await
                        .is_ok();
                    if let Some(window) = handle.get_webview_window("main") {
                        let _ = window.destroy();
                    }
                    handle.exit(i32::from(!(passed && captured)));
                });
            });
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished && webview.label() == "main" {
                webview.eval(include_str!("legacy_renewal_ui_probe.js")).unwrap();
            }
        })
        .run(tauri::generate_context!())
        .expect("legacy renewal UI proof");
}
