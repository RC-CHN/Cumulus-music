//! Closed PC EAPI transport for the real episode system-liked collector.
use super::voice_songs_fixture;
use cumulus_core::{
    Error, Result,
    account::credentials::Credentials,
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{
            ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply, MusicHttpClient,
            OFFICIAL_WINDOWS_USER_AGENT, RequestOptions, RequestPlan, RoutingPolicy, WireRoute,
        },
        mconfig::{ConfigBackend, ConfigUrl, PcConfig, Snapshot},
        wire::eapi,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub const ACCOUNT: &str = "/api/w/nuser/account/get";
const SONGS: &str = "/api/v1/dj/program/tracks";
const LIST: &str = "/api/user/playlist";
const DETAIL: &str = "/api/v6/playlist/detail";
const LIKED: &str = "/api/song/like/get";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";
const EXPECTED: [&str; 16] = [
    ACCOUNT, SONGS, LIST, DETAIL, LIST, DETAIL, LIST, DETAIL, LIKED, ADD, LIST, DETAIL, LIKED,
    LIST, DETAIL, LIKED,
];
const VERSION: &str = "3.1.42.205594";
const TOKEN: &str = "synthetic-episode-system-liked-session";
const SIGN: &str = "synthetic-episode-system-liked-client-sign";
const BUSINESS: &str = "synthetic-group-like";
const CHECK_TOKEN: &str = "synthetic-group-like-check-token";

struct ClosedConfig;
impl ConfigBackend for ClosedConfig {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::State(
                "sealed group liked config cannot use remote HTTP",
            ))
        })
    }
}
impl TokenProvider for ClosedConfig {
    fn token<'a>(&'a self, business: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async move {
            assert_eq!(business, BUSINESS);
            Ok(CHECK_TOKEN.to_owned())
        })
    }
}

pub struct Fixture {
    pub client: MusicHttpClient,
    backend: Arc<Wire>,
}

struct Wire {
    songs: Arc<voice_songs_fixture::Fixture>,
    config: Arc<PcConfig>,
    device: String,
    calls: Mutex<Vec<Value>>,
}

impl Fixture {
    pub fn open(store: Arc<StateStore>, songs: Arc<voice_songs_fixture::Fixture>) -> Result<Self> {
        let origin = ApiOrigin::Pc;
        let previous = store.get("transport", "credentials", "session")?;
        let credentials = Arc::new(Credentials::open(store.clone(), origin)?);
        let initial = credentials.request_credentials(&origin.url())?;
        if previous.is_some() || !initial.cookies.is_empty() {
            return Err(Error::State(
                "group liked proof requires an isolated profile",
            ));
        }
        credentials.record_response(
            &origin.url(),
            &[format!("MUSIC_U={TOKEN}; Path=/; Secure")],
            None,
            &initial.revision,
        )?;
        // A normal signed playlist write requires loaded PC policy. Seed one
        // synthetic protected path, without contacting the config CDN or SDK.
        store.put(
            "transport",
            "pc-mconfig-v1",
            "pc",
            &serde_json::to_value(Snapshot {
                origin,
                version: 1,
                generation: 1,
                source_url: Some("https://p1.music.126.net/obj/synthetic-group-like.data".into()),
                data: Some(
                    json!({"yidun#paths":{"whiteList":[{"businessId":BUSINESS,"list":[ADD]}]}}),
                ),
                ..Default::default()
            })?,
        )?;
        let config = PcConfig::new(store, Arc::new(ClosedConfig))?;
        let context = Arc::new(RequestContext::from_pc(
            config.clone(),
            VERSION,
            Arc::new(ClosedConfig),
        )?);
        let backend = Arc::new(Wire {
            songs,
            config,
            device: initial.device_id,
            calls: Mutex::default(),
        });
        let client = MusicHttpClient::with_backend(
            credentials,
            ClientProfile {
                user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
                os_version: "Microsoft Windows 10".into(),
                client_sign: Some(SIGN.into()),
            },
            RequestOptions {
                routing: RoutingPolicy::OfficialPcPlaylists,
                ..Default::default()
            },
            Some(context),
            backend.clone(),
        )?;
        Ok(Self { client, backend })
    }

    /// Ordinary page refreshes observe server state without consuming any of
    /// the three independent signed journal membership observations.
    pub fn resource_membership(&self, body: &Value) -> Value {
        assert_eq!(*body, json!({"uid":"2"}));
        let ids = if self.backend.calls.lock().unwrap().len() == EXPECTED.len() {
            &[701, 1710, 1601, 1791][..]
        } else {
            &[701][..]
        };
        json!({"code":200,"ids":ids})
    }

    pub fn report(&self) -> Value {
        json!({
            "transport": "MusicHttpClient",
            "syntheticOnly": true,
            "signedCalls": *self.backend.calls.lock().unwrap(),
        })
    }
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
        let values = |key| {
            plan.headers["Cookie"]
                .split(';')
                .filter_map(|field| {
                    let (name, value) = field.trim().split_once('=')?;
                    (name == key).then_some(value)
                })
                .collect::<Vec<_>>()
        };
        // Check credentials without ever projecting their values to a report.
        assert!(values("MUSIC_U") == [TOKEN] && values("deviceId") == [self.device.as_str()]);
        assert!(header["deviceId"] == self.device && header["clientSign"] == SIGN);
        assert!(header["os"] == "pc" && header["appver"] == VERSION);
        assert!(header.get("MConfig-Info").is_none());
        assert!(plan.headers["MConfig-Info"] == self.config.snapshot()?.request_header(VERSION)?);
        let token = body.as_object_mut().unwrap().remove("checkToken");
        if path == ADD {
            assert!(plan.headers["X-antiCheatToken"] == CHECK_TOKEN);
            assert!(token.as_ref().and_then(Value::as_str) == Some(CHECK_TOKEN));
        } else {
            assert!(token.is_none() && !plan.headers.contains_key("X-antiCheatToken"));
        }
        Ok((path, body))
    }

    fn respond(&self, path: &str, body: &Value) -> Result<Value> {
        // Reject duplicate writes and premature or repeated reconciliation at
        // the actual signed backend boundary, not just in renderer receipts.
        let mut calls = self.calls.lock().unwrap();
        let step = calls.len();
        assert_eq!(
            Some(path),
            EXPECTED.get(step).copied(),
            "signed group liked step {step}"
        );
        let (response, observation) = match path {
            ACCOUNT => {
                assert_eq!(*body, json!({}));
                (
                    json!({"code":200,"profile":{"userId":2,"nickname":"节目系统收藏验证"}}),
                    "account",
                )
            }
            SONGS => {
                assert_eq!(*body, json!({"id":"601"}));
                (self.songs.songs(path, body.clone()), "episode")
            }
            LIST => {
                assert_eq!(*body, json!({"uid":"2","offset":"0","limit":"1000"}));
                (
                    json!({"code":200,"more":false,"playlist":[
                        {"id":81,"name":"合成已有收藏歌单","userId":2,"privacy":10},
                        {"id":82,"name":"合成系统收藏","userId":2,"creator":{"userId":2},"privacy":10,"specialType":5},
                        {"id":83,"name":"合成他人歌单","userId":3,"privacy":0}
                    ]}),
                    "star",
                )
            }
            DETAIL => {
                assert_eq!(*body, json!({"id":"82","n":"100000","s":"0"}));
                let (tracks, observed) = if step > 9 {
                    (&[1601, 1791, 1710, 701][..], "added")
                } else {
                    (&[701][..], "original")
                };
                let tracks = tracks.iter().map(|id| json!({"id":id})).collect::<Vec<_>>();
                (
                    json!({"code":200,"playlist":{
                        "id":82,"name":"合成系统收藏","userId":2,"creator":{"userId":2},
                        "privacy":10,"specialType":5,"tags":[],"trackIds":tracks
                    }}),
                    observed,
                )
            }
            LIKED => {
                assert_eq!(*body, json!({"uid":"2"}));
                let (ids, observed) = match step {
                    8 => (&[701][..], "original"),
                    12 => (&[701, 1601][..], "incomplete"),
                    15 => (&[701, 1710, 1601, 1791][..], "complete"),
                    _ => unreachable!("exact signed sequence"),
                };
                (json!({"code":200,"ids":ids}), observed)
            }
            ADD => {
                assert_eq!(
                    *body,
                    json!({"pid":"82","op":"add","trackIds":"[\"1601\",\"1791\",\"1710\"]"})
                );
                calls.push(json!([path, body, "lost_ack"]));
                return Err(Error::Transport(
                    "synthetic lost system liked acknowledgement".into(),
                ));
            }
            _ => unreachable!("exact signed sequence"),
        };
        calls.push(json!([path, body, observation]));
        Ok(response)
    }
}

impl HttpBackend for Wire {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let (path, body) = self.verified(&plan)?;
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&self.respond(&path, &body)?)?,
                body_status: BodyStatus::Complete,
                set_cookies: vec![],
                refresh_token: None,
                metadata: Default::default(),
            })
        })
    }
}
