//! Sealed PC assignment transport: genuine MusicHttpClient context/encryption,
//! one synthetic local credential profile, and no external HTTP backend.
use cumulus_core::{
    Error, Result,
    account::credentials::Credentials,
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{
            ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply, MusicHttpClient,
            OFFICIAL_WINDOWS_USER_AGENT, RequestOptions, RequestPlan, WireRoute,
        },
        mconfig::{ConfigBackend, ConfigUrl, PcConfig},
        pc_experiments::{ASSIGNMENT_PATH, PcExperimentGroups, assignment_arguments},
        wire::eapi,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const ACCOUNT: &str = "/api/w/nuser/account/get";
const MUSIC_U: &str = "synthetic-search-signed-session";
const SIGN: &str = "synthetic-search-signed-client-sign";
const VERSION: &str = "3.1.42.205594";

struct ClosedConfig;
impl ConfigBackend for ClosedConfig {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("sealed search config cannot use remote HTTP")) })
    }
}
impl TokenProvider for ClosedConfig {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async {
            Err(Error::State(
                "sealed search assignment cannot use an SDK token",
            ))
        })
    }
}

pub struct SignedSearch {
    pub client: Arc<MusicHttpClient>,
    backend: Arc<Backend>,
}
impl SignedSearch {
    pub fn open(store: Arc<StateStore>) -> Result<Arc<Self>> {
        let mode = std::env::var("CUMULUS_SEARCH_FOOTER_PROOF").unwrap_or_else(|_| "t1".into());
        if !matches!(mode.as_str(), "t1" | "c" | "unknown" | "error") {
            return Err(Error::InvalidInput("invalid sealed search footer cohort"));
        }
        let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc)?);
        let target = ApiOrigin::Pc.url();
        let original = credentials.request_credentials(&target)?;
        // The proof root is newly created by the runner. Never import an account,
        // profile or device from the real host into this signed client.
        if !original.cookies.is_empty() {
            return Err(Error::State(
                "signed search proof requires fresh isolated credentials",
            ));
        }
        credentials.record_response(
            &target,
            &[format!("MUSIC_U={MUSIC_U}; Path=/; Secure")],
            None,
            &original.revision,
        )?;
        let config = PcConfig::new(store, Arc::new(ClosedConfig))?;
        let backend = Arc::new(Backend {
            device: credentials.device_id()?,
            config: config.clone(),
            mode,
            account_restored: AtomicBool::new(false),
            calls: Mutex::default(),
        });
        let context = Arc::new(RequestContext::from_pc(
            config,
            VERSION,
            Arc::new(ClosedConfig),
        )?);
        let client = Arc::new(MusicHttpClient::with_backend(
            credentials,
            ClientProfile {
                user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
                os_version: "Microsoft Windows 10".into(),
                client_sign: Some(SIGN.into()),
            },
            RequestOptions::default(),
            Some(context),
            backend.clone(),
        )?);
        Ok(Arc::new(Self { client, backend }))
    }

    pub fn report(&self) -> Value {
        json!({"transport":"MusicHttpClient", "syntheticOnly":true,
            "sameProfileAccountRestore":self.backend.account_restored.load(Ordering::Acquire),
            "footerCase":self.backend.mode, "calls":self.backend.calls.lock().unwrap().clone()})
    }
}

struct Backend {
    device: String,
    config: Arc<PcConfig>,
    mode: String,
    account_restored: AtomicBool,
    calls: Mutex<Vec<Value>>,
}
impl HttpBackend for Backend {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            if plan.route != WireRoute::Eapi
                || plan.url.origin() != ApiOrigin::Pc.url().origin()
                || plan.headers.get("User-Agent").map(String::as_str)
                    != Some(OFFICIAL_WINDOWS_USER_AGENT)
                || plan.headers.contains_key("x-login-chain-id")
                || plan.headers.contains_key("x-music-tu")
            {
                return Err(Error::State(
                    "sealed assignment lost its signed PC transport",
                ));
            }
            let fields: BTreeMap<_, _> = url::form_urlencoded::parse(&plan.body)
                .into_owned()
                .collect();
            if fields.len() != 1 {
                return Err(Error::Response("unexpected sealed EAPI form"));
            }
            let (path, mut args) = eapi::unwrap(
                fields
                    .get("params")
                    .ok_or(Error::Response("missing sealed EAPI params"))?,
            )?;
            if !matches!(path.as_str(), ACCOUNT | ASSIGNMENT_PATH)
                || plan.url.path() != ApiOrigin::Pc.request_url(WireRoute::Eapi, &path)?.path()
            {
                return Err(Error::State(
                    "sealed search transport refused another endpoint",
                ));
            }
            let body = args
                .as_object_mut()
                .ok_or(Error::Response("invalid signed search EAPI body"))?;
            let header = body
                .remove("header")
                .ok_or(Error::Response("signed search header missing"))?;
            let header: Value = if let Some(text) = header.as_str() {
                serde_json::from_str(text)?
            } else {
                header
            };
            body.remove("e_r");
            let cookie = plan
                .headers
                .get("Cookie")
                .ok_or(Error::Response("signed search cookie missing"))?;
            let expected_config = self.config.snapshot()?.request_header(VERSION)?;
            if cookie_values(cookie, "MUSIC_U") != [MUSIC_U]
                || cookie_values(cookie, "deviceId") != [self.device.as_str()]
                || header["deviceId"] != self.device
                || header["clientSign"] != SIGN
                || header["os"] != "pc"
                || header["osver"] != "Microsoft Windows 10"
                || header["appver"] != VERSION
                // M/92 runs after M/402 serialization. The genuine transport
                // adds MConfig-Info only to HTTP, never the encrypted header.
                || header.get("MConfig-Info").is_some()
                || plan.headers.get("MConfig-Info") != Some(&expected_config)
            {
                return Err(Error::State(
                    "signed search credentials/context do not match the restored account",
                ));
            }
            let response = if path == ACCOUNT {
                if args != json!({}) {
                    return Err(Error::Response("unexpected signed account arguments"));
                }
                self.account_restored.store(true, Ordering::Release);
                json!({"code":200,"profile":{"userId":2,"nickname":"目录界面验证"}})
            } else {
                if !self.account_restored.load(Ordering::Acquire) || args != assignment_arguments()
                {
                    return Err(Error::State(
                        "assignment must use the restored credential and exact full batch",
                    ));
                }
                if self.mode == "error" {
                    self.calls.lock().unwrap().push(json!({"path":path,"args":args,
                        "signedPcContextVerified":true,"sameAccountCredential":true,"response":"synthetic_error"}));
                    return Err(Error::Transport("sealed assignment unavailable".into()));
                }
                let mut groups = layout_rows();
                groups.push(json!({"expName":"PH-PC-search-result",
                    "expGroupName":if self.mode == "unknown" {"future"} else {self.mode.as_str()}}));
                json!({"code":200,"data":groups})
            };
            // No cookie, signature, raw device identifier, credential revision or
            // encrypted body enters the WebView report or public test evidence.
            self.calls
                .lock()
                .unwrap()
                .push(json!({"path":path,"args":args,
                "signedPcContextVerified":true,"sameAccountCredential":true,"responseCode":200}));
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&response)?,
                body_status: BodyStatus::Complete,
                set_cookies: vec![],
                refresh_token: None,
                metadata: BTreeMap::new(),
            })
        })
    }
}

fn layout_rows() -> Vec<Value> {
    let v3 = super::search_fixture::v3_enabled() || super::vertical_zone_fixture::hotspot();
    let carousel = super::search_fixture::carousel_enabled();
    vec![
        json!({"expName":"PH-PC-searchHot","expGroupName":if v3 {"t1"} else {"c"}}),
        json!({"expName":"PH-PC-searchMore2","expGroupName":if carousel {"t1"} else {"c"}}),
        json!({"expName":"PH-PC-searchPageEfficiency","expGroupName":if carousel {"t1"} else {"c"}}),
    ]
}

/// Preserve older fixture capability ownership and every observed cohort, while
/// keeping its search layout in the same full registry read. This deliberately
/// never grants the new footer cohort to a mocked legacy fixture.
pub fn legacy_layout(groups: PcExperimentGroups) -> Result<PcExperimentGroups> {
    if !super::search_fixture::enabled() {
        return Ok(groups);
    }
    let names = assignment_arguments();
    let mut rows = names["expNames"]
        .as_str()
        .ok_or(Error::Response("invalid assignment manifest"))?
        .split(',')
        .map(|name| json!({"expName":name,"expGroupName":groups.group(name)}))
        .collect::<Vec<_>>();
    for replacement in layout_rows() {
        if let Some(row) = rows
            .iter_mut()
            .find(|row| row["expName"] == replacement["expName"])
        {
            *row = replacement;
        }
    }
    PcExperimentGroups::refreshed(&json!({"code":200,"data":rows}))
}

fn cookie_values<'a>(cookie: &'a str, name: &str) -> Vec<&'a str> {
    cookie
        .split(';')
        .filter_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            (key == name).then_some(value)
        })
        .collect()
}
