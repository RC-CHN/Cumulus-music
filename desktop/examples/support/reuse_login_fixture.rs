//! Synthetic native EAPI for the recovered phone-reuse workflow; no sockets.
use cumulus_core::{
    Error,
    protocol::{
        api::ApiFuture,
        http::{BodyStatus, HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::oneshot;
#[derive(Default)]
pub struct Service {
    inner: super::auth::Service,
    mode: Mutex<String>,
    held: Mutex<Vec<oneshot::Sender<HttpReply>>>,
    pending: AtomicBool,
    dropped: AtomicU64,
}
impl Service {
    pub fn command(&self, mode: &str) {
        assert!(
            [
                "read",
                "normal",
                "read-failed",
                "read-lost",
                "confirm-false",
                "confirm-lost",
                "hint-lost",
                "taken",
                "register-false",
                "register-lost",
                "register-sms-invalid",
                "hold-read",
                "hold-confirm",
                "hold-register",
                "hold-identity",
                "fresh-login",
                "release"
            ]
            .contains(&mode)
        );
        if mode == "read" {
            return;
        }
        if mode == "release" {
            for sender in self.held.lock().unwrap().drain(..) {
                let _ = sender.send(super::auth::reply(json!({"code":200}), Some(3)));
            }
        }
        *self.mode.lock().unwrap() = if mode == "release" {
            "normal".into()
        } else {
            mode.into()
        };
    }
    pub fn snapshot(&self) -> Value {
        json!({"paths":*self.inner.paths.lock().unwrap(),"pending":self.pending.load(Ordering::SeqCst),"dropped":self.dropped.load(Ordering::SeqCst)})
    }
}
struct Held<'a> {
    service: &'a Service,
    completed: bool,
}
impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.service.pending.store(false, Ordering::SeqCst);
        if !self.completed {
            self.service.dropped.fetch_add(1, Ordering::SeqCst);
        }
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
            let own = matches!(
                path.as_str(),
                "/api/w/login/cellphone"
                    | "/api/login/cellphone"
                    | "/api/w/nuser/account/get"
                    | "/api/nickname/hint/get"
                    | "/api/nickname/duplicated"
                    | "/api/w/register/cellphone"
            ) || path.starts_with("/api/login/cellphone/twice/used/");
            if !own {
                return self.inner.post(plan).await;
            }
            *self
                .inner
                .paths
                .lock()
                .unwrap()
                .entry(path.clone())
                .or_default() += 1;
            let mode = self.mode.lock().unwrap().clone();
            let read = path.ends_with("/user/get");
            let confirm = path.ends_with("/used/confirm");
            let register = path == "/api/w/register/cellphone";
            let identity = path == "/api/w/nuser/account/get";
            if read && mode == "hold-read"
                || confirm && mode == "hold-confirm"
                || register && mode == "hold-register"
                || identity && mode == "hold-identity"
            {
                let (tx, rx) = oneshot::channel();
                self.held.lock().unwrap().push(tx);
                self.pending.store(true, Ordering::SeqCst);
                let mut held = Held {
                    service: self,
                    completed: false,
                };
                let reply = rx
                    .await
                    .map_err(|_| Error::Transport("synthetic held response dropped".into()))?;
                held.completed = true;
                return Ok(reply);
            }
            if read {
                if mode == "read-lost" {
                    return Err(Error::Transport("synthetic read loss".into()));
                }
                if mode == "read-failed" {
                    return Ok(super::auth::reply(json!({"code":503}), None));
                }
                return Ok(super::auth::reply(
                    json!({"code":200,"data":{"nickName":"关联的合成账号","createTime":"2021-03-04","avatarUrl":"","phoneNumber":"DO_NOT_EXPORT"}}),
                    None,
                ));
            }
            if confirm {
                let mut r = super::auth::reply(
                    if mode == "confirm-false" {
                        json!({"code":200,"data":{"success":false}})
                    } else {
                        json!({"code":200})
                    },
                    if mode == "confirm-false" {
                        None
                    } else {
                        Some(2)
                    },
                );
                if mode == "confirm-lost" {
                    r.body_status = BodyStatus::Interrupted;
                }
                return Ok(r);
            }
            if register {
                assert_eq!(data["phone"], "13800138000");
                assert_eq!(data["captcha"], "123456");
                assert_eq!(data["countrycode"], "86");
                assert_eq!(data["force"], "true");
                assert_eq!(data["password"], "");
                assert!(data.get("fromPage").is_none() && data.get("ursToken").is_none());
                assert!(!plan.headers.contains_key("x-login-chain-id"));
                let mut r = super::auth::reply(
                    if mode == "register-false" || mode == "register-sms-invalid" {
                        json!({"code":if mode=="register-sms-invalid"{503}else{4012}})
                    } else {
                        json!({"code":200})
                    },
                    if mode == "register-false" || mode == "register-sms-invalid" {
                        None
                    } else {
                        Some(3)
                    },
                );
                if mode == "register-lost" {
                    r.body_status = BodyStatus::Interrupted;
                }
                return Ok(r);
            }
            if path == "/api/nickname/hint/get" {
                assert!(data.get("fromPage").is_none());
                assert_eq!(data["isAlter"], "false");
                if mode == "hint-lost" {
                    return Err(Error::Transport("synthetic hint loss".into()));
                }
                return Ok(super::auth::reply(
                    json!({"code":200,"hintNickname":"合成新昵称"}),
                    None,
                ));
            }
            if path == "/api/nickname/duplicated" {
                return Ok(super::auth::reply(
                    json!({"code":200,"duplicated":mode=="taken"}),
                    None,
                ));
            }
            if identity {
                let uid = plan
                    .headers
                    .get("Cookie")
                    .and_then(|c| {
                        c.split(';')
                            .find_map(|p| p.trim().strip_prefix("MUSIC_U=fixture"))
                    })
                    .and_then(|v| v.parse::<u64>().ok());
                return Ok(super::auth::reply(
                    match uid {
                        Some(uid @ (2 | 3)) => {
                            json!({"code":200,"profile":{"userId":uid,"nickname":if uid==2{"关联账号"}else{"注册账号"}}})
                        }
                        _ => json!({"code":301}),
                    },
                    None,
                ));
            }
            assert_eq!(data["phone"], "13800138000");
            Ok(if mode == "fresh-login" {
                super::auth::reply(json!({"code":200}), Some(3))
            } else {
                super::auth::reply(json!({"code":8860,"canSkipSetNickname":true}), None)
            })
        })
    }
}
