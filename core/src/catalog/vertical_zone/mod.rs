//! Official PC 3.1.42 vertical-zone catalogue, separate from the FM queue.
mod menu;
mod projection;
pub(crate) use menu::read_menu;
pub use menu::{VerticalZoneMenu, VerticalZoneMenuTag};

use crate::{
    Error, Result,
    protocol::{api::Api, pc_experiments::PcExperimentGroups},
};
use serde::Serialize;
use serde_json::{Value, json};

const DEFAULT_TAB: &str = "default";
const CLASSIC_PATH: &str = "/api/zone/get";
const TABBED_PATH: &str = "/api/zone/tab/get";

/// Search can supply a display name instead of a tab key. No other source
/// uses the name fallback (M/485; PageVerticalZone route from=searchHotExplain).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZoneEntry<'a> {
    Tab(&'a str),
    SearchTabName(&'a str),
}

impl ZoneEntry<'_> {
    pub fn validate(&self) -> Result<()> {
        let value = match self {
            Self::Tab(value) | Self::SearchTabName(value) => *value,
        };
        if value.is_empty() || valid_tab(value) {
            Ok(())
        } else {
            Err(Error::InvalidInput("invalid vertical zone tab entry"))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneLayout {
    Classic,
    Tabs,
    Fallback,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneTab {
    pub key: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerticalZonePage {
    pub view_id: String,
    pub card_size: ZoneCardSize,
    pub layout: ZoneLayout,
    pub tabs: Vec<ZoneTab>,
    pub active_tab: Option<ZoneTab>,
    pub blocks: Vec<ZoneBlock>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneCardSize {
    Default,
    Small,
    Mini,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneBlockKind {
    Carousel,
    Horizontal,
    Ranking,
    Grid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneBlock {
    pub kind: ZoneBlockKind,
    pub requires_recommendation: bool,
    pub title: Option<String>,
    pub cards: Vec<ZoneCard>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneCard {
    pub card_key: String,
    pub title: Option<String>,
    pub cover: Option<String>,
    pub target: ZoneTarget,
    pub can_play: bool,
    pub previews: Vec<ZoneRankPreview>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ZoneTarget {
    Song { id: String },
    Playlist { id: String, personalized: bool },
    Album { id: String },
    Artist { id: String },
    User { id: String },
    Podcast { id: String },
    Voice { id: String },
    Fm { id: String, personalized: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneRankPreview {
    pub title: String,
    pub artist: Option<String>,
}

/// Uses only the two read endpoints selected by M/2986. Assignment is a
/// native PC observation; the route alone cannot enable the experiment.
pub async fn read(
    api: &dyn Api,
    groups: Option<&PcExperimentGroups>,
    recommendation_enabled: bool,
    page_code: &str,
    entry: ZoneEntry<'_>,
) -> Result<VerticalZonePage> {
    validate_page_code(page_code)?;
    entry.validate()?;
    let size = match groups.and_then(|groups| groups.group("PH-PC-jingxuanPageEfficiency")) {
        Some("t1") => ZoneCardSize::Mini,
        Some("t2") => ZoneCardSize::Small,
        _ => ZoneCardSize::Default,
    };
    if groups.and_then(|groups| groups.group("PH-PC-zhuanquTab")) != Some("t1") {
        return classic(
            api,
            page_code,
            ZoneLayout::Classic,
            recommendation_enabled,
            size,
        )
        .await;
    }
    let requested = match entry {
        ZoneEntry::Tab("") | ZoneEntry::SearchTabName(_) => DEFAULT_TAB,
        ZoneEntry::Tab(key) if valid_tab(key) => key,
        _ => return Err(Error::InvalidInput("invalid vertical zone tab key")),
    };
    let name = match entry {
        ZoneEntry::SearchTabName("") => None,
        ZoneEntry::SearchTabName(name) if valid_tab(name) => Some(name),
        ZoneEntry::SearchTabName(_) => {
            return Err(Error::InvalidInput("invalid vertical zone tab name"));
        }
        _ => None,
    };
    match tabs(api, page_code, requested, name, recommendation_enabled).await {
        Ok(mut page) => {
            page.card_size = size;
            Ok(page)
        }
        Err(error)
            if error.is_service_code(301)
                || matches!(error, Error::StaleResponse | Error::CredentialChanged) =>
        {
            Err(error)
        }
        Err(_) => {
            classic(
                api,
                page_code,
                ZoneLayout::Fallback,
                recommendation_enabled,
                size,
            )
            .await
        }
    }
}

pub fn validate_page_code(page_code: &str) -> Result<()> {
    if page_code.is_empty()
        || page_code.len() > 128
        || !page_code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(Error::InvalidInput("invalid vertical zone page code"));
    }
    Ok(())
}

pub(super) fn valid_tab(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

async fn classic(
    api: &dyn Api,
    page_code: &str,
    layout: ZoneLayout,
    recommendation_enabled: bool,
    card_size: ZoneCardSize,
) -> Result<VerticalZonePage> {
    let value = api
        .call(CLASSIC_PATH, json!({"pageCode":page_code}), false, &[200])
        .await?;
    let rows = value["data"]
        .as_array()
        .ok_or(Error::Response("missing classic vertical zone data"))?;
    let content = rows.first().unwrap_or(&Value::Null);
    Ok(VerticalZonePage {
        view_id: String::new(),
        card_size,
        layout,
        tabs: Vec::new(),
        active_tab: projection::tab(content),
        blocks: projection::blocks(content, recommendation_enabled),
    })
}

async fn tab_request(api: &dyn Api, page_code: &str, key: &str) -> Result<Value> {
    api.call(
        TABBED_PATH,
        json!({"pageCode":page_code,"tabKey":key}),
        false,
        &[200],
    )
    .await
}

async fn tabs(
    api: &dyn Api,
    page_code: &str,
    requested: &str,
    name: Option<&str>,
    recommendation_enabled: bool,
) -> Result<VerticalZonePage> {
    let mut value = tab_request(api, page_code, requested).await?;
    let mut metas = projection::tab_metas(&value["data"]);
    if let Some(name) = name
        && let Some(matched) = metas.iter().find(|tab| tab.name == name)
        && value["data"]["currentTabContent"]["tabKey"] != matched.key
    {
        value = tab_request(api, page_code, &matched.key).await?;
        metas = projection::tab_metas(&value["data"]);
    }
    let content = &value["data"]["currentTabContent"];
    let active_tab = projection::tab(content)
        .filter(|tab| name.map_or(tab.key == requested, |name| tab.name == name))
        .ok_or(Error::Response(
            "vertical zone tab did not match the requested entry",
        ))?;
    Ok(VerticalZonePage {
        view_id: String::new(),
        card_size: ZoneCardSize::Default,
        layout: ZoneLayout::Tabs,
        tabs: metas,
        active_tab: Some(active_tab),
        blocks: projection::blocks(content, recommendation_enabled),
    })
}
