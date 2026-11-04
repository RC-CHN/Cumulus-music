//! M/1211.c and M/15.hg: only project native identities from the Choice menu.
use super::{valid_tab, validate_page_code};
use crate::{Error, Result, protocol::api::Api};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use url::Url;

const PATH: &str = "/api/secondary/tab/list";
const PRIMARY: usize = 5;
const MAX_CHANNELS: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerticalZoneMenu {
    pub available: bool,
    pub default_zone_tags: Vec<VerticalZoneMenuTag>,
    pub more_zone_tags: Vec<VerticalZoneMenuTag>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerticalZoneMenuTag {
    pub key: String,
    pub name: String,
    pub page_code: String,
}

impl VerticalZoneMenu {
    pub(crate) fn contains(&self, code: &str) -> bool {
        self.default_zone_tags
            .iter()
            .chain(&self.more_zone_tags)
            .any(|tag| tag.page_code == code)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.default_zone_tags.is_empty() && self.more_zone_tags.is_empty()
    }
}

pub(crate) async fn read_menu(api: &dyn Api) -> Result<VerticalZoneMenu> {
    let response = api.call(PATH, json!({}), false, &[200]).await?;
    project(&response)
}

fn project(response: &Value) -> Result<VerticalZoneMenu> {
    let hints = response["data"]["hints"]
        .as_array()
        .ok_or(Error::Response("missing vertical zone menu hints"))?;
    let rows = hints
        .iter()
        .find(|hint| hint["position"]["code"] == "music_top_tab_full")
        .and_then(|hint| hint["data"]["extra"]["nowChannelItems"].as_array());
    let mut ordered = Vec::new();
    let mut keys = HashSet::new();
    let mut pages = HashSet::new();
    if let Some(rows) = rows {
        if rows.len() > MAX_CHANNELS {
            return Err(Error::Response("vertical zone menu exceeds channel bound"));
        }
        for row in rows {
            let Some(key) = row["code"].as_str().filter(|s| valid_tab(s)) else {
                continue;
            };
            if matches!(
                key,
                "feature" | "playlist" | "chart" | "singer" | "vip" | "origin"
            ) {
                continue;
            }
            let Some(name) = row["title"]
                .as_str()
                .filter(|s| valid_tab(s) && !s.trim().is_empty())
            else {
                continue;
            };
            // URLs are never returned or opened. A raw, unique page query
            // parameter must fit the same opaque ASCII identity as zone/get.
            let Some(url) = row["url"]
                .as_str()
                .filter(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
                .and_then(|s| Url::parse(s).ok())
            else {
                continue;
            };
            let mut params = url
                .query()
                .unwrap_or_default()
                .split('&')
                .filter_map(|pair| {
                    let (key, value) = pair.split_once('=')?;
                    (key == "page").then_some(value)
                });
            let Some(page_code) = params.next() else {
                continue;
            };
            if params.next().is_some()
                || validate_page_code(page_code).is_err()
                || keys.contains(key)
                || pages.contains(page_code)
            {
                continue;
            }
            keys.insert(key.to_owned());
            pages.insert(page_code.to_owned());
            ordered.push(VerticalZoneMenuTag {
                key: key.into(),
                name: name.into(),
                page_code: page_code.into(),
            });
        }
    }
    let more_zone_tags = ordered.split_off(ordered.len().min(PRIMARY));
    Ok(VerticalZoneMenu {
        available: true,
        default_zone_tags: ordered,
        more_zone_tags,
    })
}
