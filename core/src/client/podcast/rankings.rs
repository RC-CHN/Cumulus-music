//! M/3004 and M/597: public voice, podcast, host, and audiobook charts.
use super::{
    EpisodeView, MusicClient, PodcastCard,
    browse_playback::{self, Lane},
    home::podcast_recommendations_enabled,
    view,
};
use crate::{
    Error, Result,
    account::login_policy::AB_PATH,
    client::{dto::AccountData, mapping::artwork, scene_fm::home::ClassicObservation},
    podcast::{Episode, Radio},
    protocol::api::{Api, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const TOP_EXPERIMENT: &str = "podcast_tab_better";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PodcastRankGroup {
    Voice,
    Audio,
    Talent,
    OverallCharts,
    FreeCharts,
    NewCharts,
    VipCharts,
}
impl PodcastRankGroup {
    const BOOKS: [Self; 4] = [
        Self::OverallCharts,
        Self::FreeCharts,
        Self::NewCharts,
        Self::VipCharts,
    ];
    fn book_code(self) -> Option<&'static str> {
        Some(match self {
            Self::OverallCharts => "OVERALL_CHARTS",
            Self::FreeCharts => "FREE_CHARTS",
            Self::NewCharts => "NEW_CHARTS",
            Self::VipCharts => "VIP_CHARTS",
            _ => return None,
        })
    }
    fn book_label(self) -> &'static str {
        match self {
            Self::OverallCharts => "听书总榜",
            Self::FreeCharts => "免费听书榜",
            Self::NewCharts => "新书榜",
            Self::VipCharts => "VIP听书榜",
            _ => "",
        }
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PodcastRankQuery {
    TopConfig,
    BookGroups,
    Menu {
        group: PodcastRankGroup,
    },
    Entries {
        group: PodcastRankGroup,
        id: String,
        chart_type: String,
        #[serde(default)]
        preview: bool,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastRankTab {
    pub id: String,
    pub name: String,
    pub chart_type: String,
    pub show_score: bool,
    pub show_rank_offset: bool,
}
#[derive(Serialize)]
pub struct PodcastRankMenu {
    pub group: PodcastRankGroup,
    pub name: String,
    pub tabs: Vec<PodcastRankTab>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastRankRow {
    pub rank: Option<u64>,
    pub last_rank: Option<u64>,
    pub score: Option<String>,
    pub resource: PodcastRankResource,
}
#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PodcastRankResource {
    Radio {
        radio: PodcastCard,
    },
    Voice {
        voice: EpisodeView,
    },
    Host {
        id: String,
        name: String,
        avatar_url: Option<String>,
    },
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PodcastRanks {
    TopConfig {
        layout: PodcastTopLayout,
        #[serde(rename = "sceneFmAllowed")]
        scene_fm_allowed: bool,
    },
    BookGroups {
        groups: Vec<PodcastRankMenu>,
    },
    Menu {
        tabs: Vec<PodcastRankTab>,
    },
    Entries {
        rows: Vec<PodcastRankRow>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PodcastTopLayout {
    Classic,
    Updated,
}

fn label(raw: &Value) -> Option<String> {
    let name: String = raw
        .as_str()?
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect();
    (!name.is_empty()).then_some(name)
}
fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 64
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn part(raw: &Value) -> Option<String> {
    let value = raw
        .as_str()
        .map(str::to_owned)
        .or_else(|| raw.as_u64().map(|v| v.to_string()))?;
    valid_part(&value).then_some(value)
}
fn array(raw: &Value) -> Result<&[Value]> {
    raw.as_array()
        .map(Vec::as_slice)
        .ok_or(Error::Response("missing podcast rank records"))
}
fn ordinary_menu(rows: &[Value]) -> Vec<PodcastRankTab> {
    rows.iter()
        .take(100)
        .filter_map(|row| {
            Some(PodcastRankTab {
                id: part(&row["resourceId"])
                    .filter(|id| id != "0")
                    .or_else(|| part(&row["logId"]))
                    .unwrap_or_else(|| "0".into()),
                name: label(&row["name"])?,
                chart_type: part(&row["chartType"]).unwrap_or_else(|| "0".into()),
                show_score: row["showScore"] == true,
                show_rank_offset: row["showRankOffset"] == true,
            })
        })
        .collect()
}
fn book_groups(rows: &[Value], enabled: bool) -> Vec<PodcastRankMenu> {
    PodcastRankGroup::BOOKS
        .into_iter()
        .filter_map(|group| {
            let row = rows
                .iter()
                .find(|row| row["chartNameCode"].as_str() == group.book_code())?;
            let tabs: Vec<_> = row["subList"]
                .as_array()?
                .iter()
                .take(100)
                .filter_map(|row| {
                    let id = part(&row["code"])?;
                    if id == "RECOMMEND_CHARTS" && !enabled {
                        return None;
                    }
                    Some(PodcastRankTab {
                        id,
                        name: label(&row["name"])?,
                        chart_type: part(&row["type"]).unwrap_or_else(|| "0".into()),
                        show_score: true,
                        show_rank_offset: false,
                    })
                })
                .collect();
            (!tabs.is_empty()).then(|| PodcastRankMenu {
                group,
                name: group.book_label().into(),
                tabs,
            })
        })
        .collect()
}

fn score(raw: &Value) -> Option<String> {
    let value = raw
        .as_str()
        .map(|s| s.trim_end_matches('分').to_owned())
        .or_else(|| raw.as_f64().map(|n| n.to_string()))?;
    let number = value.parse::<f64>().ok()?;
    (number.is_finite() && number > 0.0 && value.len() <= 12).then_some(value)
}
fn number(raw: &Value) -> Option<u64> {
    raw.as_u64()
        .or_else(|| raw.as_str()?.parse().ok())
        .filter(|n| *n > 0)
}
fn row(
    raw: &Value,
    group: PodcastRankGroup,
    authority: Option<(&browse_playback::Host, &browse_playback::Observation)>,
) -> Option<PodcastRankRow> {
    let chart = &raw["chartMetaVO"];
    let resource = match group {
        PodcastRankGroup::Voice => {
            let raw = &raw["voiceMetaVO"];
            let episode = Episode::from_official(raw).ok()?;
            let mut voice = view::episode(&episode, &[]);
            if let Some((host, observation)) = authority {
                host.bind(observation, &episode, &mut voice)?;
            }
            voice.name = label(&json!(voice.name))?;
            if voice.radio_name.is_empty() {
                voice.radio_name = label(&raw["radioName"]).unwrap_or_default();
            }
            PodcastRankResource::Voice { voice }
        }
        PodcastRankGroup::Talent => PodcastRankResource::Host {
            id: identifier(&raw["id"]).ok()?,
            name: label(&raw["nickname"]).or_else(|| label(&raw["name"]))?,
            avatar_url: artwork(&raw["avatarUrl"]),
        },
        _ => {
            let mut raw = raw["voicelistMetaVO"].clone();
            if raw["anchorVO"].is_object() {
                raw["dj"] = raw["anchorVO"].clone();
            }
            let mut radio = view::card(&Radio::from_official(&raw).ok()?);
            radio.name = label(&json!(radio.name))?;
            radio.description.clear();
            PodcastRankResource::Radio { radio }
        }
    };
    Some(PodcastRankRow {
        rank: number(&chart["rank"]).or_else(|| number(&raw["rank"])),
        last_rank: number(&chart["lastRank"]).or_else(|| number(&raw["lastRank"])),
        score: score(&raw["voicelistMetaVO"]["score"]).or_else(|| score(&chart["score"])),
        resource,
    })
}

async fn top_group(api: &dyn Api) -> Option<String> {
    let response = tokio::time::timeout(
        Duration::from_secs(3),
        api.call(AB_PATH, json!({"expNames":TOP_EXPERIMENT}), false, &[200]),
    )
    .await
    .ok()?
    .ok()?;
    let rows = response["data"]
        .as_array()
        .filter(|rows| rows.len() <= 4096)?;
    let mut matching = rows.iter().filter(|row| row["expName"] == TOP_EXPERIMENT);
    let group = matching.next()?.get("expGroupName")?.as_str()?;
    matching.next().is_none().then(|| group.to_owned())
}

async fn request(
    api: Arc<dyn Api>,
    query: PodcastRankQuery,
    authority: Option<(&browse_playback::Host, &browse_playback::Observation)>,
) -> Result<PodcastRanks> {
    use PodcastRankGroup::*;
    Ok(match query {
        PodcastRankQuery::TopConfig => {
            return Err(Error::State(
                "podcast top configuration requires scoped observation",
            ));
        }
        PodcastRankQuery::BookGroups => {
            let value = api
                .call("/api/voice/book/chart/config", json!({}), false, &[200])
                .await?;
            // Optional book configuration is independent of the ordinary chart menus.
            let enabled = podcast_recommendations_enabled(api.clone()).await?;
            PodcastRanks::BookGroups {
                groups: book_groups(array(&value["data"])?, enabled),
            }
        }
        PodcastRankQuery::Menu { group } => {
            let tabs = match group {
                Voice | Audio => {
                    let id = if group == Voice { 2 } else { 1 };
                    let raw = api
                        .call(
                            "/api/podcast/ranklist/square/ranklist/get/v2",
                            json!({"ranklistSquareId":id}),
                            false,
                            &[200],
                        )
                        .await?;
                    ordinary_menu(array(&raw["data"])?)
                }
                Talent => [
                    ("hours", "每日榜"),
                    ("popular", "热门榜"),
                    ("newcomer", "新人榜"),
                ]
                .into_iter()
                .map(|(id, name)| PodcastRankTab {
                    id: id.into(),
                    name: name.into(),
                    chart_type: "0".into(),
                    show_score: false,
                    show_rank_offset: true,
                })
                .collect(),
                _ => {
                    return Err(Error::InvalidInput(
                        "audiobook rank menu is provided by the configuration",
                    ));
                }
            };
            PodcastRanks::Menu { tabs }
        }
        PodcastRankQuery::Entries {
            group,
            id,
            chart_type,
            preview,
        } => {
            if !valid_part(&id) || !valid_part(&chart_type) {
                return Err(Error::InvalidInput("invalid podcast rank target"));
            }
            let (path, args) = match group {
                Voice if chart_type != "0" => {
                    ("/api/charts/voice", json!({"chartType":chart_type}))
                }
                Voice => ("/api/charts/voice/category", json!({"categoryId":id})),
                Audio if chart_type != "0" => {
                    ("/api/charts/voicelist", json!({"chartType":chart_type}))
                }
                Audio => ("/api/charts/voicelist/category", json!({"categoryId":id})),
                Talent => match (id.as_str(), chart_type.as_str()) {
                    ("hours", "0") => ("/api/dj/toplist/hours", json!({})),
                    ("popular", "0") => ("/api/dj/toplist/popular", json!({})),
                    ("newcomer", "0") => ("/api/dj/toplist/newcomer", json!({})),
                    _ => return Err(Error::InvalidInput("unknown podcast host ranking")),
                },
                _ => {
                    if id == "RECOMMEND_CHARTS"
                        && !podcast_recommendations_enabled(api.clone()).await?
                    {
                        return Err(Error::InvalidInput(
                            "personalized podcast ranks are disabled",
                        ));
                    }
                    (
                        "/api/voice/book/chart/get",
                        json!({"chartNameCode":group.book_code(),"subCode":id,"type":chart_type}),
                    )
                }
            };
            let raw = api.call(path, args, false, &[200]).await?;
            let list = if group == Talent {
                &raw["data"]["list"]
            } else {
                &raw["data"]
            };
            PodcastRanks::Entries {
                rows: array(list)?
                    .iter()
                    .take(500)
                    .filter_map(|raw| row(raw, group, authority))
                    .take(if preview { 8 } else { 500 })
                    .collect(),
            }
        }
    })
}

impl MusicClient {
    pub async fn podcast_rankings(
        &self,
        epoch: &str,
        query: PodcastRankQuery,
    ) -> Result<AccountData<PodcastRanks>> {
        let token = self.podcast_token(epoch)?;
        let value = if matches!(query, PodcastRankQuery::TopConfig) {
            let sequence = self.scene_fm.begin_top_observation()?;
            let api = self.session.native_api(&token)?;
            let revision = api.verified_pc_experiment_revision().ok().flatten();
            let group = top_group(api.as_ref()).await;
            let layout = if group.as_deref() == Some("t1") {
                PodcastTopLayout::Updated
            } else {
                PodcastTopLayout::Classic
            };
            // The official default renders the classic layout even without an
            // assignment. Only the explicit known control assignment may arm
            // native automatic audio; never equate a read failure with it.
            let classic = revision
                .filter(|revision| {
                    group.as_deref() == Some("c")
                        && api
                            .verified_pc_experiment_revision()
                            .ok()
                            .flatten()
                            .as_ref()
                            == Some(revision)
                })
                .map(|revision| ClassicObservation::new(&token, &api, revision));
            let scene_fm_allowed = self.session.with_verified_profile(&token, &api, |_, _| {
                self.scene_fm.finish_top_observation(sequence, classic)
            })?;
            PodcastRanks::TopConfig {
                layout,
                scene_fm_allowed,
            }
        } else {
            let api = self.session.native_api(&token)?;
            let observation = match &query {
                PodcastRankQuery::Entries {
                    group: PodcastRankGroup::Voice,
                    id,
                    chart_type,
                    preview,
                } if valid_part(id) && valid_part(chart_type) => {
                    Some(self.podcast_browse_playback.begin(
                        &token,
                        &api,
                        Lane::Rank {
                            id: id.clone(),
                            chart_type: chart_type.clone(),
                            preview: *preview,
                        },
                        false,
                    )?)
                }
                _ => None,
            };
            let authority = observation
                .as_ref()
                .map(|value| (&self.podcast_browse_playback, value));
            let value = self
                .session
                .run_for(&token, |api, _| request(api, query, authority))
                .await?;
            self.session.require_api(&token, &api)?;
            if observation
                .as_ref()
                .is_some_and(|value| !value.current(&token, &api))
            {
                return Err(Error::StaleResponse);
            }
            value
        };
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
