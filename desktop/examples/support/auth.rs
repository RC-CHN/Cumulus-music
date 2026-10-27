//! Synthetic service at the native HTTP boundary; no sockets or real accounts.
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

#[derive(Default)]
pub struct Service {
    pub paths: Mutex<BTreeMap<String, u64>>,
}
pub fn reply(value: Value, uid: Option<u64>) -> HttpReply {
    HttpReply {
        status: 200,
        body: serde_json::to_vec(&value).unwrap(),
        body_status: BodyStatus::Complete,
        set_cookies: uid
            .map(|uid| vec![format!("MUSIC_U=fixture{uid}; Path=/; Secure; HttpOnly")])
            .unwrap_or_default(),
        refresh_token: uid.map(|_| "fixture-refresh".into()),
        metadata: BTreeMap::new(),
    }
}
pub fn nickname(uid: u64) -> &'static str {
    match uid {
        2 => "邮箱验证账号",
        3 => "二次验证账号",
        4 => "安全问题账号",
        5 => "短信验证账号",
        _ => "未知账号",
    }
}
impl HttpBackend for Service {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let form =
                std::str::from_utf8(&plan.body).map_err(|_| Error::Response("fixture form"))?;
            let params = form
                .split('&')
                .find_map(|p| p.strip_prefix("params="))
                .ok_or(Error::Response("fixture eapi expected"))?;
            let (path, data) = eapi::unwrap(params)?;
            *self.paths.lock().unwrap().entry(path.clone()).or_default() += 1;
            let uid = plan
                .headers
                .get("Cookie")
                .and_then(|c| {
                    c.split(';')
                        .find_map(|v| v.trim().strip_prefix("MUSIC_U=fixture"))
                })
                .and_then(|s| s.parse::<u64>().ok());
            let (value, login) = match path.as_str() {
                "/api/rtrs/abt/front/expinfo/list" => (
                    json!({"code":200,"data":[
                        {"expName":"PH-PC-loginJsk","expGroupName":"t1"},
                        {"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}
                    ]}),
                    None,
                ),
                "/api/w/nuser/account/get" => (
                    match uid {
                        Some(uid) if (2..=5).contains(&uid) => {
                            json!({"code":200,"profile":{"userId":uid,"nickname":nickname(uid)}})
                        }
                        _ => json!({"code":301}),
                    },
                    None,
                ),
                "/api/login/qrcode/unikey" => {
                    (json!({"code":200,"unikey":"synthetic-ui-qr"}), None)
                }
                "/api/login/qrcode/client/login" => (json!({"code":801}), None),
                "/api/w/login" => {
                    if data["password"] != eapi::md5_hex(b"fixture-password") {
                        (json!({"code":502}), None)
                    } else {
                        match data["username"].as_str() {
                            Some("alice@example.test") => (json!({"code":200}), Some(2)),
                            Some("verify@example.test") => {
                                (json!({"code":8830,"MUSIC_TU":"fixture-two-factor"}), None)
                            }
                            Some("question@example.test") => (json!({"code":8861}), None),
                            Some("blocked@example.test") => {
                                (json!({"code":8821,"private":"DO_NOT_EXPORT"}), None)
                            }
                            _ => return Err(Error::Response("unexpected fixture account")),
                        }
                    }
                }
                "/api/user/2fa/list" => (
                    json!({"code":200,"data":{"types":["SECURITY_EMAIL","BINDING_CELLPHONE"],"nickName":"二次验证账号","phoneMask":"138****8000"}}),
                    None,
                ),
                "/api/user/2fa/begin"
                | "/api/user/2fa/securityEmail/send"
                | "/api/sms/captcha/sent" => (json!({"code":200}), None),
                "/api/user/2fa/securityEmail/verify" => {
                    if data["captcha"] != "123456" {
                        return Err(Error::Response("fixture verification code mismatch"));
                    }
                    (json!({"code":200}), Some(3))
                }
                "/api/login/cellphone/twice/question/get" => (
                    json!({"code":200,"data":{"title":"请选择你使用过的设备","questionType":1,"options":[{"title":"设备甲","subtitle":"最近使用"},{"title":"设备乙"}]}}),
                    None,
                ),
                "/api/login/cellphone/twice/question/verify" => {
                    if data["ans"] != "设备甲" {
                        return Err(Error::Response("fixture question mismatch"));
                    }
                    (json!({"code":200}), Some(4))
                }
                "/api/w/login/cellphone" => {
                    if data["captcha"] != "123456" || data["phone"] != "13800138000" {
                        return Err(Error::Response("fixture phone mismatch"));
                    }
                    (json!({"code":200}), Some(5))
                }
                "/api/logout" => {
                    if uid != Some(2) {
                        return Err(Error::Response(
                            "fixture logout affected unexpected account",
                        ));
                    }
                    (json!({"code":200}), None)
                }
                "/api/user/playlist" => (json!({"code":200,"playlist":[],"more":false}), None),
                "/api/song/like/get" => (json!({"code":200,"ids":[]}), None),
                "/api/v3/discovery/recommend/songs" => {
                    (json!({"code":200,"data":{"dailySongs":[]}}), None)
                }
                _ => return Err(Error::Response("unexpected native fixture request")),
            };
            Ok(reply(value, login))
        })
    }
}
