//! Synthetic original-device service; only this example accepts mode controls.
use cumulus_core::{
    Error,
    protocol::{
        api::ApiFuture,
        http::{BodyStatus, HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Mutex};
use tokio::sync::oneshot;
#[derive(Default)]
pub struct Service {
    pub base: super::auth::Service,
    mode: Mutex<String>,
    held: Mutex<Option<oneshot::Sender<HttpReply>>>,
    held_start: Mutex<Option<oneshot::Sender<()>>>,
    completed_start: Mutex<u64>,
    tokens: Mutex<Vec<String>>,
    completed: Mutex<u64>,
}
impl Service {
    pub fn command(&self, mode: &str) {
        if mode == "release_start" {
            self.held_start
                .lock()
                .unwrap()
                .take()
                .expect("one pending device start")
                .send(())
                .unwrap();
            *self.mode.lock().unwrap() = "normal".into();
        } else if mode == "release_poll" {
            let sender = self
                .held
                .lock()
                .unwrap()
                .take()
                .expect("one pending device poll");
            assert!(
                sender
                    .send(super::auth::reply(json!({"code":200}), Some(2)))
                    .is_ok()
            );
            *self.mode.lock().unwrap() = "normal".into();
        } else {
            *self.mode.lock().unwrap() = mode.into();
        }
    }
    pub fn snapshot(&self) -> Value {
        json!({"paths":*self.base.paths.lock().unwrap(),"pending":self.held.lock().unwrap().is_some(),"pendingStart":self.held_start.lock().unwrap().is_some(),"completedStarts":*self.completed_start.lock().unwrap(),"deviceStarts":self.tokens.lock().unwrap().len(),"completed":*self.completed.lock().unwrap()})
    }
}
impl HttpBackend for Service {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let form: BTreeMap<_, _> = url::form_urlencoded::parse(&plan.body)
                .into_owned()
                .collect();
            let (path, data) = eapi::unwrap(
                form.get("params")
                    .ok_or(Error::Response("device fixture requires EAPI"))?,
            )?;
            let own = path.starts_with("/api/login/origin-device/")
                || path == "/api/user/2fa/list"
                || (matches!(path.as_str(), "/api/login" | "/api/w/login")
                    && data["username"] == "device@example.test");
            if !own {
                return self.base.post(plan).await;
            }
            *self
                .base
                .paths
                .lock()
                .unwrap()
                .entry(path.clone())
                .or_default() += 1;
            let jsk = data["fromPage"] == "JSK";
            if jsk && plan.headers.get("x-login-chain-id").map(String::as_str) != Some("") {
                return Err(Error::Response("device fixture JSK context"));
            }
            if matches!(path.as_str(), "/api/login" | "/api/w/login") {
                if data["password"] != eapi::md5_hex(b"fixture-password") {
                    return Err(Error::Response("device fixture password"));
                }
                let mut r = super::auth::reply(
                    json!({"code":8830,"MUSIC_TU":"fixture-device-ticket"}),
                    None,
                );
                r.set_cookies =
                    vec!["MUSIC_TU=fixture-device-ticket; Path=/; Secure; HttpOnly".into()];
                return Ok(r);
            }
            if path == "/api/user/2fa/list" {
                return Ok(super::auth::reply(
                    json!({"code":200,"data":{"types":["DEVICE_CONFIRM","DEVICE_SCAN","SECURITY_EMAIL","BINDING_CELLPHONE","WEICHAT_AUTH"],"nickName":"设备验证账号","phoneMask":"138****8000"}}),
                    None,
                ));
            }
            if jsk
                && plan.headers.get("x-music-tu").map(String::as_str)
                    != Some("fixture-device-ticket")
            {
                return Err(Error::Response("device fixture verification ticket"));
            }
            let mode = self.mode.lock().unwrap().clone();
            if path.ends_with("/start") {
                if data["deviceName"] != "Synthetic PC"
                    || (jsk && data["scene"] != "undefined")
                    || (!jsk && data.get("scene").is_some())
                {
                    return Err(Error::Response("device fixture start fields"));
                }
                if mode == "start_lost" {
                    return Err(Error::Transport("synthetic lost start receipt".into()));
                }
                let token = {
                    let mut tokens = self.tokens.lock().unwrap();
                    let token = format!("fixture-device-token-{}", tokens.len() + 1);
                    tokens.push(token.clone());
                    token
                };
                if mode == "start_hold" {
                    let (sender, receiver) = oneshot::channel();
                    *self.held_start.lock().unwrap() = Some(sender);
                    receiver
                        .await
                        .map_err(|_| Error::Response("device start hold cancelled"))?;
                    *self.completed_start.lock().unwrap() += 1;
                }
                return Ok(super::auth::reply(
                    if mode == "blocked" {
                        json!({"code":815})
                    } else {
                        json!({"code":200,"data":{"pollingToken":token,"deviceName":"Synthetic PC","safeDeviceCount":2}})
                    },
                    None,
                ));
            }
            if !data["pollingToken"]
                .as_str()
                .is_some_and(|token| self.tokens.lock().unwrap().iter().any(|t| t == token))
            {
                return Err(Error::Response("device fixture polling token"));
            }
            if path.ends_with("/cancel") {
                if mode == "cancel_lost" {
                    return Err(Error::Transport("synthetic lost cancel receipt".into()));
                }
                return Ok(super::auth::reply(json!({"code":200}), None));
            }
            if data.get("captcha").is_some() {
                if data["captcha"] != "123456" {
                    return Ok(super::auth::reply(json!({"code":400}), None));
                }
                if mode == "business_false" {
                    return Ok(super::auth::reply(
                        json!({"code":200,"data":{"success":false}}),
                        None,
                    ));
                }
                let mut response = super::auth::reply(json!({"code":200}), Some(2));
                if mode == "verify_lost" {
                    response.body_status = BodyStatus::Interrupted;
                }
                return Ok(response);
            }
            if mode == "poll_hold" {
                let (sender, receiver) = oneshot::channel();
                *self.held.lock().unwrap() = Some(sender);
                let r = receiver
                    .await
                    .map_err(|_| Error::Response("device fixture hold cancelled"))?;
                *self.completed.lock().unwrap() += 1;
                return Ok(r);
            }
            if mode == "poll_error" {
                return Err(Error::Transport("synthetic unreadable device state".into()));
            }
            if mode == "poll_slow" {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }
            let code = match mode.as_str() {
                "scanned" => 802,
                "expired" => 810,
                "rejected" => 811,
                "blocked_poll" => 815,
                "confirm_status" => 200,
                _ => 801,
            };
            Ok(super::auth::reply(json!({"code":code}), None))
        })
    }
}
