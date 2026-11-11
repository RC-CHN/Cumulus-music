//! Display-only secondary names for the dedicated Songs/Lyrics table.
//!
//! M352:879,1107 uses M113's base title cell directly or inside M607. This
//! projection handles the inline M113/M134 MarkAlias, not the M607 footer
//! or the M352 LyricExtra. Primary-name marking still has its separate
//! renderer fallback; the suffix reuses the recovered M123.f implementation.

use crate::{
    catalog::search::{self, SearchSuggestionPart},
    client::dto::TrackView,
    protocol::{api::identifier, wire::serialization::js_truthy},
};
use serde::Serialize;
use serde_json::Value;

const MAX_NAMES: usize = 16;
const MAX_NAME_BYTES: usize = 1024;
const MAX_NAME_UNITS: usize = 256;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSongInlineCredits {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    title_suffix: Vec<SearchSuggestionPart>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    artist_suffixes: Vec<Option<Vec<SearchSuggestionPart>>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    album_suffix: Vec<SearchSuggestionPart>,
}

fn safe_name(text: &str) -> bool {
    text.len() <= MAX_NAME_BYTES
        && text.encode_utf16().nth(MAX_NAME_UNITS).is_none()
        && !text.contains("://")
        && !text.chars().any(char::is_control)
}

fn names(value: &Value) -> Option<Vec<&str>> {
    let rows = value.as_array()?;
    if rows.len() > MAX_NAMES {
        return None;
    }
    rows.iter()
        .map(|row| row.as_str().filter(|text| safe_name(text)))
        .collect()
}

fn first_truthy<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .filter_map(|key| value.get(key))
        .find(|value| js_truthy(value))
}

fn suffix(names: &[&str], keyword: &str) -> Vec<SearchSuggestionPart> {
    let mut parts = search::song_table_alias_parts(names, keyword);
    if !parts.is_empty() {
        let bracket = |text: &str| SearchSuggestionPart {
            text: text.into(),
            marked: false,
        };
        parts.insert(0, bracket("（"));
        parts.push(bracket("）"));
    }
    parts
}

fn artist_names(artist: &Value) -> Option<Vec<&str>> {
    // M21.N: alias may be a semicolon-delimited string. Only raw trans
    // (not transName by itself) causes the normalized translation to be
    // inserted at the front of the alias list used by M87.a.
    let mut result = match artist.get("alias") {
        Some(Value::String(text)) if text.len() <= MAX_NAMES * MAX_NAME_BYTES => {
            let aliases: Vec<_> = text
                .split(';')
                .filter(|s| !s.is_empty())
                .take(MAX_NAMES + 1)
                .collect();
            if aliases.len() > MAX_NAMES || !aliases.iter().all(|text| safe_name(text)) {
                return None;
            }
            aliases
        }
        Some(value) if js_truthy(value) => names(value)?,
        _ => Vec::new(),
    };
    if let Some(value) = artist.get("trans").filter(|value| js_truthy(value)) {
        let text = value.as_str()?.trim();
        if !safe_name(text) {
            return None;
        }
        result.insert(0, text);
    }
    Some(result)
}

fn album_names(album: &Value) -> Option<Vec<&str>> {
    // M21.R: exact first truthy field (including a present empty array),
    // then M87.a concatenates translated names before aliases.
    let mut result = if let Some(value) = first_truthy(album, &["tns", "transNames"]) {
        names(value)?
    } else if let Some(value) = first_truthy(album, &["transName"]) {
        vec![value.as_str().filter(|text| safe_name(text))?]
    } else {
        Vec::new()
    };
    if let Some(value) = first_truthy(album, &["alia", "ala", "alias"]) {
        result.extend(names(value)?);
    }
    Some(result)
}

pub(super) fn project(
    resource: &Value,
    song: &Value,
    track: &TrackView,
    keyword: &str,
) -> Option<SearchSongInlineCredits> {
    // M15.Un/Wn spreads outer resource, then the nested Song. This is per
    // field, not a merge of artist or album objects. Title uses only tns
    // (M21.D:565, M113:245–252); song aliases belong to M607's separate hint.
    let merged = |key: &str| song.get(key).or_else(|| resource.get(key));
    let title_suffix = merged("tns")
        .and_then(names)
        .map(|items| suffix(&items, keyword))
        .unwrap_or_default();

    let artist_suffixes = merged("ar")
        .filter(|value| js_truthy(value))
        .or_else(|| merged("artists"))
        .and_then(Value::as_array)
        .filter(|artists| artists.len() == track.artist_refs.len() && artists.len() <= MAX_NAMES)
        .map(|artists| {
            artists
                .iter()
                .zip(&track.artist_refs)
                .map(|(artist, displayed)| {
                    // M113 only attaches the alias to the linked credit.
                    // Never apply metadata from a different normalized row.
                    let id = displayed.id.as_deref()?;
                    if displayed.name.is_empty()
                        || artist["name"].as_str() != Some(displayed.name.as_str())
                        || identifier(&artist["id"]).ok().as_deref() != Some(id)
                    {
                        return None;
                    }
                    let parts = suffix(&artist_names(artist)?, keyword);
                    (!parts.is_empty()).then_some(parts)
                })
                .collect::<Vec<_>>()
        })
        .filter(|slots| slots.iter().any(Option::is_some))
        .unwrap_or_default();

    let album_suffix = merged("album")
        .filter(|value| !value.is_null())
        .or_else(|| merged("al"))
        .filter(|album| {
            // M21.D prefers album over al. Current TrackView may resolve
            // differently when both exist; do not add a different album's
            // metadata to the displayed name/link. M134 also decorates a
            // genuinely ID-less named album, without making it clickable.
            let id = match first_truthy(album, &["id", "albumId"]) {
                Some(raw) => identifier(raw)
                    .ok()
                    .is_some_and(|id| Some(id.as_str()) == track.album_id.as_deref()),
                None => track.album_id.is_none(),
            };
            id && !track.album.is_empty()
                && first_truthy(album, &["name", "albumName"]).and_then(Value::as_str)
                    == Some(track.album.as_str())
        })
        .and_then(album_names)
        .map(|items| suffix(&items, keyword))
        .unwrap_or_default();

    (!title_suffix.is_empty() || !artist_suffixes.is_empty() || !album_suffix.is_empty()).then_some(
        SearchSongInlineCredits {
            title_suffix,
            artist_suffixes,
            album_suffix,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{super::results, *};
    use crate::catalog::search::results::{SearchCategory, SearchResultQuery};
    use serde_json::json;

    fn query(category: SearchCategory) -> SearchResultQuery {
        SearchResultQuery {
            keyword: "夜".into(),
            category,
            offset: 0,
            limit: 10,
            correct: true,
            channel: None,
        }
    }

    fn wrapped(hit: Value) -> Value {
        json!({"data":{"resources":[hit],"totalCount":1,"hasMore":false}})
    }

    fn row(raw: Value, category: SearchCategory, uid: &str) -> Value {
        let (view, _, _) = results::project(raw, query(category), uid, false).unwrap();
        serde_json::to_value(&view.sections[0].items[0]).unwrap()
    }

    fn marked(rest: &str) -> Value {
        json!([
            {"text":"（","match":false},
            {"text":"夜","match":true},
            {"text":rest,"match":false},
            {"text":"）","match":false}
        ])
    }

    #[test]
    fn dedicated_lyrics_and_songs_share_inline_names_but_keep_distinct_lyric_content() {
        let song = json!({
            "id":21,
            "name":"合成标题",
            "tns":["首译","夜幕译名"],
            "alia":["夜歌曲别名"],
            "ar":[
                {"id":22,"name":"首位艺人","trans":" 夜译艺人 ","alias":"夜艺人别名;备用"},
                {"id":0,"name":"无链接艺人","alias":["夜不显示"]},
                {"id":23,"name":"末位艺人","transName":"夜不可当别名","alias":["先列候选","夜可用别名"]}
            ],
            "al":{"id":24,"name":"合成专辑","tns":["另一个译名"],"transNames":["夜低优先译名"],"alia":["夜专辑别名"],"ala":["夜低优先别名"]}
        });
        let song_result = wrapped(json!({
            "tns":["夜外层标题"],
            "baseInfo":{"simpleSongData":song},
            "extInfo":{"lyrics":{"txt":"甲夜","range":[{"first":1,"second":2}]}}
        }));
        let lyrics_result = wrapped(json!({
            "tns":["夜外层标题"],
            "song":song,
            "lyrics":"多行夜\n第二行",
            "extInfo":{"lyrics":{"txt":"不使用","range":[]}}
        }));
        let expected = json!({
            "titleSuffix":marked("幕译名"),
            "artistSuffixes":[marked("译艺人"),null,marked("可用别名")],
            "albumSuffix":marked("专辑别名")
        });

        let songs = row(song_result.clone(), SearchCategory::Songs, "1");
        assert_eq!(songs["inlineCredits"], expected);
        assert_eq!(
            songs["tableHint"]["lyricParts"],
            json!([{"text":"甲","highlight":false},{"text":"夜","highlight":true}])
        );
        assert_eq!(songs["excerpt"], json!([]));

        let lyrics = row(lyrics_result, SearchCategory::Lyrics, "1");
        assert_eq!(lyrics["inlineCredits"], expected);
        assert!(lyrics.get("tableHint").is_none());
        assert_eq!(lyrics["excerpt"], json!(["多行夜", "第二行"]));

        let guest = row(song_result, SearchCategory::Songs, "");
        assert!(guest.get("inlineCredits").is_none());
    }

    #[test]
    fn outer_translation_fallback_and_an_unlinked_album_preserve_display_without_actions() {
        let result = row(
            wrapped(json!({
                "tns":["夜外层译名"],
                "song":{"id":21,"name":"主标题","alia":["不用歌曲别名"],
                    "album":{"name":"无链接专辑","transName":"可见首译","alia":[],"alias":["夜低优先别名"]}}
            })),
            SearchCategory::Lyrics,
            "1",
        );
        assert_eq!(result["inlineCredits"]["titleSuffix"], marked("外层译名"));
        assert_eq!(
            result["inlineCredits"]["albumSuffix"],
            json!([
                {"text":"（","match":false},
                {"text":"可见首译","match":false},
                {"text":"）","match":false}
            ])
        );
        assert!(result["inlineCredits"].get("artistSuffixes").is_none());
        assert!(result["track"]["albumId"].is_null());
    }

    #[test]
    fn dedicated_suffix_prefers_the_effective_rewrite_only_when_enabled() {
        let mut songs = wrapped(json!({"baseInfo":{"simpleSongData":{
            "id":21,"name":"主标题","tns":["夜幕译名","清晨译名"]
        }}}));
        songs["data"]["queryRewriteInfo"] = json!({"rewriteQuery":"清晨"});
        let mut lyrics = wrapped(json!({"song":{
            "id":21,"name":"主标题","tns":["夜幕译名","清晨译名"]
        }}));
        lyrics["data"]["queryRewriteInfo"] = json!({"rewriteQuery":"清晨"});
        for (raw, category, correct, matched) in [
            (songs.clone(), SearchCategory::Songs, true, "清晨"),
            (songs, SearchCategory::Songs, false, "夜"),
            (lyrics, SearchCategory::Lyrics, true, "夜"),
        ] {
            let mut search = query(category);
            search.correct = correct;
            let (view, _, _) = results::project(raw, search, "1", false).unwrap();
            let item = serde_json::to_value(&view.sections[0].items[0]).unwrap();
            assert_eq!(
                item["inlineCredits"]["titleSuffix"][1],
                json!({"text":matched,"match":true})
            );
        }
    }

    #[test]
    fn conflicting_album_identity_and_unsupported_values_do_not_make_suffixes() {
        let result = row(
            wrapped(json!({
                "tns":["夜外层不应盖过空数组"],
                "song":{"id":21,"name":"主标题","tns":[],"alia":["夜歌曲别名"],
                    "ar":[{"id":22,"name":"仍可链接的艺人","alias":["https://untrusted.invalid/credential"]}],
                    "al":{"id":24,"name":"同名专辑","tns":["夜紧凑专辑"]},
                    "album":{"id":25,"name":"同名专辑","tns":["夜不同专辑"]}}
            })),
            SearchCategory::Lyrics,
            "1",
        );
        assert!(result.get("inlineCredits").is_none());
        assert_eq!(result["track"]["albumId"], "24");
        assert_eq!(result["track"]["artistRefs"][0]["id"], "22");
        assert!(!result.to_string().contains("untrusted.invalid"));
    }
}
