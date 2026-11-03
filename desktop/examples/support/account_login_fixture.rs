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
    browser_documents: Mutex<Vec<cumulus_core::account::login::presentation::LoginDocument>>,
}
impl Service {
    pub fn open_document(
        &self,
        document: cumulus_core::account::login::presentation::LoginDocument,
    ) -> cumulus_core::Result<()> {
        self.browser_documents.lock().unwrap().push(document);
        if self.mode.lock().unwrap().as_str() == "browser_failed" {
            Err(Error::State("synthetic browser launch failed"))
        } else {
            Ok(())
        }
    }
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
        json!({"paths":*self.paths.lock().unwrap(),"pending":self.held.lock().unwrap().is_some(),"completed":*self.completed.lock().unwrap(),"browserDocuments":*self.browser_documents.lock().unwrap()})
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
            if path.starts_with("/api/w/v1/user/detail/")
                || path.starts_with("/api/w/v1/user/bindings/")
                || path == "/api/middle/user/device/list"
            {
                let uid = plan
                    .headers
                    .get("Cookie")
                    .and_then(|cookies| {
                        cookies
                            .split(';')
                            .find_map(|cookie| cookie.trim().strip_prefix("MUSIC_U=fixture"))
                    })
                    .and_then(|id| id.parse::<u64>().ok())
                    .filter(|id| matches!(id, 2 | 5))
                    .ok_or(Error::Response("fixture account security owner"))?;
                if path == "/api/middle/user/device/list" {
                    if data["excStatus"] != "9" && data["excStatus"] != 9 {
                        return Err(Error::Response("fixture device scope"));
                    }
                    if mode == "devices_failed" {
                        return Ok(auth::reply(json!({"code":500}), None));
                    }
                    if mode == "devices_hold" {
                        let (sender, receiver) = oneshot::channel();
                        *self.held.lock().unwrap() = Some(sender);
                        receiver
                            .await
                            .map_err(|_| Error::Response("fixture release"))?;
                        *self.completed.lock().unwrap() += 1;
                    }
                    let rows = if mode == "devices_empty" || uid == 5 {
                        json!([])
                    } else {
                        json!([
                            {"deviceKey":"fixture-private-key-a","deviceName":"旧设备","status":0,"region":"上海"},
                            {"deviceKey":"fixture-private-key-b","deviceName":"测试桌面","status":"1","ifLoginDevice":true,"loginPlace":"杭州","lastActiveTime":1700000000000_u64,"os":"Windows"}
                        ])
                    };
                    return Ok(auth::reply(
                        json!({"code":200,"data":{"userDeviceList":rows,"userDeviceLimitInfoDTO":{"currentNum":1,"deviceLimit":3}}}),
                        None,
                    ));
                }
                let uid_text = uid.to_string();
                if !path.ends_with(&format!("/{uid}"))
                    || (data["userId"] != uid_text && data["userId"] != uid)
                {
                    return Err(Error::Response("fixture profile owner mismatch"));
                }
                if path.contains("/detail/") {
                    if data["all"] != true && data["all"] != "true" {
                        return Err(Error::Response("fixture profile detail options"));
                    }
                    return Ok(auth::reply(
                        json!({"code":200,"level":7,"profile":{
                            "userId":uid,"nickname":auth::nickname(uid),
                            "signature":if uid == 2 {"这是合成的账号资料"} else {"另一个账号的资料"},
                            "birthday":946684800000_i64,"gender":1,"province":330000,"city":330100
                        }}),
                        None,
                    ));
                }
                return Ok(auth::reply(
                    json!({"code":200,"bindings":[{"type":0,"tokenJsonStr":"{\"email\":\"private-fixture@example.test\",\"password\":\"fixture-do-not-export\"}"}]}),
                    None,
                ));
            }
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
