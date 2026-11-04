//! Official PC classic discovery homepage. The batch is read-only and its
//! resource payloads are projected before crossing the account/desktop boundary.
mod project;
mod rankings;
mod request;

use super::{MusicClient, dto::AccountData};
use crate::{Result, account::AccountApi};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryHomePage {
    recommendation_enabled: bool,
    sections: Vec<DiscoveryHomeSection>,
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
    Fm,
    Daily,
}

impl MusicClient {
    pub async fn discovery_home(&self, epoch: &str) -> Result<AccountData<DiscoveryHomePage>> {
        let token = self.playlist_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                if AccountApi::new(api.clone()).settings().await?["personalServiceSetting"] == 2 {
                    return Ok(DiscoveryHomePage {
                        recommendation_enabled: false,
                        sections: Vec::new(),
                    });
                }
                let (feed, recent) = tokio::join!(request::feed(api.clone()), request::recent(api));
                let feed = feed?;
                let recent = request::optional(recent)?;
                let sections = project::home(&feed, recent.as_ref())?;
                Ok(DiscoveryHomePage {
                    recommendation_enabled: true,
                    sections,
                })
            })
            .await?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
