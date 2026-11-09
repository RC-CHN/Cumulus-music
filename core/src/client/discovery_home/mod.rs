//! Official PC classic discovery homepage. The batch is read-only and its
//! resource payloads are projected before crossing the account/desktop boundary.
mod project;
mod rankings;
pub(super) mod recall;
mod request;

use super::{
    MusicClient,
    dto::AccountData,
    style_home::{DiscoveryStyleSeeds, StyleSeedHandle},
};
use crate::{Result, account::AccountApi};
pub use recall::RecallGuide;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryHomePage {
    recommendation_enabled: bool,
    sections: Vec<DiscoveryHomeSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recall_guide: Option<RecallGuide>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveryHomeSection {
    kind: &'static str,
    title: String,
    cards: Vec<DiscoveryHomeCard>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveryHomeCard {
    title: String,
    subtitle: Option<String>,
    cover_url: Option<String>,
    target: DiscoveryHomeTarget,
}

#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DiscoveryHomeTarget {
    Song { id: String },
    Playlist { id: String },
    Album { id: String },
    Podcast { id: String },
    Voice { id: String },
    Style { seed: StyleSeedHandle },
    Fm,
    Daily,
    RecentStyle { id: String },
}

impl MusicClient {
    pub async fn discovery_home(&self, epoch: &str) -> Result<AccountData<DiscoveryHomePage>> {
        let token = self.playlist_token(epoch)?;
        let (request, recall_generation) = self.session.with_token(&token, |_| {
            Ok((
                self.style_home.begin_discovery()?,
                self.home_recall.begin()?,
            ))
        })?;
        let owner = token.clone();
        let (value, seeds) = self
            .session
            .run_for(&token, |api, _| async move {
                let mut seeds = DiscoveryStyleSeeds::default();
                let settings = AccountApi::new(api.clone()).settings().await?;
                if settings["personalServiceSetting"] == 2 {
                    return Ok((
                        DiscoveryHomePage {
                            recommendation_enabled: false,
                            sections: Vec::new(),
                            recall_guide: None,
                        },
                        seeds,
                    ));
                }
                let assignment = (settings["personalServiceSetting"].as_i64() == Some(0))
                    .then(|| self.home_recall_assignment(&owner, &api))
                    .flatten();
                let capture = if assignment.as_ref().is_some_and(|value| value.prefill) {
                    // Metadata prefill is optional: audio initialization or a
                    // busy player must not take down ordinary Home results.
                    self.capture_home_recall(&owner).await.ok().flatten()
                } else {
                    None
                };
                let hot = assignment.as_ref().is_some_and(|value| value.hot);
                let (feed, recent) = tokio::join!(
                    request::feed(api.clone(), hot),
                    request::recent(api.clone())
                );
                let feed = feed?;
                let recent = request::optional(recent)?;
                let sections = project::home(&feed, recent.as_ref(), &mut seeds)?;
                let recall_guide = if let Some((assignment, capture)) = assignment.zip(capture) {
                    self.try_home_recall(&owner, api, recall_generation, assignment, capture, &feed)
                        .await
                        .ok()
                        .flatten()
                } else {
                    None
                };
                Ok((
                    DiscoveryHomePage {
                        recommendation_enabled: true,
                        sections,
                        recall_guide,
                    },
                    seeds,
                ))
            })
            .await?;
        self.session.with_token(&token, |_| {
            self.style_home.publish_discovery(&token, request, seeds)?;
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
