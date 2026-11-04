use super::{CommunityAttachment, CommunityCard, CommunityCardStatus, CommunityCounters, Locator};
use crate::{
    client::mapping,
    protocol::{api::identifier, artwork},
};
use serde_json::Value;

const UNSUPPORTED: [i64; 7] = [21, 24, 39, 41, 42, 57, 58];
const JS_TIME_MAX: u64 = 8_640_000_000_000_000;
fn note_body(event: &Value) -> Option<Value> {
    event["json"]
        .as_str()
        .filter(|text| text.len() <= 128 * 1024)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .filter(Value::is_object)
}

pub(super) fn tombstone(event: &Value) -> bool {
    event["type"].as_i64() == Some(22)
        && note_body(event)
            .is_some_and(|body| body["event"].is_null() && appendix(&body, 22, true).is_none())
}

pub(super) fn id(value: &Value) -> Option<String> {
    identifier(value).ok().filter(|v| v.len() <= 128)
}

pub(super) fn cursor(value: &Value) -> Option<Value> {
    // Decimal text from the response must be echoed without conversion to a
    // JS number; preserve the JSON type too. -1 is only a request sentinel.
    cursor_key(value).map(|_| value.clone())
}

pub(super) fn cursor_key(value: &Value) -> Option<String> {
    let raw = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    if raw.is_empty() || raw.len() > 128 || !raw.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let key = raw.trim_start_matches('0');
    Some(if key.is_empty() {
        "0".into()
    } else {
        key.into()
    })
}

pub(super) fn text(value: &Value, limit: usize) -> Option<String> {
    let text: String = value
        .as_str()?
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(limit)
        .collect();
    (!text.trim().is_empty()).then_some(text)
}

fn counts(info: &Value) -> CommunityCounters {
    let get = |key: &str| info[key].as_u64().filter(|n| *n <= 9_007_199_254_740_991);
    CommunityCounters {
        likes: get("likedCount"),
        shares: get("shareCount"),
        comments: get("commentCount"),
    }
}

fn simple_attachment(kind: &'static str, value: &Value) -> Option<CommunityAttachment> {
    let id = id(&value["id"])?;
    let label = text(&value["name"], 200)?;
    Some(CommunityAttachment::Resource {
        kind,
        id,
        label,
        cover_url: ["picUrl", "coverImgUrl", "img1v1Url"]
            .into_iter()
            .find_map(|key| mapping::artwork(&value[key])),
    })
}

fn appendix(body: &Value, kind: i64, allow_forward: bool) -> Option<CommunityAttachment> {
    // M2157 uses the first recognized branch, in this order.
    if !body["song"].is_null() {
        return simple_attachment("track", &body["song"]);
    }
    if !body["playlist"].is_null() {
        return simple_attachment("playlist", &body["playlist"]);
    }
    if !body["album"].is_null() {
        return simple_attachment("album", &body["album"]);
    }
    if kind == 36 && !body["resource"].is_null() {
        return simple_attachment("artist", &body["resource"]);
    }
    if !body["artist"].is_null() {
        return simple_attachment("artist", &body["artist"]);
    }
    if (kind == 31 && !body["resource"].is_null()) || !body["comment"].is_null() {
        return Some(CommunityAttachment::Unavailable {
            label: "分享评论"
        });
    }
    if let Some(radio) = ["djRadio", "djradio"]
        .into_iter()
        .find_map(|key| body.get(key).filter(|v| v.is_object()))
    {
        return simple_attachment("podcast", radio);
    }
    if let Some(program) = ["program", "djprogram"]
        .into_iter()
        .find_map(|key| body.get(key).filter(|v| v.is_object()))
    {
        return simple_attachment("voice", program);
    }
    if !body["show"].is_null() {
        return Some(CommunityAttachment::Unavailable {
            label: "分享演出"
        });
    }
    if !body["event"].is_null() {
        return allow_forward
            .then(|| card(&body["event"], false))
            .flatten()
            .map(|(_, _, card)| CommunityAttachment::Forwarded {
                card: Box::new(card),
            });
    }
    if kind == 56 && !body["resource"].is_null() {
        return Some(CommunityAttachment::Unavailable {
            label: "该附件请前往 App 查看",
        });
    }
    None
}

pub(super) fn card(
    event: &Value,
    allow_forward: bool,
) -> Option<(String, Option<Locator>, CommunityCard)> {
    let event_id = id(&event["id"])?;
    let kind = event["type"].as_i64()?;
    // The desktop source passes the raw account into its own web application.
    // For uncertain anonymous identities, our IPC never reveals the author or
    // produces a detail handle that could correlate that author elsewhere.
    let anonymous = !event["anonymityInfo"].is_null();
    let user = &event["user"];
    let owner = (!anonymous).then(|| id(&user["userId"])).flatten();
    let mut card = CommunityCard {
        key: String::new(),
        author_name: if anonymous {
            "匿名用户".into()
        } else {
            ["nickname", "nickName", "userName"]
                .into_iter()
                .find_map(|key| text(&user[key], 100))
                .unwrap_or_else(|| "网易云音乐用户".into())
        },
        avatar_url: (!anonymous)
            .then(|| mapping::artwork(&user["avatarUrl"]))
            .flatten(),
        event_time: event["eventTime"].as_u64().filter(|n| *n <= JS_TIME_MAX),
        time_text: text(&event["eventTime"], 100),
        title: None,
        message: None,
        pictures: Vec::new(),
        attachment: None,
        counters: counts(&event["info"]),
        status: CommunityCardStatus::Ready,
        detail: None,
        can_delete: false,
        can_report: false,
    };
    if UNSUPPORTED.contains(&kind) {
        card.status = CommunityCardStatus::Unsupported;
        return Some((event_id, None, card));
    }
    let Some(body) = note_body(event) else {
        card.status = CommunityCardStatus::Unavailable;
        return Some((event_id, None, card));
    };
    card.title = text(&body["title"], 300);
    card.message = text(&body["msg"], 8000);
    card.pictures = event["pics"]
        .as_array()
        .into_iter()
        .flatten()
        .take(9)
        .filter_map(|pic| {
            if pic["format"] == "gif" {
                artwork::original(&pic["pcSquareUrl"])
            } else {
                artwork::original(&pic["originUrl"])
            }
        })
        .collect();
    card.attachment = appendix(&body, kind, allow_forward);
    if kind == 22 && body["event"].is_null() {
        card.status = CommunityCardStatus::Deleted;
        return Some((event_id, None, card));
    }
    let locator = owner.map(|owner| Locator {
        event: event_id.clone(),
        owner,
    });
    Some((event_id, locator, card))
}
