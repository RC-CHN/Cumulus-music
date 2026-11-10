use super::dto::{
    ArtistReference, PlaylistHeaderDate, PlaylistHeaderDateKind, PlaylistView, TrackView,
};
use crate::{Error, Result, catalog::metadata::SongTitleNames, protocol::api::identifier};
use serde_json::Value;

pub(super) use crate::protocol::artwork::square as artwork;
pub(super) fn string(value: &Value) -> String {
    value.as_str().unwrap_or_default().into()
}

pub(super) fn playlist(value: &Value, uid: &str) -> Result<PlaylistView> {
    let owner =
        identifier(&value["userId"]).or_else(|_| identifier(&value["creator"]["userId"]))?;
    let owned = owner == uid;
    let privacy = value["privacy"].as_i64();
    let special_type = value["specialType"].as_i64();
    let (date_key, kind) = if special_type == Some(10) {
        ("updateTime", PlaylistHeaderDateKind::Updated)
    } else {
        ("createTime", PlaylistHeaderDateKind::Created)
    };
    let header_date = value[date_key]
        .as_u64()
        .filter(|time| (1..=8_640_000_000_000_000).contains(time))
        .map(|timestamp_ms| PlaylistHeaderDate { kind, timestamp_ms });
    Ok(PlaylistView {
        id: identifier(&value["id"])?,
        name: string(&value["name"]),
        cover_url: artwork(&value["coverImgUrl"]),
        description: string(&value["description"]),
        creator: string(&value["creator"]["nickname"]),
        creator_id: identifier(&value["creator"]["userId"])
            .ok()
            .filter(|id| id == &owner),
        creator_avatar_url: artwork(&value["creator"]["avatarUrl"]),
        header_date,
        tags: super::playlist_tags_browse::header_tags(&value["tags"]),
        track_count: value["trackCount"].as_u64().unwrap_or(0) as usize,
        subscribed_count: value["subscribedCount"]
            .as_u64()
            .filter(|n| *n <= 9_007_199_254_740_991),
        play_count: value["playCount"]
            .as_u64()
            .filter(|count| privacy == Some(0) && (1..=9_007_199_254_740_991).contains(count)),
        owned,
        favorite: owned && special_type == Some(5),
        private: privacy == Some(10),
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
        title_names: SongTitleNames::from_song(value),
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
        album_id: crate::catalog::album_identifier(album).ok(),
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

pub(super) fn online_play_all_eligible(row: &Value) -> bool {
    // Official M21 maps pl to maxPlayBr, then M23.Z accepts positive pl
    // or trial flag bit 10. Never treat fee or a download entitlement as pl.
    // Absent privilege is left to the normal authoritative source resolver.
    !row["privilege"].is_object()
        || row["privilege"]["pl"].as_i64().is_some_and(|pl| pl > 0)
        || row["privilege"]["flag"]
            .as_i64()
            .is_some_and(|flag| flag & (1 << 10) != 0)
}

/// Whitelist catalog metadata for native queues. Signed media links stay out;
/// bounded title arrays stay native until the selected-player projection.
pub(super) fn song_metadata(value: &Value) -> Result<Value> {
    song_metadata_with_privilege(value, None)
}

pub(super) fn song_metadata_with_privilege(
    value: &Value,
    privilege: Option<&Value>,
) -> Result<Value> {
    let view = track(value)?;
    let mut song = projected_song_metadata(&view);
    if super::together::external::catalog_unsupported(value, privilege) {
        song[super::together::external::BLOCKED] = Value::Bool(true);
    }
    Ok(song)
}

pub(super) fn projected_song_metadata(view: &TrackView) -> Value {
    let mut song = serde_json::json!({"id":view.id,"name":view.name,
        "ar":view.artist_refs.iter().map(|artist|serde_json::json!({"id":artist.id,"name":artist.name})).collect::<Vec<_>>(),
        "al":{"id":view.album_id,"name":view.album,"picUrl":view.cover_url},"dt":view.duration_ms,"fee":view.fee});
    view.title_names.write_queue_fields(&mut song);
    song
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
        let legacy =
            track(&json!({"id":2,"album":{"id":0,"albumId":"90071992547409931234","name":"专辑"}}))
                .unwrap();
        assert_eq!(legacy.album_id.as_deref(), Some("90071992547409931234"));
        assert!(track(&json!({"id": 2})).unwrap().artist_refs.is_empty());
    }
}
