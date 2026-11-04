pub(super) mod attachment;

use super::{MessagePage, MessageQuery, MessageRow, MessageTab, valid_cursor};
use crate::{Error, Result};
use serde_json::Value;
use std::hash::{DefaultHasher, Hash, Hasher};

const UNSUPPORTED: &str = "当前客户端无法显示这条信息";

pub(super) fn number(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|value| *value < u64::from(u32::MAX))
}

pub(super) fn positive_id(value: &Value) -> Option<String> {
    let value = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|id| id.to_string()))?;
    (value.len() <= 20 && value.bytes().all(|ch| ch.is_ascii_digit()) && !value.starts_with('0'))
        .then_some(value)
}

fn time(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|value| valid_cursor(*value))
}

fn text(value: &Value, max: usize) -> Option<String> {
    let value = value.as_str()?.trim();
    (!value.is_empty()).then(|| {
        value
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
            .take(max)
            .collect()
    })
}

fn payload(value: &Value) -> Value {
    match value {
        Value::Object(_) => value.clone(),
        Value::String(raw) if raw.len() <= 65_536 => {
            serde_json::from_str(raw).unwrap_or(Value::Null)
        }
        _ => Value::Null,
    }
}

fn title(value: &Value) -> Option<String> {
    ["name", "title", "nickname", "content", "text"]
        .into_iter()
        .find_map(|key| text(&value[key], 300))
}

fn sender(value: &Value) -> Option<String> {
    text(&value["nickname"], 100)
}

pub(super) fn user_id(value: &Value) -> Option<String> {
    positive_id(&value["userId"]).or_else(|| positive_id(&value["id"]))
}

pub(super) fn is_plain_reply(value: &Value) -> bool {
    let content = payload(value);
    number(&content["type"]) == Some(6)
        && content["msg"].as_str().is_some_and(|text| !text.is_empty())
}

fn sharing(value: &Value) -> (Option<String>, String) {
    let kind = number(&value["type"]);
    let resource = match kind {
        Some(1 | 14) => Some(("分享歌曲", "song")),
        Some(2) => Some(("分享专辑", "album")),
        Some(3) => Some(("分享歌手", "artist")),
        Some(4) => Some(("分享歌单", "playlist")),
        Some(5) => Some(("分享声音", "program")),
        Some(7) => Some(("分享视频", "mv")),
        Some(8) => Some(("分享专栏文章", "topic")),
        Some(10) => Some(("分享名片", "user")),
        Some(11) => Some(("分享活动", "activity")),
        Some(12) => Some(("分享外链", "promotionUrl")),
        Some(13) => Some(("分享播客", "djRadio")),
        Some(15) => Some(("分享评论", "comment")),
        Some(22) => Some(("分享演出", "show")),
        Some(24) => Some(("分享视频", "video")),
        _ => None,
    };
    if let Some((caption, field)) = resource {
        return (
            Some(caption.into()),
            text(&value["msg"], 2_000)
                .or_else(|| title(&value[field]))
                .unwrap_or_else(|| caption.into()),
        );
    }
    match kind {
        Some(6) => (None, text(&value["msg"], 2_000).unwrap_or_default()),
        Some(16) => (None, "「图片」".into()),
        Some(23) => (
            None,
            text(&value["generalMsg"]["inboxBriefContent"], 2_000)
                .unwrap_or_else(|| UNSUPPORTED.into()),
        ),
        _ => (None, UNSUPPORTED.into()),
    }
}

fn notice(value: &Value) -> (Option<String>, String) {
    let label = match number(&value["type"]) {
        Some(1) => ("赞了你的笔记", "track"),
        Some(2) => ("收藏了你的歌单", "playlist"),
        Some(3) => ("分享了你的歌单", "playlist"),
        Some(4) => ("分享了你的声音", "program"),
        Some(5) => ("收藏了你的声音", "program"),
        Some(6) => ("赞了你的评论", "comment"),
        Some(7) => ("赞了你的声音", "program"),
        Some(8) => ("收藏了你的播客", "djRadio"),
        Some(9) => ("赞了你的专栏文章", "topic"),
        Some(10) => {
            return (
                text(&value["generalNotice"]["actionDesc"], 300),
                text(&value["generalNotice"]["content"], 2_000)
                    .unwrap_or_else(|| UNSUPPORTED.into()),
            );
        }
        _ => return (None, UNSUPPORTED.into()),
    };
    let body = if label.1 == "track" {
        let track = payload(&value["track"]["json"]);
        text(&track["msg"], 2_000).or_else(|| {
            [
                "playlist", "song", "album", "program", "djRadio", "topic", "resource", "show",
                "video", "mv",
            ]
            .into_iter()
            .find_map(|key| title(&track[key]))
        })
    } else {
        title(&value[label.1])
    };
    (Some(label.0.into()), body.unwrap_or_default())
}

fn row_id(value: &Value) -> String {
    positive_id(&value["id"])
        .or_else(|| positive_id(&value["commentId"]))
        .unwrap_or_else(|| {
            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            format!("item-{:016x}", hasher.finish())
        })
}

pub(super) fn project_row(tab: MessageTab, value: &Value, uid: &str) -> MessageRow {
    let mut row = MessageRow {
        id: String::new(),
        peer_id: None,
        author: String::new(),
        body: UNSUPPORTED.into(),
        caption: None,
        quote: None,
        time: time(&value["time"]),
        unread: None,
        own: false,
        target: None,
        deletion_receipt: None,
        deletion_pending: false,
    };
    let fallback = "未知用户";
    match tab {
        MessageTab::Private | MessageTab::Thread => {
            let user = &value["fromUser"];
            let id = user_id(user);
            row.author = sender(user).unwrap_or_else(|| fallback.into());
            row.own = id.as_deref() == Some(uid);
            let private = tab == MessageTab::Private;
            let detail = payload(&value[if private { "lastMsg" } else { "msg" }]);
            (row.caption, row.body) = sharing(&detail);
            if !private {
                row.target = attachment::thread(&detail);
            }
            if private {
                row.peer_id = id;
                row.time = time(&value["lastMsgTime"]);
                row.unread = number(&value["newMsgCount"]);
            }
        }
        MessageTab::Comments => {
            row.author = sender(&value["user"]).unwrap_or_else(|| fallback.into());
            row.body = text(&value["content"], 2_000).unwrap_or_default();
            row.quote = text(&value["beRepliedContent"], 1_000);
            row.caption = sender(&value["beRepliedUser"]).map(|name| format!("回复 {name}"));
            row.target = attachment::received_comment(value);
        }
        MessageTab::Forwards => {
            let event = payload(&value["json"]);
            let kind = number(&value["type"]);
            row.target = kind.and_then(|kind| attachment::forwarded(kind, &event));
            match kind {
                Some(1) => {
                    row.author = sender(&event["user"]).unwrap_or_else(|| fallback.into());
                    row.time = time(&event["eventTime"]).or(row.time);
                    let detail = payload(&event["json"]);
                    row.body = text(&detail["msg"], 2_000).unwrap_or_default();
                    row.quote = [
                        "song", "album", "playlist", "program", "djRadio", "resource", "show",
                    ]
                    .into_iter()
                    .find_map(|field| title(&detail[field]));
                    row.caption = Some("在动态中提到了你".into());
                }
                Some(2 | 3) => {
                    let comment = &event["comment"];
                    row.author = sender(&comment["user"]).unwrap_or_else(|| fallback.into());
                    row.time = time(&comment["time"]).or(row.time);
                    row.body = text(&comment["content"], 2_000).unwrap_or_default();
                    row.quote = title(&event["resource"]);
                    row.caption = Some("在评论中提到了你".into());
                }
                _ => {}
            }
        }
        MessageTab::Notices => {
            let detail = payload(&value["notice"]);
            row.author = sender(&detail["user"]).unwrap_or_else(|| "通知".into());
            (row.caption, row.body) = notice(&detail);
            row.target = attachment::notice(&detail);
        }
    }
    row.id = row.peer_id.clone().unwrap_or_else(|| row_id(value));
    row
}

pub(super) fn project(
    query: &MessageQuery,
    response: &Value,
    uid: &str,
    limit: usize,
) -> Result<MessagePage> {
    let field = match query.tab {
        MessageTab::Private | MessageTab::Thread => "msgs",
        MessageTab::Comments => "comments",
        MessageTab::Forwards => "forwards",
        MessageTab::Notices => "notices",
    };
    let raw = response[field]
        .as_array()
        .filter(|rows| rows.len() <= limit && rows.iter().all(Value::is_object))
        .ok_or(Error::Response("invalid message page"))?;
    let more = response["more"]
        .as_bool()
        .ok_or(Error::Response("invalid message continuation"))?;
    let more = more && !(query.tab == MessageTab::Comments && raw.is_empty());
    let rows: Vec<_> = raw
        .iter()
        .map(|value| project_row(query.tab, value, uid))
        .collect();
    let next_cursor = if more {
        let cursor = match query.tab {
            MessageTab::Forwards => time(&response["lasttime"]),
            MessageTab::Notices => time(&response["lastTime"]),
            _ => rows.last().and_then(|row| row.time),
        };
        Some(
            cursor
                .filter(|cursor| query.cursor.is_none_or(|previous| *cursor < previous))
                .ok_or(Error::Response("message cursor does not advance"))?,
        )
    } else {
        None
    };
    Ok(MessagePage {
        rows,
        next_cursor,
        reply_receipt: None,
        hint: if query.tab == MessageTab::Thread {
            text(&response["hint"], 500)
        } else {
            None
        },
    })
}
