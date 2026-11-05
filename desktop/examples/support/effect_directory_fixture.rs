use super::effect_peq_fixture;
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static SOURCE_CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_EFFECT_DIRECTORY_PROOF").is_some()
}

pub fn source_bypass_enabled() -> bool {
    std::env::var_os("CUMULUS_EFFECT_DEVICE_SOURCE_BYPASS_PROOF").is_some()
}

pub fn restrict_source_bypass_artwork(context: &mut tauri::Context<tauri::Wry>) {
    use tauri::utils::config::{Csp, CspDirectiveSources};
    let security = &mut context.config_mut().app.security;
    let mut directives = std::collections::HashMap::from(security.csp.take().expect("probe CSP"));
    directives.insert(
        "img-src".into(),
        CspDirectiveSources::Inline("'self' asset: http://asset.localhost data:".into()),
    );
    security.csp = Some(Csp::from(directives));
    security.dev_csp = security.csp.clone();
}

fn source_response(args: &Value) -> Result<Value> {
    let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().unwrap_or_default())?;
    let id = match ids.as_slice() {
        [id] => id.as_u64().or_else(|| id.as_str()?.parse().ok()),
        _ => None,
    };
    let (requested, returned, br) = match id {
        Some(71) => ("vivid", "standard", 6_999_000),
        Some(72) => ("standard", "vivid", 128_000),
        Some(73) => ("standard", "dolby", 128_000),
        Some(74) => ("standard", "sky", 128_000),
        Some(75) => ("standard", "jymaster", 4_999_000),
        _ => return Err(Error::Response("unapproved headphone source proof track")),
    };
    if args["level"] != requested {
        return Err(Error::Response(
            "wrong headphone source proof request quality",
        ));
    }
    SOURCE_CALLS
        .lock()
        .unwrap()
        .push(json!({"id":id,"requested":requested,"returned":returned}));
    Ok(
        json!({"code":200,"data":[{"id":id,"url":effect_peq_fixture::AUDIO_URL,
        "code":200,"md5":"fixture","level":returned,"br":br,"expi":120}]}),
    )
}

pub fn source_calls() -> Vec<Value> {
    SOURCE_CALLS.lock().unwrap().clone()
}

fn sound(id: u64) -> Value {
    let (name, subtitle, tier) = match id {
        200 => ("合成空间音效", "合成强推荐", 15),
        300 => ("合成均衡音效", "合成推荐", 6),
        301 => ("合成未标识音效", "未知会员等级", 99),
        _ => unreachable!("validated effect fixture ID"),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let expires = if id == 300 {
        now.saturating_sub(1)
    } else {
        now + 5_000
    };
    json!({"id":id,"name":name,"subTitle":subtitle,"soundType":3,
        "coverUrl":"https://example.invalid/cover-sentinel",
        "vipAuthRespItem":{"interestsVipType":tier,"authResult":{"trialDto":{"trialStatus":1,"expireTime":expires}}},
        "lightImg":"https://p1.music.126.net/cumulus-proof-artwork-light.png?token=private-sentinel",
        "darkImg":"http://p1.music.126.net/cumulus-proof-artwork-dark.png#private-sentinel",
        "soundMaterialList":[{"link":"https://example.invalid/material-sentinel"}]})
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if source_bypass_enabled() && path == "/api/song/enhance/player/url/v1" {
        return Some(source_response(args));
    }
    let value = match path {
        "/api/usertool/home/soundeffect/devicebrand/list" if args == &json!({}) => json!([
            {"deviceManagerId":"4100","deviceName":"合成耳机品牌","deviceImgUrl":"https://example.invalid/brand-artwork-sentinel"},
            {"deviceManagerId":4200,"deviceName":"合成空品牌"}
        ]),
        "/api/usertool/home/soundeffect/devicesound/list"
            if args == &json!({"deviceManagerId":4100}) =>
        {
            json!([
                {"soundId":4101,"soundType":2,"deviceModel":"合成耳机甲"},
                {"soundId":"4102","soundType":2,"deviceModel":"合成耳机乙",
                 "size":effect_peq_fixture::NCAE.len()/2,"soundMd5":effect_peq_fixture::MD5,
                 "deviceSoundUrl":effect_peq_fixture::URL}
            ])
        }
        "/api/usertool/home/soundeffect/devicesound/list"
            if args == &json!({"deviceManagerId":4200}) =>
        {
            json!([])
        }
        "/api/usertool/sound/queryAllSound" => match args["exposeType"].as_u64() {
            Some(5) if args == &json!({"exposeType":5}) => json!([sound(200)]),
            Some(6) if args == &json!({"exposeType":6}) => json!([sound(300), sound(301)]),
            Some(7) if args == &json!({"exposeType":7}) => json!([]),
            _ => {
                return Some(Err(Error::Response(
                    "unexpected synthetic effect directory request",
                )));
            }
        },
        "/api/usertool/home/sound/info" => {
            let id = [200_u64, 300].into_iter().find(|id| {
                let req = json!([{"soundId":id.to_string(),"soundType":10}]).to_string();
                args == &json!({"reqListStr":req})
            });
            let Some(id) = id else {
                return Some(Err(Error::Response(
                    "unexpected synthetic effect detail request",
                )));
            };
            json!({"soundList":[{"soundId":id.to_string(),"soundType":10,"pcSoundDetail":sound(id)}]})
        }
        "/api/vipauth/app/interests/auth/query" => {
            if args == &json!({"queryInfo":{"interestsMap":{"116":[200]}}}) {
                json!([{"interestsId":200,"interestsType":116,
                    "authResp":{"authResult":{"canUse":true,"canUseType":50}}}])
            } else if args == &json!({"queryInfo":{"interestsMap":{"127":[300]}}}) {
                let mut calls = CALLS.lock().unwrap();
                if !calls
                    .iter()
                    .any(|call| call["path"] == path && call["args"] == *args)
                {
                    calls.push(json!({"path":path,"args":args}));
                    return Some(Err(Error::Response(
                        "synthetic unavailable rights response",
                    )));
                }
                json!([])
            } else {
                return Some(Err(Error::Response(
                    "unexpected synthetic effect rights request",
                )));
            }
        }
        path if path.starts_with("/api/vipauth/app/interests/")
            || path.starts_with("/api/usertool/sound/")
            || path.starts_with("/api/usertool/home/sound/")
            || path.starts_with("/api/usertool/home/soundeffect/") =>
        {
            CALLS
                .lock()
                .unwrap()
                .push(json!({"path":path,"unexpected":true}));
            return Some(Err(Error::Response("unapproved synthetic effect request")));
        }
        _ => return None,
    };
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    Some(Ok(json!({"code":200,"data":value})))
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}
