use super::helpers::*;
use serde_json::{Map, Value, json};

pub(super) fn voice(input: &Value, origin: &Value) -> Value {
    let mut m = Map::new();
    copy(&mut m, "programId", origin.get("id"));
    for (to, from) in [("djId", "id"), ("djName", "nickname")] {
        copy(&mut m, to, origin["dj"].get(from));
    }
    copy(&mut m, "programName", origin.get("name"));
    copy(&mut m, "brand", origin["dj"].get("brand"));
    copy(&mut m, "serial", origin.get("serialNum"));
    m.insert("mainMusic".into(), origin.clone());
    copy(&mut m, "djAvatarUrl", origin["dj"].get("avatarUrl"));
    copy(&mut m, "createTime", origin.get("createTime"));
    m.insert(
        "programDesc".into(),
        default(origin.get("description"), json!("")),
    );
    for (key, value) in [
        ("radioId", json!("")),
        ("radioName", json!("")),
        ("radioDesc", json!("")),
        ("radioCategoryId", json!(0)),
        ("radioCategory", json!("")),
        ("radioFeeType", json!(0)),
        ("radioFeeScope", json!(0)),
        ("volumeDelta", default(input.get("volumeDelta"), json!(0))),
    ] {
        m.insert(key.into(), value);
    }
    if truth(origin.get("audio")) {
        let a = &origin["audio"];
        for (to, from) in [
            ("radioId", "id"),
            ("radioName", "name"),
            ("radioCategoryId", "categoryId"),
        ] {
            if let Some(value) = a.get(from) {
                m.insert(to.into(), value.clone());
            } else {
                m.remove(to);
            }
        }
        for (to, from, fallback) in [
            ("radioDesc", "desc", json!("")),
            ("radioCategory", "category", json!("")),
            ("radioFeeType", "radioFeeType", json!(0)),
            ("radioFeeScope", "feeScope", json!(0)),
        ] {
            m.insert(to.into(), default(a.get(from), fallback));
        }
    }
    m.into()
}
