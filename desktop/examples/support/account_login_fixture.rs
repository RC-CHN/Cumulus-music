//! Explicit fault controls for the standalone account/login example only.
use super::auth;
use cumulus_core::{
    Error,
    cloud::nos::{NosBackend, NosMethod, NosNode, NosRequest},
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
    profiles: Mutex<BTreeMap<u64, Value>>,
    avatar_proof: bool,
    avatars: Mutex<Avatars>,
}
#[derive(Default)]
struct Avatars {
    current: Option<String>,
    allocated: u64,
    chunks: u64,
    objects: BTreeMap<String, Vec<u8>>,
}
impl Service {
    pub fn new(avatar_proof: bool) -> Self {
        Self {
            avatar_proof,
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
        } else {
            *self.mode.lock().unwrap() = mode.into();
        }
    }
    pub fn snapshot(&self) -> Value {
        let avatars = self.avatars.lock().unwrap();
        json!({"paths":*self.paths.lock().unwrap(),"pending":self.held.lock().unwrap().is_some(),"completed":*self.completed.lock().unwrap(),"browserDocuments":*self.browser_documents.lock().unwrap(),"avatarChunks":avatars.chunks,"avatarObjects":avatars.objects.len()})
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
