//! Explicit fault controls for the standalone account/login example only.
use super::auth;
use cumulus_core::{
    Error,
    cloud::nos::{NosBackend, NosMethod, NosNode, NosRequest},
    protocol::{
        api::ApiFuture,
        http::{BodyStatus, HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};
use tokio::sync::oneshot;

#[derive(Default)]
pub struct Service {
    base: auth::Service,
    mode: Mutex<String>,
    paths: Mutex<BTreeMap<String, u64>>,
    held: Mutex<Option<oneshot::Sender<()>>>,
    completed: Mutex<u64>,
    switch_identity_reads: Mutex<u64>,
    qr_candidate_identity_reads: Mutex<u64>,
    browser_documents: Mutex<Vec<cumulus_core::account::login::presentation::LoginDocument>>,
    profiles: Mutex<BTreeMap<u64, Value>>,
    removed_devices: Mutex<BTreeSet<u64>>,
    avatar_proof: bool,
    avatars: Mutex<Avatars>,
    qr_status: Option<String>,
}
#[derive(Default)]
struct Avatars {
    current: Option<String>,
    allocated: u64,
    chunks: u64,
    objects: BTreeMap<String, Vec<u8>>,
}
impl Service {
    pub fn new(avatar_proof: bool, qr_status: Option<String>) -> Self {
        Self {
            avatar_proof,
            qr_status,
            ..Self::default()
        }
    }
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
        } else if mode != "inspect" {
            *self.mode.lock().unwrap() = mode.into();
        }
    }
    pub fn pending(&self) -> bool {
        self.held.lock().unwrap().is_some()
    }
    pub fn snapshot(&self) -> Value {
        let avatars = self.avatars.lock().unwrap();
        json!({"paths":*self.paths.lock().unwrap(),"pending":self.pending(),"completed":*self.completed.lock().unwrap(),"switchIdentityReads":*self.switch_identity_reads.lock().unwrap(),"qrCandidateIdentityReads":*self.qr_candidate_identity_reads.lock().unwrap(),"browserDocuments":*self.browser_documents.lock().unwrap(),"avatarChunks":avatars.chunks,"avatarObjects":avatars.objects.len(),"deviceRemoved":self.removed_devices.lock().unwrap().contains(&2)})
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
            let count = {
                let mut paths = self.paths.lock().unwrap();
                let count = paths.entry(path.clone()).or_default();
                *count += 1;
                *count
            };
            let mode = self.mode.lock().unwrap().clone();
            if self.qr_status.as_deref() == Some("legacy")
                && path == "/api/rtrs/abt/front/expinfo/list"
                && data["expNames"]
                    .as_str()
                    .is_some_and(|names| names.contains("PH-PC-loginJsk"))
            {
                return Ok(auth::reply(
                    json!({"code":200,"data":[
                        {"expName":"PH-PC-loginJsk","expGroupName":"c"},
                        {"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}
                    ]}),
                    None,
                ));
            }
            if let Some(protocol) = &self.qr_status
                && matches!(
                    path.as_str(),
                    "/api/login/qrcode/unikey" | "/api/login/qrcode/client/login"
                )
            {
                let legacy = protocol == "legacy";
                if data["type"] != if legacy { "3" } else { "5" }
                    || data["lastUniKey"].is_string()
                    || !legacy && data["fromPage"] != "JSK"
                {
                    return Err(Error::Response("fixture ordinary QR request"));
                }
                if path.ends_with("/unikey") {
                    return Ok(auth::reply(
                        json!({"code":200,"unikey":"fixture-qr-status-ticket"}),
                        None,
                    ));
                }
                if data["key"] != "fixture-qr-status-ticket" || data["secureCaptcha"] != "" {
                    return Err(Error::Response("fixture ordinary QR poll fields"));
                }
                let code = match count {
                    1 => 802,
                    2 => 801,
                    3 => 800,
                    _ => return Err(Error::Response("fixture unexpected ordinary QR poll")),
                };
                let (sender, receiver) = oneshot::channel();
                if self.held.lock().unwrap().replace(sender).is_some() {
                    return Err(Error::Response("fixture overlapping ordinary QR polls"));
                }
                receiver
                    .await
                    .map_err(|_| Error::Response("fixture QR release"))?;
                return Ok(auth::reply(json!({"code":code}), None));
            }
            if path == "/api/rtrs/abt/front/expinfo/list"
                && data["expNames"] == "PH-PC-quickLoginSwitchAccountNew"
            {
                let group = if mode == "quick_offer_control" {
                    "c"
                } else {
                    "t1"
                };
                return Ok(auth::reply(
                    json!({"code":200,"data":[{
                        "expName":"PH-PC-quickLoginSwitchAccountNew","expGroupName":group
                    }]}),
                    None,
                ));
            }
            if path == "/api/authentication/identity/user/all"
                || path == "/api/music-vip-membership/client/vip/info" && mode == "quick_offer"
            {
                let uid = plan
                    .headers
                    .get("Cookie")
                    .and_then(|cookies| {
                        cookies
                            .split(';')
                            .find_map(|part| part.trim().strip_prefix("MUSIC_U=fixture"))
                    })
                    .and_then(|value| value.parse::<u64>().ok())
                    .filter(|id| matches!(id, 2 | 5))
                    .ok_or(Error::Response("fixture quick offer owner"))?;
                let uid_text = uid.to_string();
                if data["userId"].as_str() != Some(uid_text.as_str()) {
                    return Err(Error::Response("fixture quick offer owner mismatch"));
                }
                let content = if path.contains("/identity/") {
                    json!([])
                } else {
                    json!({"userId":uid})
                };
                return Ok(auth::reply(json!({"code":200,"data":content}), None));
            }
            if path == "/api/lbs/region/code" {
                if data
                    .as_object()
                    .is_none_or(|fields| fields.keys().any(|key| key != "header"))
                {
                    return Err(Error::Response("fixture region request"));
                }
                return Ok(auth::reply(
                    if mode == "profile_ack_lost" {
                        json!({"code":200,"data":{"version":"broken","provinces":[]}})
                    } else {
                        json!({"code":200,"data":{"version":"fixture-2026","provinces":[{
                            "id":330000,"name":"浙江省（合成）","cities":{"330100":"杭州市（合成）"}
                        }]}})
                    },
                    None,
                ));
            }
            if mode.starts_with("active_switch_") {
                let cookie = plan.headers.get("Cookie").map(String::as_str).unwrap_or("");
                let has = |token: &str| cookie.split(';').any(|pair| pair.trim() == token);
                if matches!(
                    path.as_str(),
                    "/api/login/getquickloginlist" | "/api/login/switchuser"
                ) {
                    let rows: Value = serde_json::from_str(
                        data["userInfoList"]
                            .as_str()
                            .ok_or(Error::Response("fixture switch history"))?,
                    )?;
                    if data["currentEnv"] != "{}"
                        || data["fromPage"] != "JSK"
                        || plan.headers.get("x-login-chain-id").map(String::as_str) != Some("")
                        || rows.as_array().map(Vec::len) != Some(2)
                        || rows[0]["userId"] != "5"
                        || rows[0]["userProfile"]["userId"] != "5"
                        || rows[0]["musicU"] != "fixture5"
                        || rows[1]["userId"] != "2"
                        || rows[1]["userProfile"]["userId"] != "2"
                        || rows[1]["musicU"] != "fixture2"
                        || !has("MUSIC_U=fixture5")
                        || has("MUSIC_U=fixture2")
                    {
                        return Err(Error::Response("fixture active switch request contract"));
                    }
                    if path == "/api/login/getquickloginlist" {
                        if data.get("targetUserId").is_some() || data.get("currentUserId").is_some()
                        {
                            return Err(Error::Response(
                                "fixture eligibility contains exchange fields",
                            ));
                        }
                        return Ok(auth::reply(json!({"code":200,"deleteUserIds":[]}), None));
                    }
                    if data["targetUserId"] != "2" || data["currentUserId"] != "5" {
                        return Err(Error::Response("fixture switch target or source mismatch"));
                    }
                    if mode == "active_switch_verify" {
                        return Ok(auth::reply(json!({"code":8830}), None));
                    }
                    if mode == "active_switch_hold" {
                        let (sender, receiver) = oneshot::channel();
                        *self.held.lock().unwrap() = Some(sender);
                        receiver
                            .await
                            .map_err(|_| Error::Response("fixture switch release"))?;
                        *self.completed.lock().unwrap() += 1;
                        return Ok(auth::reply(json!({"code":200}), Some(2)));
                    }
                    if mode != "active_switch_interrupt" {
                        return Err(Error::Response("unexpected fixture active switch"));
                    }
                    let mut reply = auth::reply(json!({"code":200}), None);
                    reply.body_status = BodyStatus::Interrupted;
                    reply.body.clear();
                    reply.set_cookies =
                        vec!["MUSIC_U=fixtureSwitch2; Path=/; Secure; HttpOnly".into()];
                    return Ok(reply);
                }
                if path == "/api/w/nuser/account/get" && has("MUSIC_U=fixtureSwitch2") {
                    if has("MUSIC_U=fixture5") || has("MUSIC_U=fixture2") {
                        return Err(Error::Response(
                            "fixture target identity mixed source credentials",
                        ));
                    }
                    *self.switch_identity_reads.lock().unwrap() += 1;
                    return Ok(auth::reply(
                        json!({"code":200,"profile":{"userId":2,"nickname":auth::nickname(2)}}),
                        None,
                    ));
                }
            }
            if mode.starts_with("quick_") {
                let cookie = plan.headers.get("Cookie").map(String::as_str).unwrap_or("");
                if matches!(
                    path.as_str(),
                    "/api/login/getquickloginlist" | "/api/login/quicklogin"
                ) {
                    let rows: Value = serde_json::from_str(
                        data["userInfoList"]
                            .as_str()
                            .ok_or(Error::Response("fixture quick history"))?,
                    )?;
                    if data["currentEnv"] != "{}"
                        || data["fromPage"] != "JSK"
                        || plan.headers.get("x-login-chain-id").map(String::as_str) != Some("")
                        || rows.as_array().map(Vec::len) != Some(1)
                        || rows[0]["userId"] != "2"
                        || rows[0]["userProfile"]["userId"] != "2"
                        || rows[0]["musicU"] != "fixture2"
                    {
                        return Err(Error::Response("fixture quick request contract"));
                    }
                    if path == "/api/login/getquickloginlist" {
                        if data.get("targetUserId").is_some() || data.get("currentUserId").is_some()
                        {
                            return Err(Error::Response(
                                "fixture eligibility contains exchange fields",
                            ));
                        }
                        let invalid = if mode == "quick_offer_invalidate_current" {
                            json!([2])
                        } else {
                            json!([])
                        };
                        return Ok(auth::reply(
                            json!({"code":200,"deleteUserIds":invalid}),
                            None,
                        ));
                    }
                    if data["targetUserId"] != "2"
                        || data["currentUserId"] != ""
                        || cookie
                            .split(';')
                            .any(|pair| pair.trim().starts_with("MUSIC_U="))
                    {
                        return Err(Error::Response("fixture exchange was not guest scoped"));
                    }
                    if mode == "quick_verify" {
                        return Ok(auth::reply(json!({"code":8830}), None));
                    }
                    if mode != "quick_interrupt" {
                        return Err(Error::Response("unexpected fixture quick exchange"));
                    }
                    let mut reply = auth::reply(json!({"code":200}), None);
                    reply.body_status = BodyStatus::Interrupted;
                    reply.body.clear();
                    reply.set_cookies =
                        vec!["MUSIC_U=fixtureQuick2; Path=/; Secure; HttpOnly".into()];
                    return Ok(reply);
                }
                if path == "/api/w/nuser/account/get"
                    && cookie
                        .split(';')
                        .any(|pair| pair.trim() == "MUSIC_U=fixtureQuick2")
                {
                    return Ok(auth::reply(
                        json!({"code":200,"profile":{"userId":2,"nickname":auth::nickname(2)}}),
                        None,
                    ));
                }
            }
            if self.avatar_proof && path == "/api/nos/token/alloc" {
                if data["filename"] != "avatar.jpg"
                    || data["ext"] != "jpg"
                    || data["type"] != "image"
                    || data["bucket"] != "yyimgs"
                {
                    return Err(Error::Response("avatar fixture allocation"));
                }
                let mut avatars = self.avatars.lock().unwrap();
                avatars.allocated += 1;
                return Ok(auth::reply(
                    json!({"code":200,"result":{
                        "docId":10_000+avatars.allocated,
                        "objectKey":format!("avatar-proof-{}.jpg", avatars.allocated),
                        "token":"avatar-proof-scope","bucket":"yyimgs"
                    }}),
                    None,
                ));
            }
            if self.avatar_proof && path == "/api/user/avatar/upload/v1" {
                let id = data["imgid"]
                    .as_u64()
                    .or_else(|| data["imgid"].as_str()?.parse().ok())
                    .filter(|id| (10_001..=10_002).contains(id))
                    .ok_or(Error::Response("avatar fixture document"))?;
                let filename = format!("avatar-proof-{}.jpg", id - 10_000);
                let url = format!("https://p5.music.126.net/{filename}");
                let mut avatars = self.avatars.lock().unwrap();
                if data["url"] != url
                    || !avatars
                        .objects
                        .get(&format!("/yyimgs/{filename}"))
                        .is_some_and(|bytes| bytes.starts_with(&[0xff, 0xd8]))
                {
                    return Err(Error::Response(
                        "avatar fixture binding before uploaded JPEG",
                    ));
                }
                avatars.current = Some(url.clone());
                if mode == "avatar_ack_lost" {
                    return Err(Error::Transport(
                        "synthetic avatar acknowledgement lost".into(),
                    ));
                }
                return Ok(auth::reply(json!({"code":200,"url":url}), None));
            }
            if path.starts_with("/api/w/v1/user/detail/")
                || path.starts_with("/api/w/v1/user/bindings/")
                || path == "/api/middle/user/device/list"
                || path == "/api/middle/user/security/device/kickoff"
                || path == "/api/sms/captcha/safe/sent"
                || path == "/api/user/profile/update"
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
                if matches!(
                    path.as_str(),
                    "/api/middle/user/security/device/kickoff" | "/api/sms/captcha/safe/sent"
                ) {
                    if uid != 2
                        || !matches!(mode.as_str(), "device_removal_sms" | "device_removal_drop")
                    {
                        return Err(Error::Response(
                            "fixture device mutation is not authorized by this scenario",
                        ));
                    }
                    let fields = data
                        .as_object()
                        .ok_or(Error::Response("fixture device mutation fields"))?;
                    if path == "/api/sms/captcha/safe/sent" {
                        if mode != "device_removal_sms"
                            || data["ctCode"] != "1242"
                            || fields
                                .keys()
                                .any(|key| !matches!(key.as_str(), "header" | "ctCode"))
                        {
                            return Err(Error::Response("fixture device SMS scope"));
                        }
                        return Ok(auth::reply(json!({"code":200}), None));
                    }
                    if data["deviceKey"] != "fixture-private-key-a"
                        || fields
                            .keys()
                            .any(|key| !matches!(key.as_str(), "header" | "deviceKey" | "captcha"))
                    {
                        return Err(Error::Response("fixture device kickoff scope"));
                    }
                    if mode == "device_removal_sms" && !fields.contains_key("captcha") {
                        return Ok(auth::reply(json!({"code":401}), None));
                    }
                    if mode == "device_removal_drop"
                        && data["captcha"] == "001234"
                        && self.paths.lock().unwrap().get("/api/sms/captcha/safe/sent") == Some(&1)
                        && self.removed_devices.lock().unwrap().insert(uid)
                    {
                        return Err(Error::Transport(
                            "synthetic device removal reply lost".into(),
                        ));
                    }
                    return Err(Error::Response(
                        "fixture device kickoff repeated or invalid",
                    ));
                }
                if path == "/api/user/profile/update" {
                    let data = data
                        .as_object()
                        .ok_or(Error::Response("fixture profile patch"))?;
                    if data
                        .keys()
                        .any(|key| !matches!(key.as_str(), "header" | "signature"))
                    {
                        return Err(Error::Response("unexpected fixture profile patch field"));
                    }
                    let signature = data
                        .get("signature")
                        .and_then(Value::as_str)
                        .filter(|value| matches!(*value, "桌面验证：已保存" | "桌面验证：回包丢失"))
                        .ok_or(Error::Response("fixture signature value"))?;
                    self.profiles.lock().unwrap().insert(uid, json!(signature));
                    if mode == "profile_ack_lost" {
                        return Err(Error::Transport(
                            "synthetic write acknowledged remotely only".into(),
                        ));
                    }
                    return Ok(auth::reply(json!({"code":"200"}), None));
                }
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
                        let mut rows = Vec::new();
                        if !self.removed_devices.lock().unwrap().contains(&uid) {
                            rows.push(json!({"deviceKey":"fixture-private-key-a","deviceName":"旧设备","status":0,"region":"上海"}));
                        }
                        rows.push(json!({"deviceKey":"fixture-private-key-b","deviceName":"测试桌面","status":"1","ifLoginDevice":true,"loginPlace":"杭州","lastActiveTime":1700000000000_u64,"os":"Windows"}));
                        json!(rows)
                    };
                    let limits = if mode == "devices_empty" || uid == 5 {
                        json!({"isEmpty":true,"nickname":"不应显示的合成设备账户"})
                    } else if mode == "devices_ordinary" {
                        json!({"vipType":1,"nickname":"畅听合成账号","currentNum":0,"deviceLimitRule":{"1":4}})
                    } else {
                        json!({"vipType":6,"nickname":"设备额度合成账号","currentNum":1,
                            "deviceLimit":30,"deviceLimitRule":{"6":9,"15":11},
                            "familyLink":"https://example.invalid/?auth=synthetic-family-private",
                            "familyText":"不应显示的家庭会员链接","familyImageUrl":"https://example.invalid/family.png"})
                    };
                    return Ok(auth::reply(
                        json!({"code":200,"data":{"userDeviceList":rows,"userDeviceLimitInfoDTO":limits}}),
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
                    let signature = self
                        .profiles
                        .lock()
                        .unwrap()
                        .get(&uid)
                        .cloned()
                        .unwrap_or_else(|| {
                            json!(if uid == 2 {
                                "这是合成的账号资料"
                            } else {
                                "另一个账号的资料"
                            })
                        });
                    let avatar = self.avatar_proof.then(|| {
                        self.avatars
                            .lock()
                            .unwrap()
                            .current
                            .clone()
                            .unwrap_or_else(|| {
                                "https://p5.music.126.net/avatar-proof-baseline.jpg".into()
                            })
                    });
                    return Ok(auth::reply(
                        json!({"code":200,"level":7,"profile":{
                            "userId":uid,"nickname":auth::nickname(uid),
                            "signature":signature,"avatarUrl":avatar,
                            "birthday":946684800000_i64,"gender":1,"province":330000,"city":330100
                        }}),
                        None,
                    ));
                }
                if path.contains("/bindings/") && mode.starts_with("device_removal_") {
                    let bindings = if mode == "device_removal_unbound" {
                        json!([])
                    } else {
                        json!([{"type":1,"tokenJsonStr":json!({
                            "cellphone":"1234567890","countrycode":"1242",
                            "access_token":"fixture-device-binding-private"
                        }).to_string()}])
                    };
                    return Ok(auth::reply(json!({"code":200,"bindings":bindings}), None));
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
            if mode == "qr_identity_recovery" && path == "/api/w/nuser/account/get" {
                let candidate = plan.headers.get("Cookie").is_some_and(|cookie| {
                    cookie
                        .split(';')
                        .any(|pair| pair.trim() == "MUSIC_U=fixture2")
                });
                if candidate {
                    let mut reads = self.qr_candidate_identity_reads.lock().unwrap();
                    *reads += 1;
                    if *reads <= 2 {
                        return Ok(auth::reply(json!({"code":301}), None));
                    }
                }
            }
            if path == "/api/login/qrcode/client/login" {
                return Ok(match mode.as_str() {
                    "qr_scanned" => auth::reply(json!({"code":802}), None),
                    "qr_expired" => auth::reply(json!({"code":800}), None),
                    "qr_poll_error" => auth::reply(json!({"code":500}), None),
                    "qr_authorized" | "qr_identity_recovery" => {
                        auth::reply(json!({"code":803}), Some(2))
                    }
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

impl NosBackend for Service {
    fn discover<'a>(&'a self, bucket: &'a str) -> ApiFuture<'a, NosNode> {
        Box::pin(async move {
            if !self.avatar_proof || bucket != "yyimgs" {
                return Err(Error::Response("unexpected avatar NOS discovery"));
            }
            NosNode::parse("https://nosup-avatar-proof.127.net")
        })
    }

    fn request(&self, request: NosRequest) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            let url =
                tauri::Url::parse(request.url()).map_err(|_| Error::Response("avatar NOS URL"))?;
            if !self.avatar_proof
                || url.host_str() != Some("nosup-avatar-proof.127.net")
                || request.scoped_token() != "avatar-proof-scope"
                || request.content_type() != "image/jpeg"
            {
                return Err(Error::Response("avatar NOS scope"));
            }
            let mut avatars = self.avatars.lock().unwrap();
            if request.method() == NosMethod::Chunk {
                let offset = url
                    .query_pairs()
                    .find(|(key, _)| key == "offset")
                    .and_then(|(_, value)| value.parse::<usize>().ok());
                let object = avatars.objects.entry(url.path().into()).or_default();
                if offset != Some(object.len()) {
                    return Err(Error::Response("avatar NOS offset"));
                }
                object.extend_from_slice(request.body());
                avatars.chunks += 1;
            }
            let offset = avatars.objects.get(url.path()).map_or(0, Vec::len);
            Ok(json!({"offset":offset,"context":"synthetic-avatar-context"}))
        })
    }
}

pub fn avatar_source() -> Vec<u8> {
    let (width, height) = (480_u32, 360_u32);
    let size = 54 + width * height * 3;
    let mut bytes = b"BM".to_vec();
    for n in [size, 0, 54, 40, width, height] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(24_u16.to_le_bytes());
    for n in [0_u32, size - 54, 2835, 2835, 0, 0] {
        bytes.extend(n.to_le_bytes());
    }
    for y in 0..height {
        for x in 0..width {
            bytes.extend([(x * 255 / width) as u8, (y * 255 / height) as u8, 145]);
        }
    }
    bytes
}
