//! Explicit fault controls for the standalone account/login example only.
use super::auth;
use cumulus_core::{
    Error,
    protocol::{
        api::ApiFuture,
        http::{HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Mutex};
use tokio::sync::oneshot;

#[derive(Default)]
pub struct Service {
    base: auth::Service,
    mode: Mutex<String>,
    paths: Mutex<BTreeMap<String, u64>>,
    held: Mutex<Option<oneshot::Sender<()>>>,
    completed: Mutex<u64>,
}
impl Service {
    pub fn command(&self, mode: &str) {
        if mode == "release" {
            if let Some(sender) = self.held.lock().unwrap().take() {
                let _ = sender.send(());
            }
        } else {
            *self.mode.lock().unwrap() = mode.into();
        }
    }
    pub fn snapshot(&self) -> Value {
        json!({"paths":*self.paths.lock().unwrap(),"pending":self.held.lock().unwrap().is_some(),"completed":*self.completed.lock().unwrap()})
    }
}
impl HttpBackend for Service {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let form =
                std::str::from_utf8(&plan.body).map_err(|_| Error::Response("fixture form"))?;
            let params = form
                .split('&')
                .find_map(|part| part.strip_prefix("params="))
                .ok_or(Error::Response("fixture EAPI"))?;
            let (path, data) = eapi::unwrap(params)?;
            *self.paths.lock().unwrap().entry(path.clone()).or_default() += 1;
            let mode = self.mode.lock().unwrap().clone();
            if mode == "bootstrap_slow"
                && matches!(path.as_str(), "/api/user/playlist" | "/api/song/like/get")
            {
                tokio::time::sleep(std::time::Duration::from_millis(750)).await;
            }
            if matches!(
                path.as_str(),
                "/api/login"
                    | "/api/login/cellphone"
                    | "/api/login/qrcode/unikey"
                    | "/api/login/qrcode/client/login"
            ) {
                if data["fromPage"] != "JSK"
                    || plan.headers.get("x-login-chain-id").map(String::as_str) != Some("")
                {
                    return Err(Error::Response("fixture JSK context missing"));
                }
                if path.contains("/qrcode/") && data["type"] != "5" {
                    return Err(Error::Response("fixture JSK QR type"));
                }
            }

            if path == "/api/login/token/refresh/v1" {
                if data["ydDeviceToken"] != "" || data["_scver"] != "1" {
                    return Err(Error::Response("fixture renewal fields"));
                }
                let uid = plan
                    .headers
                    .get("Cookie")
                    .and_then(|cookies| {
                        cookies
                            .split(';')
                            .find_map(|cookie| cookie.trim().strip_prefix("MUSIC_U=fixture"))
                    })
                    .and_then(|uid| uid.parse::<u64>().ok());
                if !matches!(uid, Some(2 | 5)) {
                    return Err(Error::Response("fixture renewal identity"));
                }
                return Ok(auth::reply(json!({"code":200}), uid));
            }
            if matches!(
                path.as_str(),
                "/api/w/login/cellphone" | "/api/login/cellphone"
            ) && data["password"].is_string()
            {
                if data["phone"] != "13800138000"
                    || data["password"] != eapi::md5_hex(b"fixture-password")
                {
                    return Err(Error::Response("fixture phone/password"));
                }
                return Ok(auth::reply(json!({"code":200}), Some(5)));
            }
            if path == "/api/login/qrcode/unikey" && mode == "qr_error" {
                return Err(Error::Transport("合成二维码创建失败".into()));
            }
            if path == "/api/login/qrcode/client/login" {
                return Ok(match mode.as_str() {
                    "qr_scanned" => auth::reply(json!({"code":802}), None),
                    "qr_expired" => auth::reply(json!({"code":800}), None),
                    "qr_authorized" => auth::reply(json!({"code":803}), Some(2)),
                    _ => auth::reply(json!({"code":801}), None),
                });
            }
            if path == "/api/logout" && mode == "logout_error" {
                return Err(Error::Transport("合成服务器退出失败".into()));
            }
            if matches!(path.as_str(), "/api/w/login" | "/api/login") && mode == "login_hold" {
                let (sender, receiver) = oneshot::channel();
                *self.held.lock().unwrap() = Some(sender);
                receiver
                    .await
                    .map_err(|_| Error::Response("fixture release"))?;
                *self.completed.lock().unwrap() += 1;
            }
            self.base.post(plan).await
        })
    }
}
