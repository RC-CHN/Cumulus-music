use super::{first, strings, text};
use crate::{Result, protocol::api::identifier};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct ArtistCredit {
    /// Display-only credits (including artistName) need not have a valid ID.
    pub id: Option<String>,
    pub name: String,
}
impl ArtistCredit {
    pub fn from_official(value: &Value) -> Self {
        Self {
            id: identifier(&value["id"]).ok(),
            name: text(&value["name"]),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ArtistMetadata {
    pub id: String,
    pub account_id: Option<String>,
    pub name: String,
    pub aliases: Vec<String>,
    pub translation: String,
    pub brief_description: String,
    pub picture_url: String,
    pub avatar_url: String,
    pub song_count: u64,
    pub album_count: u64,
    pub mv_count: Option<u64>,
    pub fans_count: u64,
    pub followed: Option<bool>,
}
impl ArtistMetadata {
    pub fn from_official(value: &Value) -> Result<Self> {
        let translation = text(first(value, &["trans", "transName"]))
            .trim()
            .to_owned();
        let mut aliases = if let Some(value) = value["alias"].as_str() {
            value
                .split(';')
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect()
        } else {
            strings(&value["alias"])
        };
        // N checks trans, not transName. Its slice() does not remove duplicates.
        if super::truthy(&value["trans"]) {
            aliases.insert(0, translation.clone());
        }
        let avatar = text(&value["img1v1Url"]);
        Ok(Self {
            id: identifier(&value["id"])?,
            account_id: identifier(&value["accountId"]).ok(),
            name: text(&value["name"]),
            aliases,
            translation,
            brief_description: text(&value["briefDesc"]),
            picture_url: text(&value["picUrl"]),
            avatar_url: if value["img1v1"] == -1 || avatar.contains("5639395138885805") {
                String::new()
            } else {
                avatar
            },
            song_count: value["musicSize"].as_u64().unwrap_or(0),
            album_count: value["albumSize"].as_u64().unwrap_or(0),
            mv_count: value["mvSize"].as_u64(),
            fans_count: first(value, &["fansSize", "fansCount"])
                .as_u64()
                .unwrap_or(0),
            followed: value["followed"].as_bool(),
        })
    }

    /// Recovered Header.tsx deduplicates the translated name and aliases.
    pub fn display_aliases(&self) -> Vec<String> {
        let mut result = Vec::new();
        for value in std::iter::once(&self.translation).chain(&self.aliases) {
            if !value.is_empty() && !result.contains(value) {
                result.push(value.clone());
            }
        }
        result
    }
}
