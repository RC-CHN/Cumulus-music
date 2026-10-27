//! M/75 consumes JSON.parse(msg_attach).content, not the SDK chatroom entry
//! notification. Official nim_wrapper.dll forwards IMMessage.attach_ through
//! NEIMController vtable+0x60 to im.onChatRoomMsg. See the native evidence map.
use crate::{Error, Result};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    Play,
    Playlist,
    MemberJoined,
    End,
    VipGift,
    DigitalGift,
    VipChanged,
    Heartbeat,
    Distance,
    Statistics,
    DigitalChanged,
    PrivilegeChanged,
    Emoticon,
}
impl MessageKind {
    pub fn code(self) -> u32 {
        match self {
            Self::Play => 20000,
            Self::Playlist => 20001,
            Self::MemberJoined => 20002,
            Self::End => 20003,
            Self::VipGift => 20004,
            Self::DigitalGift => 20005,
            Self::VipChanged => 20006,
            Self::Heartbeat => 20008,
            Self::Distance => 20009,
            Self::Statistics => 20010,
            Self::DigitalChanged => 20011,
            Self::PrivilegeChanged => 20012,
            Self::Emoticon => 20013,
        }
    }
}

/// Native-only message. Identity-bearing content is never a frontend DTO.
#[derive(Clone)]
pub struct RoomMessage {
    pub kind: MessageKind,
    pub content: Value,
}
impl fmt::Debug for RoomMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoomMessage")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl RoomMessage {
    /// Exact recovered envelope, deliberately no recursive search for a type
    /// number in arbitrary messages or fallback to normal chat text.
    pub fn from_attachment(attachment: &str) -> Result<Option<Self>> {
        if attachment.len() > 1024 * 1024 {
            return Err(Error::Response("together attachment exceeds limit"));
        }
        let value: Value = serde_json::from_str(attachment)
            .map_err(|_| Error::Response("invalid together attachment JSON"))?;
        let envelope = &value["content"];
        let kind = match envelope["type"].as_u64() {
            Some(20000) => MessageKind::Play,
            Some(20001) => MessageKind::Playlist,
            Some(20002) => MessageKind::MemberJoined,
            Some(20003) => MessageKind::End,
            Some(20004) => MessageKind::VipGift,
            Some(20005) => MessageKind::DigitalGift,
            Some(20006) => MessageKind::VipChanged,
            Some(20008) => MessageKind::Heartbeat,
            Some(20009) => MessageKind::Distance,
            Some(20010) => MessageKind::Statistics,
            Some(20011) => MessageKind::DigitalChanged,
            Some(20012) => MessageKind::PrivilegeChanged,
            Some(20013) => MessageKind::Emoticon,
            _ => return Ok(None),
        };
        let content = envelope["content"].clone();
        if !content.is_object() {
            return Err(Error::Response("invalid together message content"));
        }
        Ok(Some(Self { kind, content }))
    }
    /// A NIM account may have queued messages from an older music room. Do
    /// not infer room identity from the SDK's current chatroom or song IDs.
    pub fn belongs_to(&self, room: &str) -> bool {
        self.content["roomId"].as_str() == Some(room)
    }
    /// M/75 uses Array.includes(Number(hostUid)): a string ID in the array
    /// does not exclude the numeric account. Missing/null lists do not ask for
    /// a report. Malformed non-array values are rejected at this boundary.
    pub fn requests_progress_for(&self, owner: &str) -> Result<bool> {
        if self.kind != MessageKind::Heartbeat || self.content["ignoreUserIds"].is_null() {
            return Ok(false);
        }
        let ignored = self.content["ignoreUserIds"]
            .as_array()
            .ok_or(Error::Response("invalid together progress ignore list"))?;
        let owner = owner
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or(Error::State("invalid together progress owner"))?;
        Ok(!ignored.iter().any(|value| value.as_f64() == Some(owner)))
    }
}

/// SDK single/batch receive envelopes (nim_talk_helper::ParseReceiveMessage).
/// Strip ordinary conversations and all unrelated metadata at the native edge.
pub(super) fn from_sdk(value: &Value) -> Vec<RoomMessage> {
    fn one(value: &Value) -> Option<RoomMessage> {
        let message = if value.get("content").is_some() {
            if value["rescode"].as_i64() != Some(200) {
                return None;
            }
            &value["content"]
        } else {
            value
        };
        RoomMessage::from_attachment(message["msg_attach"].as_str()?)
            .ok()
            .flatten()
    }
    match value {
        Value::Array(rows) => rows.iter().filter_map(one).collect(),
        value => one(value).into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recovered_envelope_codes_are_exact_and_not_recursive_or_text_fallbacks() {
        for code in [
            20000, 20001, 20002, 20003, 20004, 20005, 20006, 20008, 20009, 20010, 20011, 20012,
            20013,
        ] {
            let input =
                json!({"content":{"type":code,"content":{"roomId":"room-private","userId":123}}});
            let message = RoomMessage::from_attachment(&input.to_string())
                .unwrap()
                .unwrap();
            assert_eq!(message.kind.code(), code);
            assert!(message.belongs_to("room-private"));
            assert!(!message.belongs_to("other-room"));
            assert!(!format!("{message:?}").contains("private"));
        }
        for input in [
            json!({"type":20000,"content":{}}),
            json!({"content":{"type":"20000","content":{}}}),
            json!({"content":{"type":20007,"content":{}}}),
            json!({"content":{"type":4294987296_u64,"content":{}}}),
        ] {
            assert!(
                RoomMessage::from_attachment(&input.to_string())
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            RoomMessage::from_attachment(r#"{"content":{"type":20000,"content":null}}"#).is_err()
        );
        assert!(RoomMessage::from_attachment("{").is_err());
    }

    #[test]
    fn sdk_single_raw_and_batch_envelopes_drop_ordinary_messages_and_failed_results() {
        let attachment =
            json!({"content":{"type":20008,"content":{"roomId":"r","ignoreUserIds":[1]}}})
                .to_string();
        let message = json!({"msg_attach":attachment,"msg_body":"PRIVATE CHAT","from_accid":"PRIVATE SENDER"});
        let wrapped = json!({"rescode":200,"feature":1,"content":message});
        let failed = json!({"rescode":500,"feature":1,"content":message});
        let ordinary = json!({"msg_body":attachment,"msg_attach":""});
        let messages = from_sdk(&json!([message, wrapped, failed, ordinary]));
        assert_eq!(messages.len(), 2);
        assert!(
            messages
                .iter()
                .all(|m| m.kind == MessageKind::Heartbeat && m.belongs_to("r"))
        );
        assert!(!format!("{messages:?}").contains("PRIVATE"));
        let no_room = RoomMessage::from_attachment(r#"{"content":{"type":20000,"content":{}}}"#)
            .unwrap()
            .unwrap();
        assert!(!no_room.belongs_to("r"));
    }
}
