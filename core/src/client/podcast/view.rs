use super::*;
use crate::client::mapping::{artwork, string};
use crate::podcast::{Episode, PodcastMutation, PodcastSnapshot, Radio};
fn creator(v: &Value) -> String {
    let d = if v["dj"].is_object() {
        &v["dj"]
    } else {
        &v["creator"]
    };
    d["nickname"]
        .as_str()
        .or_else(|| v["creatorName"].as_str())
        .unwrap_or("")
        .into()
}
pub(super) fn card(r: &Radio) -> PodcastCard {
    PodcastCard {
        id: r.id.clone(),
        name: r.name.clone(),
        cover_url: artwork(&json!(r.cover_url)),
        creator: creator(&r.raw),
        description: r.description.clone(),
        program_count: r.program_count,
        subscribed: r.subscribed,
        fee_type: r.raw["radioFeeType"].as_i64().unwrap_or(0),
        unavailable: r.raw["grayDisplay"] == true || r.raw["underShelf"] == true,
    }
}
pub(crate) fn episode(v: &Episode, liked: &[String]) -> EpisodeView {
    EpisodeView {
        id: v.id.clone(),
        name: v.name.clone(),
        cover_url: artwork(&json!(v.cover_url)),
        creator: creator(&v.raw),
        radio_id: v.radio.as_ref().map(|r| r.id.clone()),
        radio_name: v
            .radio
            .as_ref()
            .map(|r| r.name.clone())
            .unwrap_or_else(|| string(&v.raw["belongPodcastName"])),
        duration_ms: v.duration_ms,
        fee_type: v.raw["programFeeType"].as_i64().unwrap_or(0),
        liked: liked.contains(&v.id),
        unavailable: v.raw["grayDisplay"] == true,
    }
}
pub(super) fn operation(v: PodcastMutation) -> PodcastMutationView {
    PodcastMutationView {
        action: v.action,
        id: v.id,
        desired: v.desired,
        observed: v.observed,
        phase: v.phase,
        acknowledged: v.acknowledged,
    }
}
pub(super) fn page(
    epoch: &str,
    snapshot: PodcastSnapshot,
    query: PodcastQuery,
    liked_ids: Vec<String>,
    resume_enabled: Option<bool>,
    operations: Vec<PodcastMutationView>,
) -> Result<PodcastPage> {
    let keyword = query.keyword.trim().to_lowercase();
    let is_radio = matches!(query.scope, PodcastScope::Radio { .. });
    let cards = if is_radio {
        vec![]
    } else {
        snapshot
            .radios
            .iter()
            .map(card)
            .filter(|c| {
                format!("{} {}", c.name, c.creator)
                    .to_lowercase()
                    .contains(&keyword)
            })
            .collect::<Vec<_>>()
    };
    let episodes = snapshot
        .voices
        .iter()
        .map(|v| episode(v, &liked_ids))
        .filter(|v| {
            format!("{} {}", v.name, v.creator)
                .to_lowercase()
                .contains(&keyword)
        })
        .collect::<Vec<_>>();
    let total = if matches!(
        query.scope,
        PodcastScope::Liked | PodcastScope::Radio { .. }
    ) {
        episodes.len()
    } else {
        cards.len()
    };
    Ok(PodcastPage {
        epoch: epoch.into(),
        scope: query.scope,
        revision: snapshot.revision,
        cards: cards
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect(),
        episodes: episodes
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect(),
        total,
        offset: query.offset,
        remote_total: snapshot.remote_total,
        radio: if is_radio {
            snapshot.radios.first().map(card)
        } else {
            None
        },
        liked_ids,
        resume_enabled,
        operations,
    })
}
