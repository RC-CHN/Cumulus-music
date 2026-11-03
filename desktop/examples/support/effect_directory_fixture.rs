use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_EFFECT_DIRECTORY_PROOF").is_some()
}

fn sound(id: u64) -> Value {
    let (name, subtitle) = match id {
        200 => ("合成空间音效", "合成强推荐"),
        300 => ("合成均衡音效", "合成推荐"),
        _ => unreachable!("validated effect fixture ID"),
    };
    json!({"id":id,"name":name,"subTitle":subtitle,"soundType":3,
        "coverUrl":"https://example.invalid/cover-sentinel",
        "lightImg":"https://p1.music.126.net/cumulus-proof-artwork-light.png?token=private-sentinel",
        "darkImg":"http://p1.music.126.net/cumulus-proof-artwork-dark.png#private-sentinel",
        "soundMaterialList":[{"link":"https://example.invalid/material-sentinel"}]})
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
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
                {"soundId":4101,"deviceModel":"合成耳机甲","soundMd5":"private-sentinel","soundUrl":"https://example.invalid/model-material-sentinel"},
                {"soundId":"4102","deviceModel":"合成耳机乙"}
            ])
        }
        "/api/usertool/home/soundeffect/devicesound/list"
            if args == &json!({"deviceManagerId":4200}) =>
        {
            json!([])
        }
        "/api/usertool/sound/queryAllSound" => match args["exposeType"].as_u64() {
            Some(5) if args == &json!({"exposeType":5}) => json!([sound(200)]),
            Some(6) if args == &json!({"exposeType":6}) => json!([sound(300)]),
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
