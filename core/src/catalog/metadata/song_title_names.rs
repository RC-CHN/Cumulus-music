//! Song-only title translations and aliases (M21/D and Classic TrackTitle).
use serde_json::Value;
use std::sync::Arc;

const MAX_ROWS: usize = 32;
const MAX_STRING_BYTES: usize = 1024;
const MAX_DISPLAY_BYTES: usize = 4096;

#[derive(Clone, Default)]
pub(crate) struct SongTitleNames(Option<Arc<Arrays>>);

struct Arrays {
    translations: Option<Vec<String>>,
    aliases: Option<Vec<String>>,
}

impl SongTitleNames {
    pub(crate) fn from_song(song: &Value) -> Self {
        // Official wire keys first, then the already-normalized track shape.
        // Album transName and an artist's textual alias are different fields.
        let translations = array(song, &["tns", "transNames"]);
        let aliases = array(song, &["alia", "ala", "alias"]);
        Self((translations.is_some() || aliases.is_some()).then(|| {
            Arc::new(Arrays {
                translations,
                aliases,
            })
        }))
    }

    pub(crate) fn display(&self) -> Option<String> {
        let names = self.0.as_ref()?;
        let text = names
            .translations
            .as_ref()
            .or(names.aliases.as_ref())?
            .join("；");
        (!text.is_empty()).then_some(text)
    }

    /// M/32.i checks the first translation's truthiness before trimming and
    /// falls back only to the first alias. M/123.b trims the chosen value.
    pub(crate) fn filter_label(&self) -> Option<&str> {
        let names = self.0.as_ref()?;
        names
            .translations
            .as_ref()
            .and_then(|values| values.first())
            .filter(|name| !name.is_empty())
            .or_else(|| names.aliases.as_ref().and_then(|values| values.first()))
            .map(String::as_str)
    }

    pub(crate) fn write_queue_fields(&self, song: &mut Value) {
        let Some(fields) = self.0.as_ref() else {
            return;
        };
        if let Some(names) = &fields.translations {
            song["tns"] = serde_json::json!(names);
        }
        if let Some(names) = &fields.aliases {
            song["alia"] = serde_json::json!(names);
        }
    }
}

fn array(song: &Value, keys: &[&str]) -> Option<Vec<String>> {
    let values = keys.iter().find_map(|key| song.get(key)?.as_array())?;
    // Empty arrays are present/truthy in the official client. Reject an
    // oversized/malformed array as a whole, while keeping that presence so it
    // cannot expose an unrelated fallback label.
    if values.len() > MAX_ROWS {
        return Some(vec![]);
    }
    let mut bytes = values.len().saturating_sub(1) * "；".len();
    let mut names = Vec::with_capacity(values.len());
    for value in values {
        let Some(name) = value.as_str().filter(|name| name.len() <= MAX_STRING_BYTES) else {
            return Some(vec![]);
        };
        bytes += name.len();
        if bytes > MAX_DISPLAY_BYTES {
            return Some(vec![]);
        }
        names.push(name.to_owned());
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classic_song_title_preserves_array_presence_order_and_text() {
        for (source, expected) in [
            (
                json!({"tns":["甲", "", "乙"],"alia":["备用"]}),
                Some("甲；；乙"),
            ),
            (
                json!({"tns":[],"transNames":["不回退"],"alia":["备用"]}),
                None,
            ),
            (json!({"tns":null,"alia":["甲","乙"]}), Some("甲；乙")),
            (
                json!({"alia":[],"ala":["不回退"],"alias":["也不回退"]}),
                None,
            ),
            (json!({"ala":["旧接口"]}), Some("旧接口")),
            (
                json!({"transNames":["归一化译名"],"alias":["归一化别名"]}),
                Some("归一化译名"),
            ),
            (json!({"alias":["  原样  "]}), Some("  原样  ")),
            (
                json!({"transName":"专辑字段","alias":"非歌曲数组","album":{"tns":["专辑译名"]}}),
                None,
            ),
        ] {
            let names = SongTitleNames::from_song(&source);
            assert_eq!(names.display().as_deref(), expected, "{source}");
            let mut queue = json!({});
            names.write_queue_fields(&mut queue);
            assert_eq!(
                SongTitleNames::from_song(&queue).display().as_deref(),
                expected
            );
        }
    }

    #[test]
    fn classic_song_title_rejects_malformed_or_oversize_translation_without_fallback() {
        for rows in [
            json!(["普通译名", {"authorization": "NOT_PUBLIC"}]),
            json!(vec!["多"; MAX_ROWS + 1]),
            json!(["x".repeat(MAX_STRING_BYTES + 1)]),
            json!(vec!["a".repeat(MAX_STRING_BYTES); 4]),
        ] {
            let names = SongTitleNames::from_song(&json!({"tns":rows,"alia":["备用"]}));
            assert!(names.display().is_none());
            let mut queue = json!({});
            names.write_queue_fields(&mut queue);
            assert_eq!(queue, json!({"tns":[],"alia":["备用"]}));
        }
    }
}
