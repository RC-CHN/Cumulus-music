//! Closed native PC assignment/account transport for the listened-books proof.
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
        pc_experiments::{ASSIGNMENT_PATH, assignment_arguments},
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

pub const ACCOUNT: &str = "/api/w/nuser/account/get";
const COOKIE: &str = "MUSIC_U=synthetic-listened-books";
const SIGN: &str = "synthetic-listened-books-sign";
const VERSION: &str = "3.1.42.205594";

pub struct Authority {
    pub client: Arc<MusicHttpClient>,
    backend: Arc<Backend>,
}

struct Backend {
    device: String,
    treatment: AtomicBool,
    calls: Mutex<Vec<Value>>,
}

struct ClosedConfig;
impl ConfigBackend for ClosedConfig {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::State(
                "book proof cannot fetch external configuration",
            ))
        })
    }
}
impl TokenProvider for ClosedConfig {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async { Err(Error::State("book proof cannot use an external SDK")) })
    }
}

impl Authority {
    pub fn open(store: Arc<StateStore>) -> Result<Self> {
        let target = ApiOrigin::Pc.url();
        let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc)?);
        let original = credentials.request_credentials(&target)?;
        if !original.cookies.is_empty() {
            return Err(Error::State(
                "book proof requires a fresh synthetic profile",
            ));
        }
        credentials.record_response(
            &target,
            &[format!("{COOKIE}; Path=/; Secure")],
            None,
            &original.revision,
        )?;
        let backend = Arc::new(Backend {
            device: credentials.device_id()?,
            treatment: AtomicBool::new(false),
            calls: Mutex::default(),
        });
        let config = PcConfig::new(store, Arc::new(ClosedConfig))?;
        let context = RequestContext::from_pc(config, VERSION, Arc::new(ClosedConfig))?;
        let client = Arc::new(MusicHttpClient::with_backend(
            credentials,
            ClientProfile {
                user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
                os_version: "Microsoft Windows 10".into(),
                client_sign: Some(SIGN.into()),
            },
            RequestOptions::default(),
            Some(Arc::new(context)),
            backend.clone(),
        )?);
        Ok(Self { client, backend })
    }
    pub fn set(&self, treatment: bool) {
        self.backend.treatment.store(treatment, Ordering::Release);
    }
    pub fn report(&self) -> Value {
        json!(*self.backend.calls.lock().unwrap())
    }
}

impl HttpBackend for Backend {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let fields: BTreeMap<_, _> = url::form_urlencoded::parse(&plan.body)
                .into_owned()
                .collect();
            let (path, mut args) = eapi::unwrap(
                fields
                    .get("params")
                    .ok_or(Error::Response("book proof needs real EAPI"))?,
            )?;
            let args = args
                .as_object_mut()
                .ok_or(Error::Response("invalid book proof payload"))?;
            let header = args
                .remove("header")
                .ok_or(Error::Response("missing book proof PC header"))?;
            let header: Value = if let Some(text) = header.as_str() {
                serde_json::from_str(text)?
            } else {
                header
            };
            args.remove("e_r");
            let cookies = plan
                .headers
                .get("Cookie")
                .ok_or(Error::Response("missing book proof credential"))?;
            assert_eq!(
                cookies
                    .split(';')
                    .map(str::trim)
                    .filter(|v| v.starts_with("MUSIC_U="))
                    .collect::<Vec<_>>(),
                [COOKIE]
            );
            assert_eq!(plan.route, WireRoute::Eapi);
            assert_eq!(plan.url, ApiOrigin::Pc.request_url(WireRoute::Eapi, &path)?);
            assert_eq!(plan.headers["User-Agent"], OFFICIAL_WINDOWS_USER_AGENT);
            assert_eq!(header["deviceId"], self.device);
            assert_eq!(header["clientSign"], SIGN);
            assert_eq!(header["appver"], VERSION);
            let (body, group) = match path.as_str() {
                ACCOUNT => {
                    assert!(args.is_empty());
                    (
                        json!({"code":200,"profile":{"userId":2,"nickname":"合成听书账号"}}),
                        None,
                    )
                }
                ASSIGNMENT_PATH => {
                    assert_eq!(json!(args), assignment_arguments());
                    let group = if self.treatment.load(Ordering::Acquire) {
                        "t1"
                    } else {
                        "c"
                    };
                    (
                        json!({"code":200,"data":[{"expName":"Podcast_ResumePlaySwitch","expGroupName":group}]}),
                        Some(group),
                    )
                }
                _ => return Err(Error::State("book proof refused another native endpoint")),
            };
            self.calls
                .lock()
                .unwrap()
                .push(json!({"path":path,"group":group,"sameCredential":true}));
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&body)?,
                body_status: BodyStatus::Complete,
                set_cookies: vec![],
                refresh_token: None,
                metadata: BTreeMap::new(),
            })
        })
    }
}
