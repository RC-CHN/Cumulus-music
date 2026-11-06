//! VoiceTitle.tsx:77–104, DescCard.tsx:64–68; M21 E:429–478 and W:931–1010.
//! No additional read is performed for a playback header. Online Episode
//! resources and download-origin evidence must agree with the selected song.
use super::{PlaybackSnapshot, PlayerResourceKind, identity};
use crate::protocol::{api::identifier, wire::serialization::js_truthy};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlayerVoiceCreator {
    /// Only the official user's userId; never an artist ID, URL or guessed ID.
    pub id: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerVoiceDetail {
    pub id: String,
    pub creator: Option<PlayerVoiceCreator>,
    /// Original episode createTime, positive integer milliseconds in JS Date's
    /// valid range. No file mtime, album date, publishTime or current-day fallback.
    pub release_time: Option<u64>,
    /// Plain episode text from M21 W:945. No alternate fields or markup processing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn id(value: &Value) -> Option<String> {
    identifier(value).ok().filter(|id| id.len() <= 128)
}

fn creator(voice: &Value) -> Option<PlayerVoiceCreator> {
    // M21 W:1003 chooses dj || creator; E:470 preserves this name precedence.
    // Episode::normalized() preserves userId and adds dj.id, but a raw id alone
    // cannot prove that a user profile link came from the official normalizer.
    let value = if js_truthy(&voice["dj"]) {
        &voice["dj"]
    } else {
        &voice["creator"]
    };
    let name = ["nickname", "nickName", "userName"]
        .into_iter()
        .map(|key| &value[key])
        .find(|v| js_truthy(v))?
        .as_str()?;
    if name.trim().is_empty()
        || name.encode_utf16().count() > 512
        || name.chars().any(char::is_control)
    {
        return None;
    }
    Some(PlayerVoiceCreator {
        id: id(&value["userId"]),
        name: name.into(),
    })
}

const MAX_DESCRIPTION_UTF16: usize = 8192;

fn description(voice: &Value) -> Option<String> {
    let value = voice["description"].as_str()?;
    // This is a local IPC bound. Keep valid paragraph whitespace unchanged;
    // the renderer receives plain text, never a rich-content instruction.
    if value.len() > MAX_DESCRIPTION_UTF16 * 3
        || value.trim().is_empty()
        || value.encode_utf16().count() > MAX_DESCRIPTION_UTF16
        || value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return None;
    }
    Some(value.into())
}

fn from_resource(play_id: Option<&str>, resource: &Value) -> Option<PlayerVoiceDetail> {
    play_id.filter(|id| !id.is_empty())?;
    // A CUE/local/cloud/song with an embedded `voice` object is not an episode.
    let episode = match identity::kind(resource) {
        PlayerResourceKind::Voice => id(&resource["voice_id"]),
        PlayerResourceKind::Downloaded
            if resource["local_id"].is_null() && resource["cloud_song_id"].is_null() =>
        {
            let task = resource["download_task_id"].as_str()?;
            if task.len() != 32 || !task.bytes().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            // Created only by download_resources from typed DownloadOrigin::Voice.
            let episode = id(&resource["download_voice_id"])?;
            (id(&resource["song"]["voice_id"])? == episode).then_some(episode)
        }
        _ => None,
    }?;
    let voice = resource.get("voice").filter(|v| v.is_object())?;
    if id(&voice["id"])? != episode {
        return None;
    }
    if !voice["programId"].is_null() && id(&voice["programId"])? != episode {
        return None;
    }
    let song = id(&resource["id"])?;
    // M21 W normalizes mainTrackId and track.id independently. Reject conflicting
    // aliases rather than allowing an unrelated nested voice to supply a header.
    let mut has_song = false;
    for candidate in [
        &voice["mainTrackId"],
        &voice["track"]["id"],
        &voice["mainSong"]["id"],
    ] {
        if !candidate.is_null() {
            if id(candidate)? != song {
                return None;
            }
            has_song = true;
        }
    }
    if !has_song
        || (!resource["song"]["id"].is_null() && id(&resource["song"]["id"])? != song)
        || (!resource["song"]["voice_id"].is_null()
            && id(&resource["song"]["voice_id"])? != episode)
    {
        return None;
    }
    Some(PlayerVoiceDetail {
        id: episode,
        creator: creator(voice),
        release_time: voice["createTime"]
            .as_u64()
            .filter(|value| (1..=8_640_000_000_000_000).contains(value)),
        description: description(voice),
    })
}

pub(super) fn project(snapshot: &PlaybackSnapshot) -> Option<PlayerVoiceDetail> {
    from_resource(
        snapshot.play_id.as_deref(),
        &snapshot.selected.as_ref()?.resource,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::podcast::Episode;
    use serde_json::json;

    const DESCRIPTION: &str = "  合成片头 \r\n\r\n\t第二段 🎧 <字面文字>\n";

    fn episode() -> Episode {
        Episode::from_official(&json!({
            "id":71,"name":"合成声音","mainTrackId":901,
            "mainSong":{"id":901,"duration":200000},"createTime":1735787045000_u64,
            "dj":{"userId":42,"nickname":"合成主播","avatarUrl":"PRIVATE_AVATAR","cookie":"PRIVATE_COOKIE"},
            "description":DESCRIPTION,
            "authorization":"PRIVATE_TOKEN","url":"https://private.invalid/token"
        })).unwrap()
    }

    fn online() -> Value {
        let episode = episode();
        let mut resource = episode.playback_resource().unwrap();
        resource["song"] = episode.download_metadata().unwrap();
        resource
    }

    fn download() -> Value {
        let mut resource = online();
        resource.as_object_mut().unwrap().remove("voice_id");
        resource["download_task_id"] = json!("0123456789abcdef0123456789abcdef");
        resource["download_voice_id"] = json!("71");
        resource["path"] = json!("PRIVATE_LOCAL_PATH");
        resource
    }

    #[test]
    fn official_episode_metadata_is_bound_and_whitelisted_for_online_and_downloads() {
        let expected = json!({"id":"71","creator":{"id":"42","name":"合成主播"},"releaseTime":1735787045000_u64,
            "description":DESCRIPTION});
        for resource in [online(), download()] {
            let result = from_resource(Some("native-play"), &resource).unwrap();
            assert_eq!(serde_json::to_value(&result).unwrap(), expected);
            let encoded = serde_json::to_string(&result).unwrap();
            assert!(!encoded.contains("PRIVATE"));
            assert!(!encoded.contains("download_voice_id"));
            assert!(!encoded.contains("https:"));
        }
    }

    #[test]
    fn mismatched_or_missing_episode_song_play_and_download_authority_is_never_projected() {
        assert!(from_resource(None, &online()).is_none());
        assert!(from_resource(Some(""), &online()).is_none());
        for (path, value) in [
            ("/voice/id", json!(72)),
            ("/voice/programId", json!(72)),
            ("/voice/mainTrackId", json!(902)),
            ("/voice/track/id", json!(902)),
            ("/voice/mainSong/id", json!(902)),
            ("/song/id", json!(902)),
            ("/song/voice_id", json!(72)),
            ("/voice_id", Value::Null),
        ] {
            let mut resource = online();
            *resource.pointer_mut(path).unwrap() = value;
            assert!(from_resource(Some("play"), &resource).is_none(), "{path}");
        }
        for (path, value) in [
            ("/download_voice_id", Value::Null),
            ("/download_voice_id", json!(72)),
            ("/download_task_id", json!("../foreign")),
            ("/song/voice_id", Value::Null),
        ] {
            let mut resource = download();
            *resource.pointer_mut(path).unwrap() = value;
            assert!(from_resource(Some("play"), &resource).is_none(), "{path}");
        }
    }

    #[test]
    fn local_cue_cloud_and_catalog_never_borrow_unbound_voice_metadata() {
        let mut song = online();
        song.as_object_mut().unwrap().remove("voice_id");
        let mut cloud = online();
        cloud["cloud_song_id"] = json!(901);
        let mut cue = online();
        cue["local_id"] = json!("cue-1");
        cue["metadata"] = json!({"title":"CUE title","cue":{"index":1,"start":20}});
        let mut foreign_download = download();
        foreign_download["cloud_song_id"] = json!(901);
        for resource in [song, cloud, cue, foreign_download] {
            assert!(from_resource(Some("play"), &resource).is_none());
        }
    }

    #[test]
    fn description_is_only_the_bounded_plain_text_on_the_bound_episode() {
        let mut resource = online();
        resource["description"] = json!("外层描述");
        resource["voice"]["programDesc"] = json!(["列表简介"]);
        resource["voice"]["desc"] = json!("另一个字段");
        resource["voice"]["radio"] = json!({"description":"播客描述"});
        for invalid in [
            Value::Null,
            json!(false),
            json!(["正文"]),
            json!({"text":"正文"}),
            json!(" \r\n\t "),
            json!("正文\0"),
            json!("正文\u{007f}"),
            json!("🎧".repeat(MAX_DESCRIPTION_UTF16 / 2) + " "),
        ] {
            resource["voice"]["description"] = invalid;
            let detail = from_resource(Some("native-play"), &resource).unwrap();
            assert!(detail.description.is_none());
            assert!(
                serde_json::to_value(detail)
                    .unwrap()
                    .get("description")
                    .is_none()
            );
        }
        resource["voice"]
            .as_object_mut()
            .unwrap()
            .remove("description");
        assert!(
            from_resource(Some("native-play"), &resource)
                .unwrap()
                .description
                .is_none()
        );

        let max = "🎧".repeat(MAX_DESCRIPTION_UTF16 / 2);
        resource["voice"]["description"] = json!(max);
        assert_eq!(
            from_resource(Some("native-play"), &resource)
                .unwrap()
                .description,
            Some(max)
        );
    }

    #[test]
    fn creator_name_can_be_plain_text_but_link_comes_only_from_official_user_identity() {
        for raw in [
            json!({"nickname":"主播无链接"}),
            json!({"nickname":"主播无链接","id":42,"artistId":99}),
            json!({"nickname":"主播无链接","userId":0}),
            json!({"nickname":"主播无链接","userId":"https://private.invalid"}),
        ] {
            let mut resource = online();
            resource["voice"]["dj"] = raw;
            assert_eq!(
                serde_json::to_value(from_resource(Some("play"), &resource).unwrap().creator)
                    .unwrap(),
                json!({"id":null,"name":"主播无链接"})
            );
        }
        let mut resource = online();
        resource["voice"]["dj"] =
            json!({"nickname":"","nickName":"次级名称","userName":"另一个名称","userId":42});
        assert_eq!(
            from_resource(Some("play"), &resource)
                .unwrap()
                .creator
                .unwrap()
                .name,
            "次级名称"
        );
        resource["voice"]["dj"] = Value::Null;
        resource["voice"]["creator"] = json!({"userId":43,"userName":"创建者名称"});
        assert_eq!(
            from_resource(Some("play"), &resource)
                .unwrap()
                .creator
                .unwrap()
                .id
                .as_deref(),
            Some("43")
        );
    }

    #[test]
    fn release_time_never_uses_current_day_file_album_or_other_timestamp_fields() {
        for value in [
            Value::Null,
            json!(0),
            json!(-1),
            json!(1.5),
            json!("1735787045000"),
            json!(8_640_000_000_000_001_u64),
        ] {
            let mut resource = online();
            resource["voice"]["createTime"] = value;
            resource["voice"]["publishTime"] = json!(1735787045000_u64);
            resource["voice"]["scheduledPublishTime"] = json!(1735787045000_u64);
            resource["mtime"] = json!(1735787045000_u64);
            assert_eq!(
                from_resource(Some("play"), &resource).unwrap().release_time,
                None
            );
        }
    }
}
