//! Native synthetic service for ordinary phone entry/registration. No sockets.
use cumulus_core::{
    Error,
    protocol::{
        api::ApiFuture,
        http::{BodyStatus, HttpBackend, HttpReply, RequestPlan},
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
#[derive(Default)]
pub struct Service {
    inner: super::auth::Service,
    mode: Mutex<String>,
    held: Mutex<Vec<oneshot::Sender<()>>>,
    pending: AtomicBool,
    dropped: AtomicU64,
    paths: Mutex<BTreeMap<String, u64>>,
}
impl Service {
    pub fn command(&self, mode: &str) {
        if mode == "read" {
            return;
        }
        if mode == "release" {
            for tx in self.held.lock().unwrap().drain(..) {
                let _ = tx.send(());
            }
        }
        assert!(
            [
                "new",
                "existing",
                "no-default",
                "check-failed",
                "hold-check",
                "send-failed",
                "send-lost",
                "bad-code",
                "hint-failed",
                "check-name-failed",
                "hold-hint",
                "taken",
                "sms-invalid",
                "phone-registered",
                "anti-abuse",
                "rate-limit",
                "lost-register",
                "partial-register",
                "hold-register",
                "release"
            ]
            .contains(&mode)
        );
        *self.mode.lock().unwrap() = if mode == "release" { "new" } else { mode }.into();
    }
    pub fn snapshot(&self) -> Value {
        json!({"paths":*self.paths.lock().unwrap(), "pending":self.pending.load(Ordering::SeqCst), "dropped":self.dropped.load(Ordering::SeqCst)})
    }
}
struct Held<'a>(&'a Service);
impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.0.pending.store(false, Ordering::SeqCst);
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
impl HttpBackend for Service {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let params = std::str::from_utf8(&plan.body)
                .unwrap()
                .split('&')
                .find_map(|p| p.strip_prefix("params="))
                .unwrap();
            let (path, data) = eapi::unwrap(params)?;
            *self.paths.lock().unwrap().entry(path.clone()).or_default() += 1;
            let mode = self.mode.lock().unwrap().clone();
            if path == "/api/cellphone/existence/check" && mode == "hold-check"
                || path == "/api/w/register/cellphone" && mode == "hold-register"
                || path == "/api/nickname/hint/get" && mode == "hold-hint"
            {
                let (tx, rx) = oneshot::channel();
                self.held.lock().unwrap().push(tx);
                self.pending.store(true, Ordering::SeqCst);
                let held = Held(self);
                rx.await
                    .map_err(|_| Error::Transport("synthetic release dropped".into()))?;
                drop(held);
            }
            let mut response = match path.as_str() {
                "/api/rtrs/abt/front/expinfo/list" => super::auth::reply(
                    json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":"t1"},{"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"},{"expName":"massagechange","expGroupName":"t1"}]}),
                    None,
                ),
                "/api/cellphone/existence/check" => {
                    assert_eq!(data["cellphone"], "13800138000");
                    assert_eq!(data["ctcode"], "86");
                    assert!(data.get("fromPage").is_none());
                    super::auth::reply(
                        if mode == "check-failed" {
                            json!({"code":503})
                        } else {
                            json!({"code":200,"exist":if mode=="existing"{1}else{-1},"canSkipSetNickname":mode!="no-default"})
                        },
                        None,
                    )
                }
                "/api/middle/captcha/sent/v1" => {
                    assert_eq!(data["cellphone"], "13800138000");
                    assert_eq!(data["ctcode"], "86");
                    assert_eq!(data["scene"], "0");
                    assert_eq!(data["secrete"], "music_middleuser_pclogin");
                    assert!(data.get("fromPage").is_none());
                    if mode == "send-lost" {
                        return Err(Error::Transport("synthetic lost SMS send".into()));
                    }
                    super::auth::reply(json!({"code":if mode=="send-failed"{503}else{200}}), None)
                }
                "/api/sms/captcha/verify" => {
                    assert_eq!(data["cellphone"], "13800138000");
                    assert_eq!(data["ctcode"], "86");
                    assert_eq!(data["captcha"], "123456");
                    assert!(data.get("fromPage").is_none());
                    super::auth::reply(json!({"code":if mode=="bad-code"{503}else{200}}), None)
                }
                "/api/nickname/hint/get" => {
                    assert_eq!(data["isAlter"], "false");
                    if mode == "hint-failed" {
                        return Err(Error::Transport("synthetic lost nickname hint".into()));
                    }
                    super::auth::reply(json!({"code":200,"hintNickname":"合成新昵称"}), None)
                }
                "/api/nickname/duplicated" => {
                    if mode == "check-name-failed" {
                        return Err(Error::Transport("synthetic nickname check failure".into()));
                    }
                    super::auth::reply(json!({"code":200,"duplicated":mode=="taken"}), None)
                }
                "/api/w/register/cellphone" => {
                    assert_eq!(data["phone"], "13800138000");
                    assert_eq!(data["countrycode"], "86");
                    assert_eq!(data["captcha"], "123456");
                    assert_eq!(data["password"], "");
                    assert_eq!(data["force"], "false");
                    assert!(
                        data.get("fromPage").is_none()
                            && data.get("ursToken").is_none()
                            && !plan.headers.contains_key("x-login-chain-id")
                    );
                    assert!(data.get("nickname").is_some() ^ (data["isSkipSetNickname"] == "true"));
                    if mode == "lost-register" {
                        return Err(Error::Transport("synthetic lost registration reply".into()));
                    }
                    let code = match mode.as_str() {
                        "sms-invalid" => 503,
                        "phone-registered" => 504,
                        "anti-abuse" => 250,
                        "rate-limit" => 509,
                        _ => 200,
                    };
                    let mut r = super::auth::reply(
                        json!({"code":code}),
                        if code == 200 { Some(5) } else { None },
                    );
                    if mode == "partial-register" {
                        r.body_status = BodyStatus::Interrupted;
                    }
                    r
                }
                "/api/login/cellphone" | "/api/w/login/cellphone" => {
                    assert_eq!(data["phone"], "13800138000");
                    assert_eq!(data["captcha"], "123456");
                    super::auth::reply(json!({"code":200}), Some(5))
                }
                "/api/w/nuser/account/get" => {
                    let uid = plan
                        .headers
                        .get("Cookie")
                        .and_then(|c| {
                            c.split(';')
                                .find_map(|p| p.trim().strip_prefix("MUSIC_U=fixture"))
                        })
                        .and_then(|u| u.parse::<u64>().ok());
                    super::auth::reply(
                        match uid {
                            Some(uid @ (2 | 5)) => {
                                json!({"code":200,"profile":{"userId":uid,"nickname":if uid==5{"注册账号"}else{"邮箱验证账号"}}})
                            }
                            _ => json!({"code":301}),
                        },
                        None,
                    )
                }
                _ => return self.inner.post(plan).await,
            };
            // A partial success must never install the accompanying refresh header.
            if mode == "partial-register" && path == "/api/w/register/cellphone" {
                response.refresh_token = Some("fixture-partial-must-not-install".into());
            }
            Ok(response)
        })
    }
}
