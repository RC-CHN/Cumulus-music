//! Frozen synthetic account-question responses; no real service transport.
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
                "device",
                "album",
                "artist",
                "read-failed",
                "read-lost",
                "wrong",
                "business-false",
                "verify-lost",
                "hold-read",
                "hold-verify",
                "hold-identity",
                "release"
            ]
            .contains(&mode)
        );
        if mode == "read" {
            return;
        }
        if mode == "release" {
            for tx in self.held.lock().unwrap().drain(..) {
                let _ = tx.send(super::auth::reply(json!({"code":200}), Some(4)));
            }
        }
        *self.mode.lock().unwrap() = if mode == "release" {
            "device".into()
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
            let mode = self.mode.lock().unwrap().clone();
            let held_identity = mode == "hold-identity" && path == "/api/w/nuser/account/get";
            if !path.starts_with("/api/login/cellphone/twice/question/") && !held_identity {
                return self.inner.post(plan).await;
            }
            *self
                .inner
                .paths
                .lock()
                .unwrap()
                .entry(path.clone())
                .or_default() += 1;
            if !held_identity {
                if data["fromPage"] == "JSK" {
                    assert_eq!(data["scene"], "undefined");
                } else {
                    assert!(data.get("scene").is_none());
                }
            }
            let read = path.ends_with("/get");
            if held_identity || (read && mode == "hold-read") || (!read && mode == "hold-verify") {
                let (tx, rx) = oneshot::channel();
                self.held.lock().unwrap().push(tx);
                self.pending.store(true, Ordering::SeqCst);
                let mut held = Held {
                    service: self,
                    completed: false,
                };
                let response = rx
                    .await
                    .map_err(|_| Error::Transport("fixture dropped".into()))?;
                held.completed = true;
                return Ok(response);
            }
            if read {
                if mode == "read-lost" {
                    return Err(Error::Transport("synthetic read loss".into()));
                }
                if mode == "read-failed" {
                    return Ok(super::auth::reply(json!({"code":503}), None));
                }
                let kind = match mode.as_str() {
                    "album" => 2,
                    "artist" => 3,
                    _ => 1,
                };
                let options:Vec<_>=(1..=6).map(|i|json!({"title":format!("{} {i} · 用于检查长文本排布的合成内容",match kind{2=>"合成专辑",3=>"合成艺人",_=>"合成设备"}),"subtitle":"Cumulus · synthetic account question only"})).collect();
                return Ok(super::auth::reply(
                    json!({"code":200,"data":{"questionType":kind,"title":match kind{2=>"请选择你购买过的数字专辑",3=>"请选择你关注过的艺人",_=>"请选择你使用过的设备"},"options":options}}),
                    None,
                ));
            }
            Ok(match mode.as_str() {
                "wrong" => super::auth::reply(json!({"code":4012}), None),
                "business-false" => {
                    super::auth::reply(json!({"code":200,"data":{"success":false}}), Some(4))
                }
                "verify-lost" => {
                    let mut r = super::auth::reply(json!({"code":200}), Some(4));
                    r.body_status = BodyStatus::Interrupted;
                    r
                }
                _ => {
                    assert!(
                        data["ans"] == "none"
                            || data["ans"].as_str().is_some_and(|s| s.starts_with("合成"))
                    );
                    super::auth::reply(json!({"code":200}), Some(4))
                }
            })
        })
    }
}
