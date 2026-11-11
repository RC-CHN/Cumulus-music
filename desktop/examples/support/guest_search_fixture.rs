//! Sealed real PC transport for a fresh guest WebView, with no real account.
use cumulus_core::{
    Error, Result,
    account::{anonymous_identity::encode_anonymous_id, credentials::Credentials},
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{
            ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply, MusicHttpClient,
            OFFICIAL_WINDOWS_USER_AGENT, RequestOptions, RequestPlan, WireRoute,
        },
        mconfig::{ConfigBackend, ConfigUrl, PcConfig},
        pc_experiments::{ANONYMOUS_PATH, ASSIGNMENT_PATH, assignment_arguments},
        wire::eapi,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const ACCOUNT: &str = "/api/w/nuser/account/get";
const MUSIC_A: &str = "sealed-guest-search";
const SIGN: &str = "synthetic-guest-search-sign";
const VERSION: &str = "3.1.42.205594";

pub fn enabled() -> bool {
    std::env::var("CUMULUS_GUEST_SEARCH_PROOF").as_deref() == Ok("1")
}

struct Offline;
impl ConfigBackend for Offline {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("sealed guest fixture cannot fetch configuration") })
    }
}
impl TokenProvider for Offline {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async { panic!("sealed guest fixture cannot obtain an SDK token") })
    }
}

pub struct Wire {
    device: String,
    calls: Mutex<Vec<Value>>,
}

impl Wire {
    pub fn open(store: Arc<StateStore>) -> Result<(Arc<MusicHttpClient>, Arc<Self>)> {
        if store.get("transport", "credentials", "session")?.is_some() {
            return Err(Error::State(
                "guest search proof requires a fresh synthetic store",
            ));
        }
        let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc)?);
        let config = PcConfig::new(store, Arc::new(Offline))?;
        let wire = Arc::new(Self {
            device: credentials.device_id()?,
            calls: Mutex::default(),
        });
        let client = Arc::new(MusicHttpClient::with_backend(
            credentials,
            ClientProfile {
                user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
                os_version: "Microsoft Windows 10".into(),
                client_sign: Some(SIGN.into()),
            },
            RequestOptions::default(),
            Some(Arc::new(RequestContext::from_pc(
                config,
                VERSION,
                Arc::new(Offline),
            )?)),
            wire.clone(),
        )?);
        Ok((client, wire))
    }

    pub fn report(&self) -> Vec<Value> {
        self.calls.lock().unwrap().clone()
    }
}

impl HttpBackend for Wire {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            assert_eq!(plan.route, WireRoute::Eapi);
            assert_eq!(plan.url.origin(), ApiOrigin::Pc.url().origin());
            assert_eq!(plan.headers["User-Agent"], OFFICIAL_WINDOWS_USER_AGENT);
            let mut fields = url::form_urlencoded::parse(&plan.body);
            let (key, encrypted) = fields.next().expect("sealed EAPI body");
            assert_eq!(key, "params");
            assert!(fields.next().is_none());
            let (path, mut args) = eapi::unwrap(&encrypted)?;
            assert_eq!(
                plan.url.path(),
                ApiOrigin::Pc.request_url(WireRoute::Eapi, &path)?.path()
            );
            let fields = args.as_object_mut().expect("sealed EAPI object");
            let header = fields.remove("header").expect("sealed EAPI header");
            let header: Value = if let Some(text) = header.as_str() {
                serde_json::from_str(text)?
            } else {
                header
            };
            fields.remove("e_r");
            assert_eq!(header["deviceId"], self.device);
            assert_eq!(header["clientSign"], SIGN);
            assert_eq!(header["os"], "pc");
            assert_eq!(header["appver"], VERSION);
            let cookie = &plan.headers["Cookie"];
            let anonymous = cookie
                .split(';')
                .map(str::trim)
                .filter_map(|pair| pair.strip_prefix("MUSIC_A="))
                .collect::<Vec<_>>();
            assert!(
                cookie
                    .split(';')
                    .map(str::trim)
                    .all(|pair| !pair.starts_with("MUSIC_U="))
            );
            let mut set_cookies = vec![];
            let before_registration = matches!(path.as_str(), ACCOUNT | ANONYMOUS_PATH);
            if before_registration {
                assert!(anonymous.is_empty());
            } else {
                assert_eq!(anonymous, [MUSIC_A]);
            }
            let response = match path.as_str() {
                ACCOUNT => {
                    assert_eq!(args, json!({}));
                    json!({"code":301})
                }
                ANONYMOUS_PATH => {
                    assert_eq!(args, json!({"username":encode_anonymous_id(&self.device)?}));
                    args = json!({"usernameVerified":true});
                    set_cookies.push(format!("MUSIC_A={MUSIC_A}; Path=/; Secure"));
                    json!({"code":200,"userId":9001})
                }
                ASSIGNMENT_PATH => {
                    assert_eq!(args, assignment_arguments());
                    args = json!({"fullBatchVerified":true});
                    json!({"code":200,"data":[{"expName":"PH-PC-searchHot","expGroupName":"c"}]})
                }
                "/api/search/default/keyword/get"
                | "/api/search/pc/suggest/keyword/get"
                | "/api/search/pc/rcmd/keyword/get"
                | "/api/search/pc/chart/list"
                | "/api/search/pc/chart/detail"
                | "/api/search/pc/complex/page"
                | "/api/search/song/list/page" => {
                    super::search_fixture::response(&path, &args)
                        .ok_or(Error::State("sealed guest fixture has no search response"))??
                }
                _ => panic!("unexpected endpoint in sealed guest search: {path}"),
            };
            self.calls
                .lock()
                .unwrap()
                .push(json!({"path":path,"args":args,
                "anonymousCookieVerified":!before_registration,"signedPcContextVerified":true}));
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&response)?,
                body_status: BodyStatus::Complete,
                set_cookies,
                refresh_token: None,
                metadata: Default::default(),
            })
        })
    }
}

/// A fresh QR candidate uses its own native backend and cannot borrow the
/// anonymous search cookie. No account can be authorized by this fixture.
#[derive(Default)]
pub struct LoginWire(Mutex<Vec<String>>);

impl LoginWire {
    pub fn report(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl HttpBackend for LoginWire {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            const KEY: &str = "synthetic-guest-share-login";
            assert_eq!(plan.route, WireRoute::Eapi);
            assert_eq!(plan.url.origin(), ApiOrigin::Pc.url().origin());
            let mut fields = url::form_urlencoded::parse(&plan.body);
            let (key, encrypted) = fields.next().expect("isolated login EAPI body");
            assert_eq!(key, "params");
            assert!(fields.next().is_none());
            let (path, args) = eapi::unwrap(&encrypted)?;
            assert_eq!(
                plan.url.path(),
                ApiOrigin::Pc.request_url(WireRoute::Eapi, &path)?.path()
            );
            if let Some(cookie) = plan.headers.get("Cookie") {
                assert!(cookie.split(';').map(str::trim).all(|pair| {
                    !pair.starts_with("MUSIC_A=") && !pair.starts_with("MUSIC_U=")
                }));
            }
            assert_eq!(args["type"], "3");
            let response = match path.as_str() {
                "/api/login/qrcode/unikey" => json!({"code":200,"unikey":KEY}),
                "/api/login/qrcode/client/login" => {
                    assert_eq!(args["key"], KEY);
                    assert_eq!(args["secureCaptcha"], "");
                    json!({"code":801})
                }
                _ => return Err(Error::State("unexpected sealed guest QR request")),
            };
            self.0.lock().unwrap().push(path);
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&response)?,
                body_status: BodyStatus::Complete,
                set_cookies: vec![],
                refresh_token: None,
                metadata: Default::default(),
            })
        })
    }
}
