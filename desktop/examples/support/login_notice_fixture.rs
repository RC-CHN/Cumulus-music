//! Native-only synthetic notice/penalty/security-QR service. No sockets.
use super::auth;
use cumulus_core::{
    Error, Result,
    account::login::presentation::LoginDocument,
    client::LoginCaptchaProvider,
    protocol::{
        api::ApiFuture,
        http::{HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::sync::oneshot;

const NATIVE_PROOF: &str = "synthetic-native-only-completed-proof";
const SEED_KEY: &str = "synthetic-native-only-security-ticket";
const COMMUNITY_BADGE: &str = "/api/user/friend/new/song/notice/web/get";

struct Held {
    kind: &'static str,
    sender: oneshot::Sender<()>,
}

#[derive(Default)]
pub struct Service {
    base: auth::Service,
    mode: Mutex<String>,
    paths: Mutex<BTreeMap<String, u64>>,
    mode_paths: Mutex<BTreeMap<String, BTreeMap<String, u64>>>,
    login_codes: Mutex<Vec<i64>>,
    community_badge_reads: Mutex<BTreeMap<u64, u64>>,
    held: Mutex<Option<Held>>,
    browser_documents: Mutex<Vec<LoginDocument>>,
    captcha_calls: AtomicU64,
    proof_polls: AtomicU64,
    captcha_solved: AtomicBool,
    legacy_creates: AtomicU64,
    legacy_polls: AtomicU64,
    legacy_mode_polls: Mutex<BTreeMap<String, u64>>,
    identity_301: AtomicU64,
    identity_authorized: AtomicU64,
    released: AtomicU64,
    cancelled_replies: AtomicU64,
    unexpected: AtomicU64,
}
impl Service {
    pub fn command(&self, mode: &str) -> Result<()> {
        match mode {
            "inspect" => {}
            "release" => {
                let held = self
                    .held
                    .lock()
                    .unwrap()
                    .take()
                    .ok_or(Error::State("no synthetic reply is held"))?;
                self.released.fetch_add(1, Ordering::SeqCst);
                if held.sender.send(()).is_err() {
                    self.cancelled_replies.fetch_add(1, Ordering::SeqCst);
                }
            }
            "normal"
            | "seed_old"
            | "risk_8810"
            | "risk_10004"
            | "disabled"
            | "upgrade_8820"
            | "upgrade_8810"
            | "penalty_upgrade"
            | "penalty_identity"
            | "penalty_unknown"
            | "penalty_status_fail"
            | "penalty_status_unknown"
            | "penalty_status_passed"
            | "qr_seed"
            | "qr_802"
            | "qr_expired"
            | "qr_refresh"
            | "qr_captcha"
            | "qr_hold"
            | "qr_authorize" => {
                *self.mode.lock().unwrap() = mode.into();
                if matches!(mode, "qr_seed" | "qr_hold" | "qr_authorize") {
                    self.captcha_solved.store(false, Ordering::SeqCst);
                }
            }
            _ => return Err(Error::InvalidInput("unknown synthetic notice mode")),
        }
        Ok(())
    }

    pub fn open_document(&self, document: LoginDocument) -> Result<()> {
        self.browser_documents.lock().unwrap().push(document);
        Ok(())
    }

    pub fn snapshot(&self) -> Value {
        let (pending, held_receiver_closed) = {
            let held = self.held.lock().unwrap();
            (
                held.as_ref().map(|held| held.kind),
                held.as_ref().is_some_and(|held| held.sender.is_closed()),
            )
        };
        json!({
            "mode": *self.mode.lock().unwrap(),
            "paths": *self.paths.lock().unwrap(),
            "modePaths": *self.mode_paths.lock().unwrap(),
            "loginCodes": *self.login_codes.lock().unwrap(),
            "communityBadgeReads": *self.community_badge_reads.lock().unwrap(),
            "pending": pending,
            "heldReceiverClosed": held_receiver_closed,
            "browserDocuments": *self.browser_documents.lock().unwrap(),
            "captchaCalls": self.captcha_calls.load(Ordering::SeqCst),
            "proofPolls": self.proof_polls.load(Ordering::SeqCst),
            "legacyCreates": self.legacy_creates.load(Ordering::SeqCst),
            "legacyPolls": self.legacy_polls.load(Ordering::SeqCst),
            "legacyModePolls": *self.legacy_mode_polls.lock().unwrap(),
            "identity301": self.identity_301.load(Ordering::SeqCst),
            "identityAuthorized": self.identity_authorized.load(Ordering::SeqCst),
            "released": self.released.load(Ordering::SeqCst),
            "cancelledReplies": self.cancelled_replies.load(Ordering::SeqCst),
            "unexpectedRequests": self.unexpected.load(Ordering::SeqCst)
        })
    }

    async fn hold(&self, kind: &'static str) -> Result<()> {
        let (sender, receiver) = oneshot::channel();
        if self
            .held
            .lock()
            .unwrap()
            .replace(Held { kind, sender })
            .is_some()
        {
            return Err(Error::State("overlapping synthetic held replies"));
        }
        receiver.await.map_err(|_| Error::Cancelled)
    }

    fn penalty_steps(mode: &str) -> Value {
        match mode {
            "penalty_upgrade" => json!([
                {"type":"IDENTITY_VERIFY","state":"PENDING","minVersion":"official-identity-v2"},
                {"type":"UPGRADE_VERSION","state":"PENDING","minVersion":"official-component-v3"}
            ]),
            "penalty_identity" => json!([
                {"type":"IDENTITY_VERIFY","state":"PENDING","minVersion":"official-identity-v2"},
                {"type":"UPGRADE_WEAK_PASSWORD","state":"PENDING"}
            ]),
            "penalty_status_passed" => json!([
                {"type":"IDENTITY_VERIFY","state":"PASSED"},
                {"type":"FUTURE_SECURITY_ACTION","state":"PASSED"}
            ]),
            "penalty_unknown" => json!(std::iter::once(json!({"type":"IDENTITY_VERIFY","state":"PASSED"}))
                .chain((1..32).map(|index| json!({"type":format!("FUTURE_SECURITY_ACTION_{index}"),"state":"PENDING"})))
                .collect::<Vec<_>>()),
            _ => json!([
                {"type":"IDENTITY_VERIFY","state":"PASSED"},
                {"type":"FUTURE_SECURITY_ACTION","state":"PENDING"}
            ]),
        }
    }
}

impl LoginCaptchaProvider for Service {
    fn solve(&self) -> ApiFuture<'_, String> {
        Box::pin(async move {
            self.captcha_calls.fetch_add(1, Ordering::SeqCst);
            Ok(NATIVE_PROOF.into())
        })
    }
}

impl HttpBackend for Service {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let form = std::str::from_utf8(&plan.body)
                .map_err(|_| Error::Response("synthetic notice form"))?;
            let params = form
                .split('&')
                .find_map(|p| p.strip_prefix("params="))
                .ok_or(Error::Response("synthetic notice EAPI"))?;
            let (path, data) = eapi::unwrap(params)?;
            let mode = self.mode.lock().unwrap().clone();
            *self.paths.lock().unwrap().entry(path.clone()).or_default() += 1;
            *self
                .mode_paths
                .lock()
                .unwrap()
                .entry(mode.clone())
                .or_default()
                .entry(path.clone())
                .or_default() += 1;
            let uid = plan
                .headers
                .get("Cookie")
                .and_then(|cookies| {
                    cookies
                        .split(';')
                        .find_map(|cookie| cookie.trim().strip_prefix("MUSIC_U=fixture"))
                })
                .and_then(|uid| uid.parse::<u64>().ok());

            if path == "/api/search/default/keyword/get" {
                return Ok(auth::reply(
                    json!({"code":200,"data":{"refreshTime":60000,"keywords":[{"realkeyword":"合成搜索","showKeyword":"合成搜索"}]}}),
                    None,
                ));
            }
            if path == "/api/relay/setting/show" {
                return Ok(auth::reply(json!({"code":200,"data":{"groups":[]}}), None));
            }
            if path == COMMUNITY_BADGE {
                // Official M198 reads the passive followed-song badge after
                // adoption. Only the two synthetic selected users and the
                // uncached empty business body are valid in this probe.
                let expected = match mode.as_str() {
                    "seed_old" => Some(2),
                    "qr_authorize" => Some(5),
                    _ => None,
                };
                let empty_body = data.as_object().is_some_and(|fields| {
                    fields.contains_key("header")
                        && fields
                            .keys()
                            .all(|key| matches!(key.as_str(), "header" | "e_r"))
                });
                if let Some(uid) = uid.filter(|uid| Some(*uid) == expected)
                    && plan.url.path() == "/eapi/user/friend/new/song/notice/web/get"
                    && plan.url.query().is_none()
                    && empty_body
                {
                    let mut reads = self.community_badge_reads.lock().unwrap();
                    let count = reads.entry(uid).or_default();
                    *count += 1;
                    if *count == 1 {
                        return Ok(auth::reply(
                            json!({"code":200,"data":{"style":"musician-empty","time":0}}),
                            None,
                        ));
                    }
                }
                self.unexpected.fetch_add(1, Ordering::SeqCst);
                return Err(Error::Response(
                    "unexpected synthetic followed-song badge request",
                ));
            }
            if path == "/api/login/token/refresh/v1" {
                // The inherited fixture enables both official login flags.
                // After adopting the new QR account, the production renewal
                // pump performs one check. Model a complete headerless reply;
                // do not silently accept an unexpected account or credential.
                if mode != "qr_authorize"
                    || uid != Some(5)
                    || data["clientUserId"] != "5"
                    || data["refreshToken"] != "fixture-refresh"
                    || data["_scver"] != "1"
                    || data["_sign"].as_str().is_none_or(|value| value.is_empty())
                {
                    return Err(Error::Response("synthetic QR renewal context"));
                }
                return Ok(auth::reply(json!({"code":200}), None));
            }
            if path == "/api/login" {
                if data["fromPage"] != "JSK"
                    || data["username"] != "notice@example.test"
                    || data["password"] != eapi::md5_hex(b"fixture-password")
                    || plan.headers.get("x-login-chain-id").map(String::as_str) != Some("")
                {
                    return Err(Error::Response("synthetic notice credential context"));
                }
                let (value, login_uid) = match mode.as_str() {
                    "seed_old" => (json!({"code":200}), Some(2)),
                    "risk_8810" => (
                        json!({"code":8810,"toast":{"type":3,
                        "title":"合成账号风险说明","content":["检测到合成登录环境需要进一步确认。","请在官方客户端确认账号安全后，再选择合适的登录方式。"],
                        "url":"https://example.invalid/private-authorization","private":"NATIVE_NOTICE_PRIVATE"}}),
                        None,
                    ),
                    "risk_10004" => {
                        let mut paragraphs =
                            vec!["<strong>合成文字，应作为正文显示</strong>".to_owned()];
                        paragraphs.extend((1..=8).map(|index| {
                            format!(
                                "第{index}段合成安全说明：{}",
                                "这段文字只用于检查长文换行、滚动和焦点，不包含真实账号信息。"
                                    .repeat(4)
                            )
                        }));
                        (
                            json!({"code":10004,"toast":{"type":3,"title":"合成风险说明 — 10004","content":paragraphs}}),
                            None,
                        )
                    }
                    "disabled" => (json!({"code":304}), None),
                    "upgrade_8820" => (json!({"code":8820}), None),
                    "upgrade_8810" => (
                        json!({"code":8810,"toast":{"type":2,"title":"NATIVE_NOTICE_PRIVATE"}}),
                        None,
                    ),
                    "penalty_upgrade" | "penalty_identity" | "penalty_unknown" => (
                        json!({"code":8831,"data":{"steps":Self::penalty_steps(&mode),"musicTuP":"NATIVE_PENALTY_PRIVATE"}}),
                        None,
                    ),
                    "qr_seed" | "qr_hold" | "qr_authorize" => (
                        json!({"code":8810,"toast":{"type":1,"unikey":SEED_KEY,"nickname":"合成安全验证账号","private":"NATIVE_QR_PRIVATE"}}),
                        None,
                    ),
                    _ => return Err(Error::Response("unexpected synthetic notice login")),
                };
                self.login_codes
                    .lock()
                    .unwrap()
                    .push(value["code"].as_i64().unwrap());
                let penalty = value["code"] == 8831;
                let mut reply = auth::reply(value, login_uid);
                if penalty {
                    reply
                        .set_cookies
                        .push("MUSIC_TU=synthetic-penalty-tu; Path=/; Secure; HttpOnly".into());
                }
                return Ok(reply);
            }
            if path == "/api/login/penalty/status" {
                if data["fromPage"] != "JSK"
                    || ["musicTuP", "password", "ignorePenalty", "stepType"]
                        .iter()
                        .any(|key| data.get(*key).is_some())
                    || plan.headers.contains_key("x-music-tu-p")
                {
                    return Err(Error::Response("synthetic penalty read fields"));
                }
                if mode == "penalty_status_fail" {
                    return Err(Error::Transport("合成状态读取失败".into()));
                }
                if !matches!(
                    mode.as_str(),
                    "penalty_status_unknown" | "penalty_status_passed"
                ) {
                    return Err(Error::Response("unexpected synthetic penalty read"));
                }
                return Ok(auth::reply(
                    json!({"code":200,"data":{"steps":Self::penalty_steps(&mode)}}),
                    None,
                ));
            }
            if path == "/api/login/qrcode/unikey" && data["type"] == "3" {
                self.legacy_creates.fetch_add(1, Ordering::SeqCst);
                if data["lastUniKey"] != SEED_KEY || data.get("fromPage").is_some() {
                    return Err(Error::Response("synthetic security QR refresh context"));
                }
                return Ok(auth::reply(
                    json!({"code":200,"unikey":"synthetic-native-only-refreshed-ticket"}),
                    None,
                ));
            }
            if path == "/api/login/qrcode/client/login" {
                if data["type"] == "5" {
                    return Ok(auth::reply(json!({"code":801}), None));
                }
                if data["type"] != "3" || data.get("fromPage").is_some() {
                    return Err(Error::Response("synthetic security QR poll context"));
                }
                self.legacy_polls.fetch_add(1, Ordering::SeqCst);
                *self
                    .legacy_mode_polls
                    .lock()
                    .unwrap()
                    .entry(mode.clone())
                    .or_default() += 1;
                let expected_key = if matches!(
                    mode.as_str(),
                    "qr_seed" | "qr_802" | "qr_expired" | "qr_authorize"
                ) {
                    SEED_KEY
                } else {
                    "synthetic-native-only-refreshed-ticket"
                };
                if data["key"] != expected_key {
                    return Err(Error::Response("synthetic security QR candidate ticket"));
                }
                if data.get("secureCaptcha").is_some() {
                    if data["secureCaptcha"] != NATIVE_PROOF {
                        return Err(Error::Response("synthetic native CAPTCHA provider"));
                    }
                    self.proof_polls.fetch_add(1, Ordering::SeqCst);
                    self.captcha_solved.store(true, Ordering::SeqCst);
                }
                return Ok(match mode.as_str() {
                    "qr_802" => auth::reply(json!({"code":802}), None),
                    "qr_expired" => auth::reply(json!({"code":800}), None),
                    "qr_captcha" if !self.captcha_solved.load(Ordering::SeqCst) => {
                        auth::reply(json!({"code":8821}), None)
                    }
                    "qr_hold" => {
                        self.hold("qr_poll").await?;
                        auth::reply(json!({"code":803}), Some(5))
                    }
                    "qr_authorize" => auth::reply(json!({"code":803}), Some(5)),
                    _ => auth::reply(json!({"code":801}), None),
                });
            }
            if path == "/api/w/nuser/account/get" {
                if mode == "penalty_status_passed" && uid.is_none() {
                    self.identity_301.fetch_add(1, Ordering::SeqCst);
                    return Ok(auth::reply(json!({"code":301}), None));
                }
                if mode == "qr_authorize" && uid == Some(5) {
                    self.identity_authorized.fetch_add(1, Ordering::SeqCst);
                    self.hold("identity_read").await?;
                }
            }
            let reply = self.base.post(plan).await;
            if reply.is_err() {
                self.unexpected.fetch_add(1, Ordering::SeqCst);
            }
            reply
        })
    }
}
