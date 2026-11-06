use super::MusicClient;
use crate::{
    Error, Result,
    client::{dto::AccountData, mapping::artwork},
    podcast::Radio,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;

const RECOMMENDATIONS: &str = "/api/djradio/my/radio/recommend";
const MAX_CARDS: usize = 6;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastSubscriptionRecommendation {
    pub id: String,
    pub name: String,
    pub cover_url: Option<String>,
    pub play_count: Option<u64>,
    pub unavailable: bool,
}

fn project(body: &Value) -> Result<Vec<PodcastSubscriptionRecommendation>> {
    if body["data"].is_null() {
        return Ok(Vec::new());
    }
    let rows = body["data"].as_array().ok_or(Error::Response(
        "invalid subscribed podcast recommendations",
    ))?;
    let mut seen = HashSet::new();
    // M3108's GridCarousel exposes only the first six ordered slots.
    Ok(rows
        .iter()
        .take(MAX_CARDS)
        .filter_map(|row| {
            let radio = Radio::from_official(row).ok()?;
            if radio.name.trim().is_empty() || !seen.insert(radio.id.clone()) {
                return None;
            }
            let play_count = row["playCount"]
                .as_u64()
                .or_else(|| row["playCount"].as_str()?.parse().ok())
                .filter(|&count| count <= 9_007_199_254_740_991);
            Some(PodcastSubscriptionRecommendation {
                id: radio.id,
                name: radio.name,
                cover_url: artwork(&json!(radio.cover_url)),
                play_count,
                unavailable: row["grayDisplay"] == true || row["underShelf"] == true,
            })
        })
        .collect())
}

impl MusicClient {
    pub async fn podcast_subscription_recommendations(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Vec<PodcastSubscriptionRecommendation>>> {
        let token = self.podcast_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                // M15:1012 and :3607 use the no-cache strategy. The caller
                // retains the view in memory during the active visit.
                project(&api.call(RECOMMENDATIONS, json!({}), false, &[200]).await?)
            })
            .await?;
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
