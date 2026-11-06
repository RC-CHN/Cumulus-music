use super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    client::{dto::AccountData, mapping::artwork},
    playback::engine::Cancellation,
    podcast::Radio,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastSubscriptionRecommendations {
    pub receipt: String,
    pub cards: Vec<PodcastSubscriptionRecommendation>,
}

#[derive(Default)]
pub(in crate::client) struct Host {
    issued: AtomicU64,
    published: Arc<AtomicU64>,
    current: Mutex<Option<Snapshot>>,
}
struct Snapshot {
    token: SessionToken,
    receipt: String,
    sequence: u64,
    radios: Vec<Radio>,
}

fn project(body: &Value) -> Result<Vec<(PodcastSubscriptionRecommendation, Radio)>> {
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
            let mut radio = Radio::from_official(row).ok()?;
            if radio.name.trim().is_empty() || !seen.insert(radio.id.clone()) {
                return None;
            }
            let play_count = row["playCount"]
                .as_u64()
                .or_else(|| row["playCount"].as_str()?.parse().ok())
                .filter(|&count| count <= 9_007_199_254_740_991);
            let cover_url = artwork(&json!(radio.cover_url));
            // The receipt stores only data needed by the later playback
            // action; the original personalized response is not retained.
            radio.cover_url = cover_url.clone().unwrap_or_default();
            radio.raw = json!({"categoryId": row["categoryId"].as_i64()});
            Some((
                PodcastSubscriptionRecommendation {
                    id: radio.id.clone(),
                    name: radio.name.clone(),
                    cover_url,
                    play_count,
                    unavailable: row["grayDisplay"] == true || row["underShelf"] == true,
                },
                radio,
            ))
        })
        .collect())
}

impl MusicClient {
    pub async fn podcast_subscription_recommendations(
        &self,
        epoch: &str,
    ) -> Result<AccountData<PodcastSubscriptionRecommendations>> {
        let token = self.podcast_token(epoch)?;
        let host = &self.podcast_subscription_recommendations;
        let sequence = host.issued.fetch_add(1, Ordering::AcqRel) + 1;
        let rows = self
            .session
            .run_for(&token, |api, _| async move {
                // M15:1012 and :3607 use the no-cache strategy. The caller
                // retains the view in memory during the active visit.
                project(&api.call(RECOMMENDATIONS, json!({}), false, &[200]).await?)
            })
            .await?;
        let (cards, radios): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        self.session.with_token(&token, |_| {
            let mut current = host.current.lock().map_err(|_| Error::LockPoisoned)?;
            if host.issued.load(Ordering::Acquire) != sequence {
                return Err(Error::StaleResponse);
            }
            *current = Some(Snapshot {
                token: token.clone(),
                receipt: receipt.clone(),
                sequence,
                radios: radios
                    .into_iter()
                    .zip(&cards)
                    .filter_map(|(radio, card)| (!card.unavailable).then_some(radio))
                    .collect(),
            });
            host.published.store(sequence, Ordering::Release);
            Ok(())
        })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: PodcastSubscriptionRecommendations { receipt, cards },
        })
    }

    pub(in crate::client) fn subscription_recommendation(
        &self,
        token: &SessionToken,
        receipt: &str,
        radio_id: &str,
    ) -> Result<(Radio, Cancellation)> {
        let host = &self.podcast_subscription_recommendations;
        let current = host.current.lock().map_err(|_| Error::LockPoisoned)?;
        let snapshot = current
            .as_ref()
            .filter(|snapshot| &snapshot.token == token && snapshot.receipt == receipt)
            .ok_or(Error::StaleResponse)?;
        let radio = snapshot
            .radios
            .iter()
            .find(|radio| radio.id == radio_id)
            .cloned()
            .ok_or(Error::QueueChanged)?;
        let sequence = snapshot.sequence;
        let published = host.published.clone();
        Ok((
            radio,
            Cancellation::new(move || published.load(Ordering::Acquire) == sequence),
        ))
    }
}
