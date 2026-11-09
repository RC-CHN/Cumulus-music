//! M/3090, M/2119: project only confirmed actions and canonical identities.
use super::{number, payload, positive_id};
use crate::comments::{CommentResource, ResourceKind};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum MessageTarget {
    PlayTrack {
        id: String,
    },
    PlayVoice {
        id: String,
    },
    Playlist {
        id: String,
    },
    Podcast {
        id: String,
    },
    Album {
        id: String,
    },
    Artist {
        id: String,
    },
    Mv {
        id: String,
    },
    Comment {
        resource: CommentResource,
        comment_id: String,
    },
}

fn track(value: &Value) -> Option<MessageTarget> {
    Some(MessageTarget::PlayTrack {
        id: positive_id(&value["id"])?,
    })
}
fn voice(value: &Value) -> Option<MessageTarget> {
    Some(MessageTarget::PlayVoice {
        id: positive_id(&value["id"])?,
    })
}
fn playlist(value: &Value) -> Option<MessageTarget> {
    Some(MessageTarget::Playlist {
        id: positive_id(&value["id"])?,
    })
}
fn album(value: &Value) -> Option<MessageTarget> {
    Some(MessageTarget::Album {
        id: positive_id(&value["id"])?,
    })
}
fn artist(value: &Value) -> Option<MessageTarget> {
    Some(MessageTarget::Artist {
        id: positive_id(&value["id"])?,
    })
}

fn thread_target(
    thread: &str,
    comment: &Value,
    declared_kind: Option<u64>,
    declared_id: Option<&str>,
) -> Option<MessageTarget> {
    let (prefix, id) = thread.rsplit_once('_')?;
    let (kind, expected_kind) = match prefix {
        "A_PL_0" => (ResourceKind::Playlist, 0),
        "A_DJ_1" => (ResourceKind::Voice, 1),
        "R_AL_3" => (ResourceKind::Album, 3),
        "R_SO_4" => (ResourceKind::Track, 4),
        "R_MV_5" if declared_kind == Some(5) => (ResourceKind::Mv, 5),
        _ => return None,
    };
    if declared_kind.is_some_and(|kind| kind != expected_kind)
        || declared_id.is_some_and(|declared| declared != id)
    {
        return None;
    }
    let resource = CommentResource {
        kind,
        id: positive_id(&Value::from(id))?,
        owner: None,
    };
    if resource.thread().ok()?.as_str() != thread {
        return None;
    }
    Some(MessageTarget::Comment {
        resource,
        comment_id: positive_id(comment)?,
    })
}

pub(in crate::client::messages) fn received_comment(value: &Value) -> Option<MessageTarget> {
    let mut fields = value["combindId"].as_str()?.split(':');
    fields.next()?;
    let comment = fields.next()?;
    let thread = fields.next()?;
    if fields.next().is_some() {
        return None;
    }
    let id = positive_id(&payload(&value["resourceJson"])["id"])?;
    thread_target(
        thread,
        &Value::from(comment),
        Some(number(&value["resourceType"])?),
        Some(&id),
    )
}

fn shared_comment(value: &Value) -> Option<MessageTarget> {
    let raw = value
        .get("resourceJson")
        .filter(|value| !value.is_null())
        .or_else(|| value.get("resourceInfo"))?;
    let id = positive_id(&payload(raw)["id"])?;
    thread_target(
        value["threadId"].as_str()?,
        &value["commentId"],
        Some(number(&value["resourceType"])?),
        Some(&id),
    )
}

pub(in crate::client::messages) fn thread(value: &Value) -> Option<MessageTarget> {
    match number(&value["type"])? {
        // Type 14 is a private cloud song; a public song ID is not a valid substitute.
        1 => track(&value["song"]),
        2 => album(&value["album"]),
        3 => artist(&value["artist"]),
        4 => playlist(&value["playlist"]),
        5 => voice(&value["program"]),
        7 => Some(MessageTarget::Mv {
            id: positive_id(&value["mv"]["id"])?,
        }),
        15 => shared_comment(&value["comment"]),
        _ => None,
    }
}

pub(in crate::client::messages) fn forwarded(kind: u64, value: &Value) -> Option<MessageTarget> {
    match kind {
        1 => {
            let appendix = payload(&value["json"]);
            match number(&value["type"])? {
                13 => playlist(&appendix["playlist"]),
                17 => voice(&appendix["program"]),
                18 => track(&appendix["song"]),
                19 => album(&appendix["album"]),
                31 => shared_comment(&appendix["resource"]),
                36 => artist(&appendix["resource"]),
                _ => None,
            }
        }
        3 => {
            let resource = &value["resource"];
            match number(&value["resourceType"])? {
                0 => playlist(resource),
                1 => voice(resource),
                3 => album(resource),
                4 => track(resource),
                31 => shared_comment(resource),
                36 => artist(resource),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(in crate::client::messages) fn notice(value: &Value) -> Option<MessageTarget> {
    match number(&value["type"])? {
        2 | 3 => playlist(&value["playlist"]),
        4 | 5 | 7 => voice(&value["program"]),
        // NoticeItem.jsx:279–281 uses the radio id, never an episode/song id.
        8 => Some(MessageTarget::Podcast {
            id: positive_id(&value["djRadio"]["id"])?,
        }),
        6 => {
            let comment = &value["comment"];
            thread_target(
                comment["threadId"].as_str()?,
                &comment["commentId"],
                None,
                None,
            )
        }
        _ => None,
    }
}
