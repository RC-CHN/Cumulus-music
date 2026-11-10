//! Device-local main interface font; separate from desktop and vinyl lyrics.
use crate::{Error, Result, storage::StateStore};
use serde::Serialize;
use serde_json::Value;

const OWNER: &str = "local-device";
const KIND: &str = "interface-font-v1";
const DEFAULT_LABEL: &str = "默认";
const DECORATIVE: [&str; 5] = [
    "华文琥珀",
    "华文彩云",
    "汉仪篆书繁",
    "汉仪瘦金书",
    "汉仪瘦金书繁",
];

pub fn is_choice(name: &str) -> bool {
    name != DEFAULT_LABEL
        && name.len() <= 256
        && !name.chars().any(char::is_control)
        && name.chars().any(|c| ('\u{4e00}'..='\u{9fa5}').contains(&c))
        && !DECORATIVE.contains(&name)
}

pub fn choices<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut choices: Vec<_> = names.into_iter().filter(|name| is_choice(name)).collect();
    // The original Array.sort compares UTF-16 code units, not locale collation.
    choices.sort_unstable_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
    choices.dedup();
    choices.into_iter().map(str::to_owned).collect()
}

fn size(name: &str) -> u8 {
    if name.contains("华康少女") || name.contains("汉仪") {
        return 13;
    }
    match name {
        "华文细黑" | "仿宋" => 13,
        "方正舒体" | "华文仿宋" | "华文彩云" | "华文新魏" | "华文楷体" | "华文行楷" | "楷体"
        | "隶书" => 14,
        "华文隶书" => 15,
        _ => 12,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InterfaceFont {
    family: Option<String>,
    #[serde(rename = "sizePx")]
    size_px: u8,
}

impl InterfaceFont {
    fn new(family: Option<String>) -> Result<Self> {
        if family.as_deref().is_some_and(|name| !is_choice(name)) {
            return Err(Error::InvalidInput("invalid interface font"));
        }
        Ok(Self {
            size_px: family.as_deref().map_or(12, size),
            family,
        })
    }

    pub fn read(store: &StateStore) -> Result<Self> {
        let value = store.get(OWNER, KIND, "family")?;
        let family = match value {
            None | Some(Value::Null) => None,
            Some(Value::String(family)) => Some(family),
            Some(_) => return Err(Error::State("invalid saved interface font")),
        };
        Self::new(family)
    }

    pub fn replace(store: &StateStore, family: Option<String>) -> Result<Self> {
        let next = Self::new(family)?;
        store.put(OWNER, KIND, "family", &serde_json::to_value(&next.family)?)?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_filters_sorts_and_deduplicates_like_the_official_main_menu() {
        assert_eq!(
            choices([
                "楷体",
                "Latin",
                "华文琥珀",
                "仿宋",
                "默认",
                "汉仪篆书繁",
                "仿宋",
                "汉仪普通",
                "\u{9fa6}",
                "楷\n体",
                "\u{e000}中",
                "\u{10000}中",
            ]),
            ["仿宋", "楷体", "汉仪普通", "\u{10000}中", "\u{e000}中"]
        );
    }

    #[test]
    fn saved_family_drives_size_and_does_not_touch_lyric_or_account_preference() {
        use crate::preferences::desktop_lyrics::DesktopLyricPreferences;

        let folder = tempfile::tempdir().unwrap();
        let store = StateStore::open(folder.path()).unwrap();
        let desktop_lyrics = DesktopLyricPreferences::read(&store).unwrap();
        assert_eq!(InterfaceFont::read(&store).unwrap().size_px, 12);
        for (family, expected) in [
            ("仿宋", 13),
            ("楷体", 14),
            ("华文隶书", 15),
            ("汉仪其他", 13),
            ("华康少女新体", 13),
            ("微软雅黑", 12),
        ] {
            assert_eq!(
                InterfaceFont::replace(&store, Some(family.into()))
                    .unwrap()
                    .size_px,
                expected
            );
        }
        assert!(InterfaceFont::replace(&store, Some("华文彩云".into())).is_err());
        drop(store);
        let store = StateStore::open(folder.path()).unwrap();
        assert_eq!(
            InterfaceFont::read(&store).unwrap().family.as_deref(),
            Some("微软雅黑")
        );
        assert!(store.get("7", KIND, "family").unwrap().is_none());
        assert_eq!(InterfaceFont::replace(&store, None).unwrap().size_px, 12);
        assert_eq!(InterfaceFont::read(&store).unwrap().family, None);
        assert_eq!(
            DesktopLyricPreferences::read(&store).unwrap(),
            desktop_lyrics
        );
    }

    #[test]
    fn malformed_local_record_is_not_silently_rewritten() {
        let folder = tempfile::tempdir().unwrap();
        let store = StateStore::open(folder.path()).unwrap();
        store
            .put(OWNER, KIND, "family", &Value::Bool(true))
            .unwrap();
        assert!(InterfaceFont::read(&store).is_err());
        assert_eq!(
            store.get(OWNER, KIND, "family").unwrap(),
            Some(Value::Bool(true))
        );
        InterfaceFont::replace(&store, None).unwrap();
        assert_eq!(InterfaceFont::read(&store).unwrap().family, None);
    }
}
