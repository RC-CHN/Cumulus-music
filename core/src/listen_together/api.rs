use super::{ListCommand, PlayCommand, Room, TogetherSnapshot, model::opaque};
use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::{fmt, sync::Arc};

#[derive(Clone)]
pub struct TogetherApi {
    api: Arc<dyn Api>,
}
#[derive(Clone, Debug)]
pub struct CreatedRoom {
    pub room: Room,
    pub is_new: bool,
}
/// Native-only credentials: intentionally no Serialize or derived Debug.
pub struct ImCredentials {
    pub uid: String,
    pub account_id: String,
    pub token: String,
}
pub struct RtcCredentials {
    pub token: String,
    pub expires_at: u64,
}
impl fmt::Debug for ImCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImCredentials([redacted])")
    }
}
impl fmt::Debug for RtcCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RtcCredentials([redacted])")
    }
}
impl TogetherApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    async fn request(&self, path: &str, payload: Value) -> Result<Value> {
        let r = self.api.call(path, payload, false, &[200]).await?;
        if !r["data"].is_object() {
            return Err(Error::Response("together response data missing"));
        }
        Ok(r["data"].clone())
    }
    async fn result(&self, path: &str, payload: Value, field: &str) -> Result<()> {
        let data = self.request(path, payload).await?;
        match data[field].as_bool() {
            Some(true) => Ok(()),
            Some(false) => Err(Error::service(path, json!(200), json!({"data":data}))),
            None => Err(Error::Response("together acknowledgement missing")),
        }
    }
    pub async fn status(&self) -> Result<Option<Room>> {
        let data = self
            .request("/api/listen/together/status/get", json!({}))
            .await?;
        match data["inRoom"].as_bool() {
            Some(false) => Ok(None),
            Some(true) => Room::from_official(&data["roomInfo"]).map(Some),
            None => Err(Error::Response("together room status missing")),
        }
    }
    pub async fn create(&self, refer: &str) -> Result<CreatedRoom> {
        opaque(refer)?;
        let data = self
            .request("/api/listen/together/room/create", json!({"refer":refer}))
            .await?;
        let is_new = match data["type"].as_str() {
            Some("NEW_ROOM") => true,
            Some("ALREADY_IN_ROOM") => false,
            _ => return Err(Error::Response("unknown room creation outcome")),
        };
        Ok(CreatedRoom {
            room: Room::from_official(&data["roomInfo"])?,
            is_new,
        })
    }
    pub async fn check(&self, room: &str) -> Result<Value> {
        self.request(
            "/api/listen/together/room/check",
            json!({"roomId":opaque(room)?}),
        )
        .await
    }
    pub async fn accept(&self, room: &str, inviter: &str, refer: &str) -> Result<Room> {
        let data=self.request("/api/listen/together/play/invitation/accept",json!({"refer":opaque(refer)?,"roomId":opaque(room)?,"inviterId":identifier(&json!(inviter))?})).await?;
        let found = Room::from_official(&data["roomInfo"])?;
        if found.room_id != room {
            return Err(Error::Response("accepted a different together room"));
        }
        Ok(found)
    }
    pub async fn end(&self, room: &str, count: Option<u64>) -> Result<()> {
        let mut payload = json!({"roomId":opaque(room)?});
        if let Some(count) = count {
            payload["shareInfo"] = json!(serde_json::to_string(&json!({"thisListenCount":count}))?);
        }
        self.result("/api/listen/together/end/v2", payload, "success")
            .await
    }
    pub async fn snapshot(&self, room: &str) -> Result<TogetherSnapshot> {
        TogetherSnapshot::from_official(
            &self
                .request(
                    "/api/listen/together/sync/playlist/get",
                    json!({"roomId":opaque(room)?}),
                )
                .await?,
        )
    }
    pub async fn list_command(&self, room: &str, command: &ListCommand) -> Result<()> {
        self.result("/api/listen/together/sync/list/command/report",json!({"roomId":opaque(room)?,"playlistParam":serde_json::to_string(&command.wire()?)?}),"result").await
    }
    pub async fn play_command(&self, room: &str, command: &PlayCommand) -> Result<()> {
        self.result(
            "/api/listen/together/play/command/report",
            json!({"roomId":opaque(room)?,"commandInfo":serde_json::to_string(&command.wire()?)?}),
            "result",
        )
        .await
    }
    pub async fn heartbeat(&self, room: &str, command: &PlayCommand) -> Result<()> {
        command.validate()?;
        self.result("/api/listen/together/heartbeat",json!({"roomId":opaque(room)?,"songId":command.target_song_id,"playStatus":command.play_status,"progress":command.progress.max(1)}),"result").await
    }
    pub async fn sync_notice(&self, room: &str) -> Result<()> {
        self.result(
            "/api/listen/together/sync/notice",
            json!({"roomId":opaque(room)?}),
            "result",
        )
        .await
    }
    pub async fn privilege_changed(&self, room: &str) -> Result<Value> {
        self.request(
            "/api/listen/together/privilege/change/report",
            json!({"roomId":opaque(room)?}),
        )
        .await
    }
    pub async fn privileges(&self, other: &str, songs: &[String]) -> Result<Vec<Value>> {
        super::model::ids(&json!(songs))?;
        let data = self
            .request(
                "/api/listen/together/privilege/get",
                json!({"otherUserId":identifier(&json!(other))?,"songIds":songs}),
            )
            .await?;
        let rows = data["result"]
            .as_array()
            .ok_or(Error::Response("together privileges missing"))?;
        rows.iter()
            .map(|v| {
                let mut v = v.clone();
                let song = super::model::id(&v["songId"])?;
                if !songs.contains(&song) {
                    return Err(Error::Response("unexpected together privilege song"));
                }
                v["trackId"] = json!(song);
                v.as_object_mut()
                    .ok_or(Error::Response("invalid privilege entry"))?
                    .remove("songId");
                for field in ["unplayableUserIds", "unplayableOuterIds"] {
                    v[field] = json!(
                        v[field]
                            .as_array()
                            .ok_or(Error::Response("invalid privilege identity list"))?
                            .iter()
                            .map(|id| match id {
                                Value::String(s) => Ok(s.clone()),
                                Value::Number(n) => Ok(n.to_string()),
                                _ => Err(Error::Response("invalid privilege identity")),
                            })
                            .collect::<Result<Vec<_>>>()?
                    );
                }
                Ok(v)
            })
            .collect()
    }
    pub async fn statistics(&self, room: &str, users: &[String]) -> Result<Value> {
        for id in users {
            identifier(&json!(id))?;
        }
        self.request(
            "/api/listen/together/relation/statistics/get/v2",
            json!({"roomId":opaque(room)?,"roomUserIds":serde_json::to_string(users)?}),
        )
        .await
    }
    pub async fn im_credentials(&self) -> Result<ImCredentials> {
        let data = self.request("/api/middle/im/token/get", json!({})).await?;
        Ok(ImCredentials {
            uid: super::model::id(&data["uid"])?,
            account_id: opaque(
                data["accId"]
                    .as_str()
                    .ok_or(Error::Response("IM account missing"))?,
            )?
            .into(),
            token: secret(&data["token"])?,
        })
    }
    pub async fn rtc_credentials(&self, room: &Room) -> Result<RtcCredentials> {
        let data = self
            .request(
                "/api/listen/together/yunxin/token/get",
                json!({"roomId":opaque(&room.room_id)?,"channelId":opaque(&room.channel_id)?}),
            )
            .await?;
        Ok(RtcCredentials {
            token: secret(&data["token"])?,
            expires_at: data["expireTime"]
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or(Error::Response("RTC expiration missing"))?,
        })
    }
}
fn secret(v: &Value) -> Result<String> {
    let s = v
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 8192 && !s.chars().any(char::is_control))
        .ok_or(Error::Response("together credential missing"))?;
    Ok(s.into())
}
