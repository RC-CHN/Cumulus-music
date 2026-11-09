//! The recommendation feed of the public podcast homepage, recovered from M/597.
use super::{
    EpisodeView, MusicClient, PodcastApi, PodcastCard,
    browse_playback::{self, Lane},
    discovery::DiscoveryScope,
    view,
};
use crate::{
    Result,
    account::AccountApi,
    client::dto::AccountData,
    podcast::{Episode, Radio},
    protocol::api::{Api, identifier},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) async fn podcast_recommendations_enabled(api: Arc<dyn Api>) -> Result<bool> {
    Ok(AccountApi::new(api).settings().await?["personalServiceSetting"] != 2)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastHomePage {
    recommendation_enabled: bool,
    blocks: Vec<PodcastHomeBlock>,
    continuation: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PodcastHomeBlock {
    Radios {
        title: Option<String>,
        cards: Vec<PodcastCard>,
    },
    Voices {
        title: Option<String>,
        groups: Vec<Vec<EpisodeView>>,
    },
}

fn label(value: &Value) -> Option<String> {
    let label: String = value
        .as_str()?
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect();
    (!label.is_empty()).then_some(label)
}

fn block(
    row: &Value,
    host: &browse_playback::Host,
    observation: &browse_playback::Observation,
) -> Option<PodcastHomeBlock> {
    if matches!(
        row["blockCode"].as_str(),
        Some("VOICE_HOMEPAGE_BLOCK_SCENE_FM" | "PC_HOMEPAGE_VOICEBOOK_RCMD")
    ) || !row["uiElement"].is_object()
    {
        return None;
    }
    let title = label(&row["uiElement"]["mainTitle"]["title"]);
    let creatives = row["creatives"].as_array()?.iter().take(12);
    match row["showType"].as_str()? {
        "VOICE_HOMEPAGE_RCMDLIKE_VOICELIST" | "VOICE_HOMEPAGE_FIXED_VOICELIST" => {
            let cards: Vec<_> = creatives
                .filter_map(|creative| {
                    let radio =
                        Radio::from_official(&creative["creativeExtInfoVO"]["radio"]).ok()?;
                    let mut card = view::card(&radio);
                    card.name = label(&json!(card.name))?;
                    card.description.clear();
                    Some(card)
                })
                .collect();
            (!cards.is_empty()).then_some(PodcastHomeBlock::Radios { title, cards })
        }
        "VOICE_HOMEPAGE_WEEKNEW_VOICE" => {
            let groups: Vec<Vec<EpisodeView>> = creatives
                .filter_map(|creative| {
                    let voices: Vec<_> = creative["resources"]
                        .as_array()?
                        .iter()
                        .take(6)
                        .filter_map(|resource| {
                            if resource["resourceType"] != "voice" || resource["valid"] == false {
                                return None;
                            }
                            let raw = &resource["resourceExtInfo"]["djProgram"];
                            let episode = Episode::from_official(raw).ok()?;
                            if identifier(&resource["resourceId"]).ok()? != episode.id {
                                return None;
                            }
                            let mut voice = view::episode(&episode, &[]);
                            voice.name = label(&json!(voice.name))?;
                            host.bind(observation, &episode, &mut voice)?;
                            Some(voice)
                        })
                        .collect();
                    (!voices.is_empty()).then_some(voices)
                })
                .collect();
            (!groups.is_empty()).then_some(PodcastHomeBlock::Voices { title, groups })
        }
        _ => None,
    }
}

impl MusicClient {
    pub async fn podcast_home(
        &self,
        epoch: &str,
        continuation: Option<&str>,
    ) -> Result<AccountData<PodcastHomePage>> {
        let token = self.podcast_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let observation = self.podcast_browse_playback.begin(
            &self.session,
            &token,
            &api,
            Lane::Weekly,
            continuation.is_some(),
        )?;
        let scope = DiscoveryScope::PodcastHome(observation.visit);
        let cursor = self
            .podcast_discovery
            .resolve(epoch, &scope, continuation)?;
        let result = self
            .session
            .run_for(&token, |api, _| async {
                // A continuation may outlive a settings change on this or
                // another desktop. Recheck before any personalized request.
                if !podcast_recommendations_enabled(api.clone()).await? {
                    return Ok(None);
                }
                PodcastApi::new(api).home(&cursor).await.map(Some)
            })
            .await?;
        self.session.require_api(&token, &api)?;
        if !observation.current(&token, &api) {
            return Err(crate::Error::StaleResponse);
        }
        let enabled = result.is_some();
        let value = match result {
            None => {
                self.podcast_browse_playback.retire(&observation)?;
                self.session
                    .with_token(&token, |_| self.podcast_discovery.forget(epoch, &scope))?;
                PodcastHomePage {
                    recommendation_enabled: false,
                    blocks: Vec::new(),
                    continuation: None,
                }
            }
            Some(page) => PodcastHomePage {
                recommendation_enabled: true,
                blocks: page
                    .records
                    .iter()
                    .take(24)
                    .filter_map(|row| block(row, &self.podcast_browse_playback, &observation))
                    .collect(),
                continuation: if page.more {
                    Some(self.session.with_token(&token, |_| {
                        self.podcast_discovery.save(epoch, scope, page.cursor)
                    })?)
                } else {
                    None
                },
            },
        };
        self.session.require_api(&token, &api)?;
        if enabled && !observation.current(&token, &api) {
            return Err(crate::Error::StaleResponse);
        }
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
