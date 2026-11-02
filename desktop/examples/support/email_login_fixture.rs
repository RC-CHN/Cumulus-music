//! Synthetic HTTP faults for the production email verification WebView.
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
                "wrong",
                "verify-limit",
                "send-limit",
                "send-failed",
                "send-lost",
                "verify-lost",
                "business-false",
                "hold-send",
                "hold-verify",
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
                .find_map(|v| v.strip_prefix("params="))
                .unwrap();
            let (path, payload) = eapi::unwrap(params)?;
            if ![
                "/api/user/2fa/securityEmail/send",
                "/api/user/2fa/securityEmail/verify",
            ]
            .contains(&path.as_str())
            {
                let mut response = self.inner.post(plan).await?;
                if serde_json::from_slice::<Value>(&response.body).is_ok_and(|v| v["code"] == 8830)
                {
                    response
                        .set_cookies
                        .push("MUSIC_TU=synthetic-email-ticket; Path=/; Secure; HttpOnly".into());
                }
                return Ok(response);
            }
            *self
                .inner
                .paths
                .lock()
                .unwrap()
                .entry(path.clone())
                .or_default() += 1;
            assert_eq!(payload["scene"], "0");
            let send = path.ends_with("/send");
            let mode = self.mode.lock().unwrap().clone();
            if (send && mode == "hold-send") || (!send && mode == "hold-verify") {
                let (tx, rx) = oneshot::channel();
                self.held.lock().unwrap().push(tx);
                self.pending.store(true, Ordering::SeqCst);
                let mut held = Held {
                    service: self,
                    completed: false,
                };
                let value = rx
                    .await
                    .map_err(|_| Error::Transport("fixture abandoned".into()))?;
                held.completed = true;
                return Ok(value);
            }
            if send {
                return match mode.as_str() {
                    "send-limit" => Ok(super::auth::reply(json!({"code":4011}), None)),
                    "send-failed" => Ok(super::auth::reply(json!({"code":503}), None)),
                    "send-lost" => Err(Error::Transport(
                        "synthetic email send response lost".into(),
                    )),
                    _ => Ok(super::auth::reply(
                        json!({"code":200,"data":{"mailAddress":"a-very-long-synthetic-mailbox-name@example.test"}}),
                        None,
                    )),
                };
            }
            Ok(match mode.as_str() {
                "wrong" => super::auth::reply(json!({"code":4012}), None),
                "verify-limit" => super::auth::reply(json!({"code":4013}), None),
                "business-false" => {
                    super::auth::reply(json!({"code":200,"data":{"success":false}}), Some(3))
                }
                "verify-lost" => {
                    let mut r = super::auth::reply(json!({"code":200}), Some(3));
                    r.body_status = BodyStatus::Interrupted;
                    r
                }
                _ => {
                    assert_eq!(payload["captcha"], "123456");
                    super::auth::reply(json!({"code":200}), Some(3))
                }
            })
        })
    }
}
