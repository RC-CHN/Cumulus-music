//! Bounded fields from M/21's songTag and M/3141's detail requests.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ReasonTarget {
    Recommendation {
        reason_id: String,
        song_id: String,
        name: String,
    },
    City {
        chart_code: String,
        target_id: String,
        target_type: String,
    },
}

pub(crate) fn atom(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn parameter(value: &Value) -> Option<String> {
    let text = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|n| n.to_string()))?;
    atom(&text).then_some(text)
}

pub(crate) fn text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 180
        && !value.contains("://")
        && !value.chars().any(char::is_control)
}

#[derive(Clone, Serialize)]
pub struct ReasonTag {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ReasonTarget>,
}

pub(crate) fn tags(value: &Value) -> Vec<ReasonTag> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .take(8)
        .filter_map(|tag| {
            let label = tag["text"].as_str()?.trim();
            if !text(label) {
                return None;
            }
            let target = (tag["clickable"] == true)
                .then(|| target(tag, label))
                .flatten();
            Some(ReasonTag {
                label: label.into(),
                target,
            })
        })
        .collect()
}

fn target(tag: &Value, label: &str) -> Option<ReasonTarget> {
    match tag["reasonType"].as_u64()? {
        1 => Some(ReasonTarget::Recommendation {
            reason_id: parameter(&tag["reasonId"])?,
            song_id: crate::protocol::api::identifier(&tag["resourceId"])
                .ok()
                .filter(|id| id.len() <= 32)?,
            name: label.into(),
        }),
        5 => {
            let direct = ["chartCode", "targetId", "targetType"]
                .into_iter()
                .all(|key| tag.get(key).is_some());
            let field = |key: &str| {
                if direct {
                    return parameter(&tag[key]);
                }
                let (_, query) = tag["url"]
                    .as_str()
                    .filter(|s| s.len() <= 2048)?
                    .split_once('?')?;
                let mut matches = url::form_urlencoded::parse(query.split('#').next()?)
                    .filter(|(name, _)| name == key);
                let (_, value) = matches.next()?;
                (matches.next().is_none() && atom(&value)).then(|| value.into_owned())
            };
            Some(ReasonTarget::City {
                chart_code: field("chartCode")?,
                target_id: field("targetId")?,
                target_type: field("targetType")?,
            })
        }
        _ => None,
    }
}

// V3 rows are normalized before projection. Preserve only bounded plain
// labels and parsed fields; even the intermediate result cannot contain a URL.
pub(crate) fn whitelist(value: &Value) -> Value {
    Value::Array(tags(value).into_iter().map(|tag| match tag.target {
        Some(ReasonTarget::Recommendation {reason_id, song_id, name}) =>
            json!({"text":name,"clickable":true,"reasonType":1,"reasonId":reason_id,"resourceId":song_id}),
        Some(ReasonTarget::City {chart_code, target_id, target_type}) =>
            json!({"text":tag.label,"clickable":true,"reasonType":5,"chartCode":chart_code,"targetId":target_id,"targetType":target_type}),
        None => json!({"text":tag.label}),
    }).collect())
}
