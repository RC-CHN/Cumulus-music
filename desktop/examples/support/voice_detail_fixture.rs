//! Closed voice detail metadata/profile fixtures; no external service or writes.
use serde_json::{Value, json};
use std::sync::Mutex;

pub const RELEASE: u64 = 1_700_049_600_000;
#[derive(Default)]
pub struct VoiceDetail {
    calls: Mutex<Vec<Value>>,
}
impl VoiceDetail {
    pub fn calls(&self) -> Vec<Value> {
        self.calls.lock().unwrap().clone()
    }
    pub fn profile(&self, path: &str, args: &Value) -> Option<Value> {
        let result = match path {
            "/api/w/v1/user/detail/55" => {
                assert_eq!(*args, json!({"all":true,"userId":"55"}));
                json!({"code":200,"profile":{"userId":55,"nickname":"合成主播主页","followed":false}})
            }
            "/api/djradio/get/create/sub/count" => {
                assert_eq!(*args, json!({"userId":"55"}));
                json!({"code":200,"createRadioCount":0,"subRadioCount":0})
            }
            "/api/user/playlist" if args["uid"] == "55" => {
                assert_eq!(args["offset"], 0);
                json!({"code":200,"playlist":[],"more":false})
            }
            _ => return None,
        };
        self.calls.lock().unwrap().push(json!([path, args]));
        Some(result)
    }
}
pub fn enrich(mut response: Value) -> Value {
    let description = std::env::var("CUMULUS_VOICE_DESCRIPTION_PROOF").ok();
    assert!(
        description
            .as_deref()
            .is_none_or(|case| matches!(case, "short" | "missing"))
    );
    fn voice(value: &mut Value, description: Option<&str>) {
        let Some(id) = value.get("id").and_then(Value::as_u64) else {
            return;
        };
        assert!(matches!(id, 601 | 602));
        value["creator"] = json!({
            "userId":55,"nickname":"合成主播",
            "cookie":"synthetic-private-field-never-project",
            "email":"synthetic-private-field-never-project",
        });
        value["createTime"] = json!(RELEASE);
        if description.is_none() && id == 601 {
            value["listenerCount"] = json!(0);
            value["playCount"] = json!(123456);
            value["adjustedPlayCount"] = json!("unexpected fallback");
        }
        match (description, id) {
            (Some(_), 601) => {
                value["description"] = json!(concat!(
                    "第一行合成节目简介\n第二行说明不改变播放\n第三行保留换行\n",
                    "第四行仍然可见\n第五行仅展开后可见\n第六行结尾标记"
                ));
            }
            (Some("short"), 602) => value["description"] = json!("这是合成短简介"),
            (Some("missing"), 602) => assert!(value.get("description").is_none()),
            _ => {}
        }
    }
    if let Some(rows) = response
        .get_mut("data")
        .and_then(Value::as_array_mut)
        .and_then(|v| v.first_mut())
        .and_then(|v| v.get_mut("creatives"))
        .and_then(Value::as_array_mut)
    {
        for row in rows {
            if let Some(voices) = row["creativeExtInfoVO"]["djPrograms"].as_array_mut() {
                for v in voices {
                    voice(v, description.as_deref());
                }
            }
        }
    }
    if let Some(program) = response.get_mut("program") {
        voice(program, description.as_deref());
    }
    response
}
