use super::dto::{ArtistReference, PlaylistView, TrackView};
use crate::{Error, Result, protocol::api::identifier};
use serde_json::Value;

/// Only public image CDNs. Never pass arbitrary service-controlled URLs to a
/// WebView; artwork requests carry no account credentials or signed queries.
pub(super) fn artwork(value: &Value) -> Option<String> {
    let mut url = url::Url::parse(value.as_str()?).ok()?;
    let host = url.host_str()?;
    if !["music.126.net", "music.163.com"]
        .iter()
        .any(|suffix| host.ends_with(&format!(".{suffix}")))
        || !["http", "https"].contains(&url.scheme())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    url.set_scheme("https").ok()?;
    url.set_query(Some("param=320y320"));
    url.set_fragment(None);
    Some(url.into())
}
pub(super) fn string(value: &Value) -> String {
    value.as_str().unwrap_or_default().into()
}

pub(super) fn playlist(value: &Value, uid: &str) -> Result<PlaylistView> {
    let owner =
        identifier(&value["userId"]).or_else(|_| identifier(&value["creator"]["userId"]))?;
    let owned = owner == uid;
    Ok(PlaylistView {
        id: identifier(&value["id"])?,
        name: string(&value["name"]),
        cover_url: artwork(&value["coverImgUrl"]),
        description: string(&value["description"]),
        creator: string(&value["creator"]["nickname"]),
        track_count: value["trackCount"].as_u64().unwrap_or(0) as usize,
        owned,
        favorite: owned && value["specialType"].as_i64() == Some(5),
        private: value["privacy"].as_i64() == Some(10),
    })
}

pub(super) fn track(value: &Value) -> Result<TrackView> {
    let artists = value
        .get("ar")
        .or_else(|| value.get("artists"))
        .and_then(Value::as_array);
    let album = value
        .get("al")
        .or_else(|| value.get("album"))
        .unwrap_or(&Value::Null);
    Ok(TrackView {
        id: identifier(&value["id"])?,
        voice_id: value
            .get("voice_id")
            .filter(|v| !v.is_null())
            .map(identifier)
            .transpose()?,
        name: string(&value["name"]),
        artists: artists
            .into_iter()
            .flatten()
            .map(|v| string(&v["name"]))
            .collect(),
        artist_refs: artists
            .into_iter()
            .flatten()
            .map(|v| ArtistReference {
                id: identifier(&v["id"]).ok(),
                name: string(&v["name"]),
            })
            .collect(),
        album: string(&album["name"]),
        album_id: identifier(&album["id"]).ok(),
        cover_url: artwork(&album["picUrl"]),
        duration_ms: value
            .get("dt")
            .or_else(|| value.get("duration"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        fee: value["fee"].as_i64().unwrap_or(0),
        missing: false,
        local: false,
    })
}

pub(super) fn page(offset: usize, limit: usize) -> Result<()> {
    if !(1..=100).contains(&limit) || offset > 1_000_000 {
        Err(Error::InvalidInput("invalid page bounds"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::track;
    use serde_json::json;

    #[test]
    fn catalog_references_keep_credit_order_and_only_valid_decimal_ids() {
        let value = track(&json!({
            "id": 1,
            "ar": [
                {"id": "900719925474099312345", "name": "第一位", "cookie": "DO_NOT_EXPORT"},
                {"id": 0, "name": "未关联歌手"},
                {"id": "javascript:secret", "name": "第三位"},
                {"name": "第四位"}
            ],
            "al": {"id": "0021", "name": "专辑", "url": "DO_NOT_EXPORT"}
        }))
        .unwrap();
        assert_eq!(value.artists, ["第一位", "未关联歌手", "第三位", "第四位"]);
        assert_eq!(
            value
                .artist_refs
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            value.artists
        );
        assert_eq!(
            value.artist_refs[0].id.as_deref(),
            Some("900719925474099312345")
        );
        assert!(value.artist_refs[1..].iter().all(|a| a.id.is_none()));
        assert_eq!(value.album_id.as_deref(), Some("0021"));
        let serialized = serde_json::to_string(&value).unwrap();
        assert!(!serialized.contains("DO_NOT_EXPORT"));
        assert!(!serialized.contains("javascript:"));
    }

    #[test]
    fn legacy_catalog_references_and_unlinked_albums_remain_readable() {
        let value = track(&json!({
            "id": 1, "name": "旧接口歌曲",
            "artists": [{"id": 12, "name": "歌手"}],
            "album": {"id": 0, "name": "本地专辑"}
        }))
        .unwrap();
        assert_eq!(value.artist_refs[0].id.as_deref(), Some("12"));
        assert_eq!(value.album, "本地专辑");
        assert!(value.album_id.is_none());
        assert!(track(&json!({"id": 2})).unwrap().artist_refs.is_empty());
    }
}
