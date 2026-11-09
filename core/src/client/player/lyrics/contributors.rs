//! M/21 lyricUser and transUser; M/182 navigates by userid, never credit id.
use super::{LyricContributorView, PlaybackSnapshot, PlayerResourceKind, identity};
use crate::{lyrics::LyricSnapshot, protocol::api::identifier};
use serde_json::Value;

pub(super) fn project(
    value: &LyricSnapshot,
    player: &PlaybackSnapshot,
) -> (Option<LyricContributorView>, Option<LyricContributorView>) {
    let Some(song_id) = value
        .resource_key
        .as_deref()
        .and_then(|key| key.strip_prefix("song:"))
    else {
        return (None, None);
    };
    if value.from_sidecar
        || value.from_embedded
        || !player
            .selected
            .as_ref()
            .is_some_and(|entry| ordinary_song(&entry.resource, song_id))
    {
        return (None, None);
    }
    let metadata = &value.lyric.metadata;
    (
        credit(metadata.get("lyricContributor")),
        credit(metadata.get("translationContributor")),
    )
}

pub(super) fn ordinary_song(resource: &Value, song_id: &str) -> bool {
    let ordinary = match identity::kind(resource) {
        PlayerResourceKind::Catalog => true,
        PlayerResourceKind::Downloaded => ["cloud_song_id", "voice_id", "download_voice_id"]
            .into_iter()
            .all(|key| resource.get(key).is_none()),
        _ => false,
    };
    ordinary && identifier(&resource["id"]).is_ok_and(|id| id == song_id)
}

fn credit(value: Option<&Value>) -> Option<LyricContributorView> {
    let value = value?;
    let nickname = value.get("nickname")?.as_str()?;
    if nickname.trim().is_empty()
        || nickname.encode_utf16().take(513).count() > 512
        || nickname.chars().any(char::is_control)
    {
        return None;
    }
    // Cached imported credit data is validated again at the IPC boundary.
    let user_id = value
        .get("userid")
        .filter(|v| v.as_str().is_none_or(|s| s.len() <= 128))
        .and_then(|v| identifier(v).ok());
    Some(LyricContributorView {
        nickname: nickname.into(),
        user_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn public_credit_uses_user_id_and_drops_unrelated_fields() {
        let input = json!({
            "id":"55","userid":"9007199254740993","nickname":"合成 <作者>",
            "url":"https://example.invalid/secret","cookie":"PRIVATE"
        });
        assert_eq!(
            serde_json::to_value(credit(Some(&input))).unwrap(),
            json!({"userId":"9007199254740993","nickname":"合成 <作者>"})
        );
        for invalid in [
            json!("../9"),
            json!("0"),
            json!("9".repeat(129)),
            Value::Null,
        ] {
            let input = json!({"id":"55","userid":invalid,"nickname":"无链接作者"});
            assert_eq!(
                serde_json::to_value(credit(Some(&input))).unwrap(),
                json!({"userId":null,"nickname":"无链接作者"})
            );
        }
        for name in ["  ".into(), "one\ntwo".into(), "声".repeat(513)] {
            assert!(credit(Some(&json!({"userid":"2","nickname":name}))).is_none());
        }
    }

    #[test]
    fn public_credit_source_requires_the_current_ordinary_song_not_an_id_collision() {
        assert!(ordinary_song(&json!({"id":7}), "7"));
        assert!(ordinary_song(
            &json!({"id":7,"download_task_id":"task"}),
            "7"
        ));
        for resource in [
            json!({"id":8}),
            json!({"id":7,"local_id":"indexed"}),
            json!({"id":7,"cloud_song_id":"7"}),
            json!({"id":7,"voice_id":"6"}),
            json!({"id":7,"download_task_id":"task","download_voice_id":"6"}),
            json!({"id":7,"download_task_id":"task","cloud_song_id":"7"}),
        ] {
            assert!(!ordinary_song(&resource, "7"));
        }
    }
}
