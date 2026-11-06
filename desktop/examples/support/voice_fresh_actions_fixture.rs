//! The positive VoicePage proof runs its action authority and writes through
//! real PC EAPI planning and encryption against a closed synthetic backend.
use super::{scene_home_fixture, voice_detail_fixture, voice_page_fixture};
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
        wire::eapi,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub const ACCOUNT: &str = "/api/w/nuser/account/get";
const DETAIL: &str = "/api/dj/program/detail";
const RADIO: &str = "/api/djradio/v3/get";
const SUBSCRIBE: &str = "/api/djradio/sub";
const VERSION: &str = "3.1.42.205594";
const TOKEN: &str = "synthetic-voice-fresh-actions-session";
const SIGN: &str = "synthetic-voice-fresh-actions-client-sign";
pub const CREATOR_NAME: &str = "合成新主播";
pub const RADIO_NAME: &str = "合成新播单";

struct ClosedConfig;
impl ConfigBackend for ClosedConfig {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("sealed voice config cannot use remote HTTP")) })
    }
}
impl TokenProvider for ClosedConfig {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async { Err(Error::State("sealed voice actions cannot use SDK tokens")) })
    }
}

pub struct Fixture {
    pub client: MusicHttpClient,
    backend: Arc<Wire>,
    routes: Mutex<Vec<Value>>,
}

struct Wire {
    page: Arc<voice_page_fixture::Fixture>,
    config: Arc<PcConfig>,
    device: String,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    account_restored: bool,
    subscribed: bool,
    calls: Vec<Value>,
}

impl Fixture {
    pub fn open(store: Arc<StateStore>, page: Arc<voice_page_fixture::Fixture>) -> Result<Self> {
        let origin = ApiOrigin::Pc;
        let previous = store.get("transport", "credentials", "session")?;
        let credentials = Arc::new(Credentials::open(store.clone(), origin)?);
        let initial = credentials.request_credentials(&origin.url())?;
        if previous.is_some() || !initial.cookies.is_empty() {
            return Err(Error::State(
                "fresh voice action proof needs an isolated profile",
            ));
        }
        credentials.record_response(
            &origin.url(),
            &[format!("MUSIC_U={TOKEN}; Path=/; Secure")],
            None,
            &initial.revision,
        )?;
        let config = PcConfig::new(store, Arc::new(ClosedConfig))?;
        let context = Arc::new(RequestContext::from_pc(
            config.clone(),
            VERSION,
            Arc::new(ClosedConfig),
        )?);
        let backend = Arc::new(Wire {
            page,
            config,
            device: initial.device_id,
            state: Mutex::default(),
        });
        let client = MusicHttpClient::with_backend(
            credentials,
            ClientProfile {
                user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
                os_version: "Microsoft Windows 10".into(),
                client_sign: Some(SIGN.into()),
            },
            RequestOptions::default(),
            Some(context),
            backend.clone(),
        )?;
        Ok(Self {
            client,
            backend,
            routes: Mutex::default(),
        })
    }

    pub fn report(&self) -> Value {
        let state = self.backend.state.lock().unwrap();
        json!({"transport":"MusicHttpClient", "syntheticOnly":true,
            "sameProfileAccountRestore":state.account_restored, "subscribed":state.subscribed,
            "signedCalls":state.calls, "routeReads":self.routes.lock().unwrap().clone()})
    }

    pub fn route(&self, path: &str, body: &Value) -> Option<Result<Value>> {
        let subscribed = self.backend.state.lock().unwrap().subscribed;
        let response = match path {
            "/api/w/v1/user/detail/56" => {
                assert_eq!(*body, json!({"all":true,"userId":"56"}));
                json!({"code":200,"profile":{"userId":56,"nickname":"合成新主播主页","followed":false}})
            }
            "/api/djradio/get/create/sub/count" => {
                assert_eq!(*body, json!({"userId":"56"}));
                json!({"code":200,"createRadioCount":0,"subRadioCount":0})
            }
            "/api/user/playlist" if body["uid"] == "56" => {
                assert_eq!(body["offset"], 0);
                json!({"code":200,"playlist":[],"more":false})
            }
            RADIO => {
                assert_eq!(*body, json!({"id":"81"}));
                radio(subscribed)
            }
            "/api/v6/dj/program/byradio" => {
                assert_eq!(body["radioId"], "81");
                assert_eq!(body["offset"], 0);
                assert!(body["asc"].is_boolean());
                json!({"code":200,"data":{"programs":[],"count":0,"more":false,"asc":body["asc"]}})
            }
            "/api/djradio/get/subed" => {
                assert_eq!(*body, json!({"targetUserId":"2","limit":1000}));
                let rows = if subscribed {
                    vec![json!({"id":81})]
                } else {
                    vec![]
                };
                json!({"code":200,"count":rows.len(),"hasMore":false,"djRadios":rows})
            }
            "/api/content/my/liked/voice" => {
                assert_eq!(*body, json!({"limit":100,"likeTime":0}));
                json!({"code":200,"data":{"data":[],"hasMore":false}})
            }
            SUBSCRIBE | "/api/djradio/unsub" => {
                panic!("fresh voice collection bypassed its signed typed path")
            }
            _ => return None,
        };
        let mut routes = self.routes.lock().unwrap();
        assert!(routes.len() < 96, "unbounded synthetic voice routes");
        routes.push(json!({"path":path,"args":body}));
        Some(Ok(response))
    }
}

fn radio(subscribed: bool) -> Value {
    json!({"code":200,"data":{"id":81,"name":RADIO_NAME,"programCount":0,"subCount":9,
        "subed":subscribed,"dj":{"userId":56,"nickname":CREATOR_NAME}}})
}

impl Wire {
    fn verified(&self, plan: &RequestPlan) -> Result<(String, Value)> {
        let origin = ApiOrigin::Pc;
        assert_eq!(plan.route, WireRoute::Eapi);
        assert_eq!(plan.url.origin(), origin.url().origin());
        assert_eq!(plan.headers["User-Agent"], OFFICIAL_WINDOWS_USER_AGENT);
        let fields = url::form_urlencoded::parse(&plan.body).collect::<Vec<_>>();
        assert!(fields.len() == 1 && fields[0].0 == "params");
        let (path, mut body) = eapi::unwrap(fields[0].1.as_ref())?;
        assert_eq!(
            plan.url.path(),
            origin.request_url(WireRoute::Eapi, &path)?.path()
        );
        let raw = body.as_object_mut().unwrap().remove("header").unwrap();
        let header: Value = serde_json::from_str(raw.as_str().unwrap())?;
        let cookie = &plan.headers["Cookie"];
        let values = |key| {
            cookie
                .split(';')
                .filter_map(|field| {
                    let (name, value) = field.trim().split_once('=')?;
                    (name == key).then_some(value)
                })
                .collect::<Vec<_>>()
        };
        // Do not use assert_eq! for secret-bearing actual values or project them.
        assert!(values("MUSIC_U") == [TOKEN] && values("deviceId") == [self.device.as_str()]);
        assert!(header["deviceId"] == self.device && header["clientSign"] == SIGN);
        assert!(header["os"] == "pc" && header["appver"] == VERSION);
        assert!(header.get("MConfig-Info").is_none());
        assert!(plan.headers["MConfig-Info"] == self.config.snapshot()?.request_header(VERSION)?);
        Ok((path, body))
    }

    fn record(&self, path: &str, body: &Value, outcome: &str) {
        let mut state = self.state.lock().unwrap();
        assert!(
            state.calls.len() < 96,
            "unbounded synthetic signed voice requests"
        );
        state
            .calls
            .push(json!({"path":path,"args":body,"outcome":outcome}));
    }
}

impl HttpBackend for Wire {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let (path, body) = self.verified(&plan)?;
            assert!(path == ACCOUNT || self.state.lock().unwrap().account_restored);
            let response = match path.as_str() {
                ACCOUNT => {
                    assert_eq!(body, json!({}));
                    self.state.lock().unwrap().account_restored = true;
                    json!({"code":200,"profile":{"userId":2,"nickname":"节目动作验证"}})
                }
                DETAIL => {
                    let ticket = self
                        .page
                        .begin(voice_page_fixture::Lane::Detail, &path, &body);
                    let detail = voice_detail_fixture::enrich(scene_home_fixture::program_detail(
                        ticket.program,
                    ));
                    let reply = self.page.detail(&ticket, detail).await;
                    match self.page.finish(ticket, reply) {
                        Ok(response) => response,
                        Err(error) => {
                            self.record(&path, &body, "synthetic_failure");
                            return Err(error);
                        }
                    }
                }
                RADIO => {
                    assert_eq!(body, json!({"id":"81"}));
                    radio(self.state.lock().unwrap().subscribed)
                }
                SUBSCRIBE => {
                    assert_eq!(body, json!({"id":"81"}));
                    let mut state = self.state.lock().unwrap();
                    assert!(
                        !state.subscribed,
                        "unexpected duplicate signed subscription"
                    );
                    state.subscribed = true;
                    json!({"code":200})
                }
                _ => panic!("unexpected signed VoicePage endpoint"),
            };
            self.record(&path, &body, "success");
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&response)?,
                body_status: BodyStatus::Complete,
                set_cookies: vec![],
                refresh_token: None,
            })
        })
    }
}
