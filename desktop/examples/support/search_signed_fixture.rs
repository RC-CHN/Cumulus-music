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
use sha2::{Digest, Sha256};
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
const PROOF_MARKER: (&str, &str, &str) = ("synthetic-proof", "signed-search-v1", "native-session");

fn record_hash(record: &Value) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(record)?)))
}

fn seed_or_restore_credentials(
    store: Arc<StateStore>,
    mode: &str,
    reopen: bool,
) -> Result<Arc<Credentials>> {
    let previous = store.get("transport", "credentials", "session")?;
    let marker = store.get(PROOF_MARKER.0, PROOF_MARKER.1, PROOF_MARKER.2)?;
    let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc)?);
    let target = ApiOrigin::Pc.url();
    let original = credentials.request_credentials(&target)?;
    if reopen {
        let login = credentials.login_credentials()?;
        let known = previous
            .as_ref()
            .zip(marker.as_ref())
            .is_some_and(|(record, marker)| {
                marker["schema"] == 1
                    && marker["mode"] == mode
                    && marker["device"] == original.device_id
                    && record_hash(record).is_ok_and(|hash| marker["recordHash"] == hash)
            });
        if !known
            || original.cookies != [("MUSIC_U".into(), MUSIC_U.into())]
            || login.token.as_deref() != Some(MUSIC_U)
            || login.refresh_token.is_some()
        {
            return Err(Error::State(
                "signed search reopen requires its exact synthetic fixture",
            ));
        }
        // The second process reuses this proof's own record unchanged. It does
        // not rotate the device/revision or copy an external saved account.
        return Ok(credentials);
    }
    if previous.is_some() || marker.is_some() || !original.cookies.is_empty() {
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
    let record = store
        .get("transport", "credentials", "session")?
        .ok_or(Error::State(
            "synthetic credential record was not committed",
        ))?;
    store.put(PROOF_MARKER.0, PROOF_MARKER.1, PROOF_MARKER.2,
        &json!({"schema":1,"mode":mode,"device":original.device_id,"recordHash":record_hash(&record)?}))?;
    Ok(credentials)
}

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
pub fn presentation_enabled() -> bool {
    std::env::var("CUMULUS_PC_PRESENTATION_PROOF").as_deref() == Ok("1")
}

struct ServerState {
    mode: String,
    generation: u64,
}
impl SignedSearch {
    pub fn open(store: Arc<StateStore>) -> Result<Arc<Self>> {
        let mode = std::env::var("CUMULUS_SEARCH_FOOTER_PROOF").unwrap_or_else(|_| "t1".into());
        if !matches!(mode.as_str(), "t1" | "c" | "unknown" | "error") {
            return Err(Error::InvalidInput("invalid sealed search footer cohort"));
        }
        let credentials = seed_or_restore_credentials(
            store.clone(),
            &mode,
            std::env::var("CUMULUS_PROOF_REOPEN").as_deref() == Ok("1"),
        )?;
        let config = PcConfig::new(store, Arc::new(ClosedConfig))?;
        let backend = Arc::new(Backend {
            device: credentials.device_id()?,
            config: config.clone(),
            server: Arc::new(Mutex::new(ServerState {
                mode: mode.clone(),
                generation: 0,
            })),
            initial_mode: mode,
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
        let state = self.backend.server.lock().unwrap();
        json!({"transport":"MusicHttpClient", "syntheticOnly":true,
            "sameProfileAccountRestore":self.backend.account_restored.load(Ordering::Acquire),
            "footerCase":self.backend.initial_mode, "serverMode":state.mode,
            "serverGeneration":state.generation, "calls":self.backend.calls.lock().unwrap().clone()})
    }

    pub fn set_server_mode(&self, mode: &str) -> Result<()> {
        if !presentation_enabled() || !matches!(mode, "t1" | "c" | "error") {
            return Err(Error::InvalidInput(
                "invalid sealed presentation server mode",
            ));
        }
        let mut state = self.backend.server.lock().unwrap();
        state.mode = mode.into();
        state.generation += 1;
        Ok(())
    }

    pub fn full_reads(&self) -> usize {
        self.backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call["path"] == ASSIGNMENT_PATH)
            .count()
    }
}

struct Backend {
    device: String,
    config: Arc<PcConfig>,
    initial_mode: String,
    server: Arc<Mutex<ServerState>>,
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
                let (mode, generation) = {
                    let state = self.server.lock().unwrap();
                    (state.mode.clone(), state.generation)
                };
                if mode == "error" {
                    self.calls.lock().unwrap().push(json!({"path":path,"args":args,
                        "signedPcContextVerified":true,"sameAccountCredential":true,"response":"synthetic_error",
                        "serverMode":mode,"serverGeneration":generation}));
                    return Err(Error::Transport("sealed assignment unavailable".into()));
                }
                let mut groups = layout_rows();
                if presentation_enabled() {
                    for row in &mut groups {
                        if row["expName"] != "PH-PC-searchHot" {
                            row["expGroupName"] = json!(mode);
                        }
                    }
                }
                groups.push(json!({"expName":"PH-PC-search-result",
                    "expGroupName":if mode == "unknown" {"future"} else {mode.as_str()}}));
                self.calls
                    .lock()
                    .unwrap()
                    .push(json!({"path":path,"args":args,
                    "signedPcContextVerified":true,"sameAccountCredential":true,"responseCode":200,
                    "serverMode":mode,"serverGeneration":generation}));
                json!({"code":200,"data":groups})
            };
            // No cookie, signature, raw device identifier, credential revision or
            // encrypted body enters the WebView report or public test evidence.
            if path == ACCOUNT {
                self.calls.lock().unwrap().push(json!({"path":path,"args":args,
                    "signedPcContextVerified":true,"sameAccountCredential":true,"responseCode":200}));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_reopen_keeps_exact_fixture_credentials_device_and_revision() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let first = seed_or_restore_credentials(store.clone(), "t1", false).unwrap();
        let before = first.request_credentials(&ApiOrigin::Pc.url()).unwrap();
        drop(first);
        let restored = seed_or_restore_credentials(store.clone(), "t1", true).unwrap();
        let after = restored.request_credentials(&ApiOrigin::Pc.url()).unwrap();
        assert_eq!(after.device_id, before.device_id);
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.cookies, before.cookies);
        assert!(seed_or_restore_credentials(store, "t1", false).is_err());
    }

    #[test]
    fn reopen_rejects_foreign_unmarked_or_different_cohort_records() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let credentials = seed_or_restore_credentials(store.clone(), "t1", false).unwrap();
        assert!(seed_or_restore_credentials(store.clone(), "c", true).is_err());
        let marker = store
            .get(PROOF_MARKER.0, PROOF_MARKER.1, PROOF_MARKER.2)
            .unwrap()
            .unwrap();
        store
            .delete(PROOF_MARKER.0, PROOF_MARKER.1, PROOF_MARKER.2)
            .unwrap();
        // The fixed synthetic token alone cannot authorize fixture recovery.
        assert!(seed_or_restore_credentials(store.clone(), "t1", true).is_err());
        store
            .put(PROOF_MARKER.0, PROOF_MARKER.1, PROOF_MARKER.2, &marker)
            .unwrap();
        let snapshot = credentials
            .request_credentials(&ApiOrigin::Pc.url())
            .unwrap();
        credentials
            .record_response(
                &ApiOrigin::Pc.url(),
                &["MUSIC_U=foreign-synthetic-only; Path=/; Secure".into()],
                None,
                &snapshot.revision,
            )
            .unwrap();
        assert!(seed_or_restore_credentials(store.clone(), "t1", true).is_err());
    }
}
