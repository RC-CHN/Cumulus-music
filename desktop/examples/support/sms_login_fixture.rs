//! Synthetic native HTTP only. Modes are private to sms_login_ui_probe.
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
    held: Mutex<Option<oneshot::Sender<()>>>,
    dropped: Mutex<u64>,
}
impl Service {
    pub fn command(&self, mode: &str) {
        if mode == "release"
            && self
                .held
                .lock()
                .unwrap()
                .take()
                .expect("one held SMS request")
                .send(())
                .is_err()
        {
            *self.dropped.lock().unwrap() += 1;
        }
        *self.mode.lock().unwrap() = mode.into();
    }
    pub fn snapshot(&self) -> Value {
        json!({"paths":*self.base.paths.lock().unwrap(),"pending":self.held.lock().unwrap().is_some(),"dropped":*self.dropped.lock().unwrap()})
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
                    .ok_or(Error::Response("SMS fixture EAPI"))?,
            )?;
            if path != "/api/user/2fa/list"
                && !path.starts_with("/api/middle/account/")
                && !(matches!(path.as_str(), "/api/login" | "/api/w/login")
                    && data["username"] == "sms@example.test")
            {
                return self.base.post(plan).await;
            }
            *self
                .base
                .paths
                .lock()
                .unwrap()
                .entry(path.clone())
                .or_default() += 1;
            let mode = self.mode.lock().unwrap().clone();
            let jsk = data["fromPage"] == "JSK";
            if jsk && plan.headers.get("x-login-chain-id").map(String::as_str) != Some("") {
                return Err(Error::Response("SMS fixture chain"));
            }
            if matches!(path.as_str(), "/api/login" | "/api/w/login") {
                if data["password"] != eapi::md5_hex(b"fixture-password") {
                    return Err(Error::Response("SMS fixture password"));
                }
                let mut response =
                    super::auth::reply(json!({"code":8830,"MUSIC_TU":"fixture-sms-ticket"}), None);
                response.set_cookies =
                    vec!["MUSIC_TU=fixture-sms-ticket; Path=/; Secure; HttpOnly".into()];
                return Ok(response);
            }
            if path == "/api/user/2fa/list" {
                let country = match mode.as_str() {
                    "non86" => json!("1"),
                    "no_country" => Value::Null,
                    _ => json!("86"),
                };
                return Ok(super::auth::reply(
                    json!({"code":200,"data":{"types":["UP_SMS","SECURITY_EMAIL"],"nickName":"短信验证账号","phoneMask":"139****1111","authDetail":{"UP_SMS":"138****8000"},"ctCode":country}}),
                    None,
                ));
            }
            if jsk
                && plan.headers.get("x-music-tu").map(String::as_str) != Some("fixture-sms-ticket")
            {
                return Err(Error::Response("SMS fixture ticket"));
            }
            let oracle: Value = serde_json::from_str(include_str!(
                "../../../core/tests/fixtures/sms-login-official.json"
            ))
            .unwrap();
            let name = match path.as_str() {
                "/api/middle/account/uplinkcaptcha/get" => "getUpSmsInfo",
                "/api/middle/account/uplinkcaptcha/verify" => "verifyUpSms",
                "/api/middle/account/captcha/sent" => "sendDownSms",
                "/api/middle/account/captcha/verify" => "verifyDownSms",
                _ => return Err(Error::Response("SMS fixture path")),
            };
            let mut actual = data.clone();
            actual.as_object_mut().unwrap().remove("header");
            actual.as_object_mut().unwrap().remove("e_r");
            if path.ends_with("/captcha/verify") && actual["captcha"] == "000000" {
                actual["captcha"] = json!("123456");
            }
            let expected = if jsk {
                &oracle["jsk"][name]["payload"]
            } else {
                &oracle["legacy"][&path]
            };
            if &actual != expected {
                return Err(Error::Response("SMS fixture official fields"));
            }
            if mode == "get_hold" || (mode == "check_hold" && path.ends_with("/verify")) {
                let (sender, receiver) = oneshot::channel();
                *self.held.lock().unwrap() = Some(sender);
                receiver
                    .await
                    .map_err(|_| Error::State("SMS fixture held channel"))?;
            }
            if path.ends_with("/get") {
                if mode == "info_lost" {
                    return Err(Error::Transport("synthetic SMS info failure".into()));
                }
                return Ok(super::auth::reply(
                    json!({"code":200,"data":{"uploadText":"SEND 1234","uploadNumber":"1069000001","uploadNumberBackup":"1069000002"}}),
                    None,
                ));
            }
            if path.ends_with("/sent") {
                return Ok(super::auth::reply(
                    json!({"code":if mode == "send_failed" {509} else {200}}),
                    None,
                ));
            }
            if mode == "timeout" {
                return Ok(super::auth::reply(json!({"code":200,"data":false}), None));
            }
            if data["captcha"] == "000000" {
                return Ok(super::auth::reply(json!({"code":400}), None));
            }
            if mode == "business_false" {
                return Ok(super::auth::reply(
                    json!({"code":200,"data":{"success":false}}),
                    Some(2),
                ));
            }
            let mut response = super::auth::reply(json!({"code":200,"data":true}), Some(2));
            if mode == "down_lost" {
                response.body_status = BodyStatus::Interrupted;
            }
            Ok(response)
        })
    }
}
