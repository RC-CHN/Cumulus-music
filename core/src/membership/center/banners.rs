use crate::{
    Error, Result,
    protocol::wire::serialization::{js_string, js_truthy},
};
use serde_json::{Value, json};

pub(super) fn normalize(data: &Value, top: bool) -> Result<Value> {
    let mut output = vec![];
    let rows = match data.get("contentList") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(rows)) => rows.as_slice(),
        _ => return Err(Error::Response("invalid member banner list")),
    };
    for value in rows {
        let mut row = if let Some(text) = value.as_str() {
            serde_json::from_str(text)?
        } else {
            value.clone()
        };
        if !row.is_object() {
            return Err(Error::Response("invalid member banner"));
        }
        if row["imgUrl"].as_str().is_none_or(|s| s.is_empty()) {
            continue;
        }
        if !top {
            if row["jumpUrl"].as_str().is_some_and(|s| !s.is_empty()) {
                output.push(row);
            }
            continue;
        }
        if row["algFlag"].as_i64() == Some(1) {
            continue;
        }
        match row["resourceType"].as_str() {
            Some("SONG") => {
                row["action"] = json!({"kind":"play","resourceId":row.get("resourceId").filter(|v|!v.is_null()).cloned().unwrap_or(json!("")),"resourceType":"track"})
            }
            Some("PLAYLIST") => {
                row["action"] = json!({"kind":"link","linkPath":format!("/playlist/{}",row.get("resourceId").map(js_string).transpose()?.unwrap_or_else(||"undefined".into()))})
            }
            _ if (row["resourceType"] == "ACTIVITY" || !js_truthy(&row["resourceType"]))
                && row["jumpUrl"].as_str().is_some_and(|s| !s.is_empty()) =>
            {
                row["resourceType"] = json!("ACTIVITY");
                row["action"] = json!({"kind":"link","linkPath":row["jumpUrl"]});
            }
            _ => (),
        }
        if js_truthy(&row["action"]) {
            output.push(row);
        }
    }
    if top && output.is_empty() {
        const LINK: &str = "https://music.163.com/#/playlist?id=7785091694";
        output.push(json!({"title":"兜底","resourceType":"PC_BACKUP","imgUrl":"https://p6.music.126.net/obj/wonDlsKUwrLClGjCm8Kx/28130577125/e38e/d6da/cdef/f1bd709679d39ed581fa6c9182626754.png","jumpUrl":LINK,"action":{"kind":"link","linkPath":LINK}}));
    }
    Ok(json!({"kind":if top {"topBanner"} else {"doubleBanners"},"value":output}))
}
