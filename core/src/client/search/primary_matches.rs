//! M352:879,1107 delegates the dedicated Songs/Lyrics base row to M113;
//! the named album uses M134. Each observed label has its own M123.d
//! marks. Inline secondary names and the M607 footer are separate.

use crate::{
    catalog::search::{SearchLabelMatcher, SearchSuggestionPart},
    client::dto::TrackView,
};
use serde::Serialize;

const MAX_ARTIST_SLOTS: usize = 16;

/// Presence makes the projection authoritative for this row. An omitted
/// field/slot means plain existing text, not renderer-side re-matching.
/// This type contains no playback, link, or operation authority.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSongPrimaryMatches {
    #[serde(skip_serializing_if = "Option::is_none")]
    title_parts: Option<Vec<SearchSuggestionPart>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    artist_parts: Vec<Option<Vec<SearchSuggestionPart>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    album_parts: Option<Vec<SearchSuggestionPart>>,
}

pub(super) struct PreparedPrimaryMatches(SearchLabelMatcher);

impl PreparedPrimaryMatches {
    /// The caller supplies the effective search term for this exact view.
    pub(super) fn new(keyword: &str) -> Self {
        Self(SearchLabelMatcher::new(keyword))
    }

    pub(super) fn project(&self, track: &TrackView) -> SearchSongPrimaryMatches {
        let named = |label: &str| {
            if label.is_empty() {
                None
            } else {
                self.0.changed_parts(label)
            }
        };
        let mut artist_parts: Vec<_> = if track.artist_refs.len() <= MAX_ARTIST_SLOTS {
            track
                .artist_refs
                .iter()
                .map(|artist| {
                    // M113:191–243 uses MarkText only inside a linked artist.
                    artist.id.as_ref()?;
                    named(&artist.name)
                })
                .collect()
        } else {
            Vec::new()
        };
        if artist_parts.iter().all(Option::is_none) {
            artist_parts.clear();
        }
        SearchSongPrimaryMatches {
            title_parts: named(&track.name),
            artist_parts,
            // M134:42–85 uses MarkText on a named album whether or not
            // it has an ID. Missing labels are handled by the row, not here.
            album_parts: named(&track.album),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        catalog::search::results::{SearchCategory, SearchResultQuery},
        client::{mapping, search::results},
    };
    use serde_json::{Value, json};

    #[test]
    fn prepared_primary_labels_match_the_frozen_m123_d_differential() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/search-suggestion-marks-official.json"
        ))
        .unwrap();
        let vectors = fixture["vectors"].as_array().unwrap();
        assert_eq!(vectors.len(), 33);
        for case in vectors {
            let label = case["label"].as_str().unwrap();
            let keyword = case["keyword"].as_str().unwrap();
            let expected = match case["scope"].as_str().unwrap() {
                "exact" => case["originalParts"].clone(),
                "plain_for_empty_pattern" | "plain_for_input_bound" => {
                    json!([{"text":label,"match":false}])
                }
                scope => panic!("unexpected primary-label fixture scope: {scope}"),
            };
            let actual = SearchLabelMatcher::new(keyword)
                .changed_parts(label)
                .map_or_else(
                    || json!([{"text":label,"match":false}]),
                    |parts| serde_json::to_value(parts).unwrap(),
                );
            assert_eq!(actual, expected, "{}", case["name"]);
        }
    }

    #[test]
    fn projects_only_supported_display_labels_with_the_prepared_effective_term() {
        let track = mapping::track(&json!({
            "id":21,
            "name":"Ａ＋Ｂ A+B 夜场",
            "ar":[
                {"id":22,"name":"樂隊 乐队 + A+B"},
                {"id":0,"name":"乐队 + A+B"},
                {"id":23,"name":"完全不匹配"}
            ],
            "al":{"name":"<img> 乐队 / A+B"}
        }))
        .unwrap();
        let shown = PreparedPrimaryMatches::new("Ａ＋Ｂ 樂隊").project(&track);
        let shown = serde_json::to_value(shown).unwrap();
        assert_eq!(
            shown,
            json!({
                "titleParts":[
                    {"text":"Ａ＋Ｂ ","match":false},{"text":"A+B","match":true},{"text":" 夜场","match":false}
                ],
                "artistParts":[[
                    {"text":"樂隊 ","match":false},{"text":"乐队","match":true},
                    {"text":" + ","match":false},{"text":"A+B","match":true}
                ],null,null],
                "albumParts":[
                    {"text":"<img> ","match":false},{"text":"乐队","match":true},
                    {"text":" / ","match":false},{"text":"A+B","match":true}
                ]
            })
        );
        assert_eq!(track.artist_refs[0].id.as_deref(), Some("22"));
        assert!(track.artist_refs[1].id.is_none() && track.album_id.is_none());
        let original = PreparedPrimaryMatches::new("original typo").project(&track);
        assert_eq!(serde_json::to_value(original).unwrap(), json!({}));
    }

    #[test]
    fn keeps_plain_misses_sparse_and_bounds_duplicate_label_bytes() {
        let matcher = SearchLabelMatcher::new("a");
        assert!(matcher.changed_parts("not found").is_none());
        assert!(
            matcher
                .changed_parts(&format!(" {} ", "a".repeat(1025)))
                .is_none()
        );
        assert_eq!(
            serde_json::to_value(matcher.changed_parts("　").unwrap()).unwrap(),
            json!([])
        );
        assert_eq!(
            serde_json::to_value(matcher.changed_parts("  untouched  ").unwrap()).unwrap(),
            json!([{"text":"untouched","match":false}])
        );
    }

    #[test]
    fn projects_against_the_effective_word_only_for_signed_dedicated_rows() {
        let song = json!({"id":21,"name":"原词 A+B 乐队","ar":[
            {"id":22,"name":"乐队"},{"id":0,"name":"乐队"}
        ]});
        let run = |category, correct, uid| {
            let resource = if category == SearchCategory::Songs {
                json!({"baseInfo":{"simpleSongData":song},"extInfo":{"lyrics":{
                    "txt":"原词仍显示","range":[{"first":0,"second":2}]
                }}})
            } else {
                json!({"song":song,"lyrics":"原词和乐队\n第二行"})
            };
            let response = json!({"data":{"resources":[resource],"totalCount":1,
                "queryRewriteInfo":{"rewriteQuery":"Ａ＋Ｂ 樂隊"}}});
            let query = SearchResultQuery {
                keyword: "原词".into(),
                category,
                offset: 0,
                limit: 10,
                correct,
                channel: None,
            };
            let (view, _, _) = results::project(response, query, uid, false).unwrap();
            serde_json::to_value(&view.sections[0].items[0]).unwrap()
        };
        let marked = |item: &Value| {
            item["primaryMatches"]["titleParts"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["match"] == true)
                .map(|p| p["text"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        let songs = run(SearchCategory::Songs, true, "1");
        assert_eq!(marked(&songs), ["A+B", "乐队"]);
        assert!(songs["primaryMatches"]["artistParts"][1].is_null());
        assert_eq!(songs["tableHint"]["lyricParts"][0]["text"], "原词");
        assert_eq!(marked(&run(SearchCategory::Songs, false, "1")), ["原词"]);
        let lyrics = run(SearchCategory::Lyrics, true, "1");
        assert_eq!(marked(&lyrics), ["原词"]);
        assert_eq!(lyrics["excerpt"], json!(["原词和乐队", "第二行"]));
        assert!(lyrics.get("tableHint").is_none());
        assert!(
            run(SearchCategory::Songs, true, "")
                .get("primaryMatches")
                .is_none()
        );
    }
}
