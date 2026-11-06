//! Current-episode "听了该声音的人也在听"; M/69, M/109, M/182 and M/283.
use super::{
    MusicClient, PlaybackSnapshot, PlaybackStatus, vinyl_similar::stale_credentials, voice_detail,
};
use crate::{
    Error, Result,
    account::SessionToken,
    protocol::{
        api::{Api, VinylVoiceRecommendationsRead, identifier, validate_response},
        artwork,
        wire::serialization::js_truthy,
    },
};
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex, Weak};

const PAGE_SIZE: usize = 6;
const MAX_ROWS: usize = 120;
const MAX_ID: usize = 32;
const MAX_TEXT: usize = 1024;
const MAX_URL: usize = 2048;
const MAX_JS_INTEGER: u64 = (1 << 53) - 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylVoiceRecommendationCard {
    /// The response row's own id, sent directly to /audioDetail/:id by M/182.
    pub radio_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    pub play_count: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylVoiceRecommendationsView {
    pub play_id: String,
    pub voice_id: String,
    pub receipt: String,
    /// One based, or zero for an empty shelf.
    pub page: usize,
    pub page_count: usize,
    pub cards: Vec<VinylVoiceRecommendationCard>,
}

#[derive(PartialEq, Eq)]
struct Seed {
    voice_id: String,
    song_id: String,
    entry_id: String,
}

fn seed(snapshot: &PlaybackSnapshot, token: &SessionToken, play_id: &str) -> Result<Seed> {
    if play_id.is_empty()
        || &snapshot.account != token
        || snapshot.play_id.as_deref() != Some(play_id)
        || !matches!(
            snapshot.state,
            PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
        )
    {
        return Err(Error::StaleResponse);
    }
    let entry = snapshot.selected.as_ref().ok_or(Error::StaleResponse)?;
    // M/23 and M/308 classify ordinary/Scene FM voices and downloaded
    // voice origins. An ordinary indexed file is not an episode producer.
    let (voice_id, song_id, _) =
        voice_detail::bound_episode(&entry.resource).ok_or(Error::StaleResponse)?;
    Ok(Seed {
        voice_id,
        song_id,
        entry_id: entry.entry_id.clone(),
    })
}

fn project(reply: Value) -> Result<Vec<VinylVoiceRecommendationCard>> {
    let reply = if reply.is_object() && !js_truthy(&reply["code"]) {
        reply
    } else {
        validate_response(VinylVoiceRecommendationsRead::PATH, reply, &[200])?
    };
    let data = &reply["data"];
    let rows = match data {
        Value::Array(rows) if rows.len() <= MAX_ROWS => rows.as_slice(),
        Value::Object(_) => std::slice::from_ref(data),
        _ => return Err(Error::Response("invalid vinyl voice recommendation rows")),
    };
    let mut cards = Vec::with_capacity(rows.len());
    for row in rows {
        if !row.is_object() || row["id"].as_str().is_some_and(|id| id.len() > MAX_ID) {
            return Err(Error::Response("invalid vinyl voice recommendation target"));
        }
        let radio_id = identifier(&row["id"])
            .map_err(|_| Error::Response("invalid vinyl voice recommendation target"))?;
        let name = match &row["name"] {
            Value::Null => "",
            Value::String(name)
                if name.len() <= MAX_TEXT && !name.chars().any(char::is_control) =>
            {
                name
            }
            _ => return Err(Error::Response("invalid vinyl voice recommendation name")),
        };
        let image = &row["picUrl"];
        let cover_url = image
            .as_str()
            .filter(|url| url.len() <= MAX_URL)
            .and_then(|_| artwork::square(image));
        let count = &row["playCount"];
        let play_count = count
            .as_u64()
            .or_else(|| count.as_str()?.parse().ok())
            .filter(|count| *count <= MAX_JS_INTEGER)
            .unwrap_or(0);
        cards.push(VinylVoiceRecommendationCard {
            radio_id,
            name: name.into(),
            cover_url,
            play_count,
        });
    }
    // M/69 removes a final group of one to three rows, but leaves
    // groups of four or five and every complete six. Preserve wire order.
    let tail = cards.len() % PAGE_SIZE;
    if (1..4).contains(&tail) {
        cards.truncate(cards.len() - tail);
    }
    Ok(cards)
}

struct Shelf {
    token: SessionToken,
    api: Weak<dyn Api>,
    revision: String,
    play_id: String,
    seed: Seed,
    receipt: String,
    cards: Vec<VinylVoiceRecommendationCard>,
    index: usize,
}

impl Shelf {
    fn view(&self) -> VinylVoiceRecommendationsView {
        let page_count = self.cards.len().div_ceil(PAGE_SIZE);
        let from = self.index * PAGE_SIZE;
        let to = (from + PAGE_SIZE).min(self.cards.len());
        VinylVoiceRecommendationsView {
            play_id: self.play_id.clone(),
            voice_id: self.seed.voice_id.clone(),
            receipt: self.receipt.clone(),
            page: if page_count == 0 { 0 } else { self.index + 1 },
            page_count,
            cards: self.cards[from..to].to_vec(),
        }
    }

    fn next(&mut self) -> VinylVoiceRecommendationsView {
        let page_count = self.cards.len().div_ceil(PAGE_SIZE);
        if page_count > 1 {
            self.index = (self.index + 1) % page_count;
        }
        self.view()
    }
}

#[derive(Default)]
struct State {
    serial: u64,
    shelf: Option<Shelf>,
}

#[derive(Default)]
pub(super) struct Host(Mutex<State>);

impl Host {
    fn begin(&self, check: impl FnOnce() -> Result<()>) -> Result<u64> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        // Fence before replacing the latest ticket. An older account/task
        // cannot supersede a new account read while acquiring this mutex.
        check()?;
        state.serial = state
            .serial
            .checked_add(1)
            .ok_or(Error::State("vinyl voice recommendation reads exhausted"))?;
        Ok(state.serial)
    }

    fn check_read(&self, serial: u64) -> Result<()> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.serial != serial {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

impl MusicClient {
    pub async fn player_vinyl_voice_recommendations(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<VinylVoiceRecommendationsView> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let current = seed(&controller.snapshot(), &token, play_id)?;
        let read = VinylVoiceRecommendationsRead::new(&current.voice_id)?;
        let room = self.together_detached_guard(&token);
        let check_play = || {
            self.session
                .with_verified_profile(&token, &api, |_, _| Ok(()))?;
            if self.login_pending()?
                || room.is_cancelled()
                || seed(&controller.snapshot(), &token, play_id)? != current
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check_play()?;
        // Verify the current native PC credentials without fetching an AB
        // assignment; VoicePage has no RcmdCard cohort gate.
        let revision = api
            .verified_pc_experiment_revision()
            .map_err(stale_credentials)?
            .ok_or(Error::State(
                "vinyl voice recommendations require signed native PC credentials",
            ))?;
        let check_revision = || {
            if api
                .verified_pc_experiment_revision()
                .map_err(stale_credentials)?
                .as_deref()
                != Some(revision.as_str())
            {
                return Err(Error::StaleResponse);
            }
            check_play()
        };
        let host = &self.player.voice_recommendations;
        let serial = host.begin(&check_revision)?;
        let permission = || {
            check_revision()?;
            host.check_read(serial)
        };
        let reply = api
            .request_vinyl_voice_recommendations(read, &revision, &permission)
            .await;
        permission()?;
        let cards = project(reply.map_err(stale_credentials)?)?;
        let mut state = host.0.lock().map_err(|_| Error::LockPoisoned)?;
        check_revision()?;
        if state.serial != serial {
            return Err(Error::StaleResponse);
        }
        let shelf = Shelf {
            token,
            api: Arc::downgrade(&api),
            revision,
            play_id: play_id.into(),
            seed: current,
            receipt: uuid::Uuid::new_v4().simple().to_string(),
            cards,
            index: 0,
        };
        let view = shelf.view();
        state.shelf = Some(shelf);
        Ok(view)
    }

    pub fn player_vinyl_voice_recommendations_next(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
    ) -> Result<VinylVoiceRecommendationsView> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let room = self.together_detached_guard(&token);
        let mut state = self
            .player
            .voice_recommendations
            .0
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        let shelf = state.shelf.as_mut().ok_or(Error::StaleResponse)?;
        self.session
            .with_verified_profile(&token, &api, |_, _| Ok(()))?;
        if shelf.receipt != receipt
            || shelf.play_id != play_id
            || shelf.token != token
            || !shelf
                .api
                .upgrade()
                .is_some_and(|bound| Arc::ptr_eq(&bound, &api))
            || self.login_pending()?
            || room.is_cancelled()
            || seed(&controller.snapshot(), &token, play_id)? != shelf.seed
            || api
                .verified_pc_experiment_revision()
                .map_err(stale_credentials)?
                .as_deref()
                != Some(shelf.revision.as_str())
        {
            return Err(Error::StaleResponse);
        }
        Ok(shelf.next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tail_is_applied_to_all_original_row_counts_and_keeps_source_order() {
        for (count, kept) in [
            (0, 0),
            (1, 0),
            (3, 0),
            (4, 4),
            (5, 5),
            (6, 6),
            (9, 6),
            (10, 10),
            (11, 11),
            (12, 12),
            (15, 12),
            (120, 120),
        ] {
            let data: Vec<_> = (1..=count)
                .map(|id| json!({"id":id,"name":format!("radio {id}")}))
                .collect();
            let cards = project(json!({"code":200,"data":data})).unwrap();
            assert_eq!(cards.len(), kept, "{count}");
            assert_eq!(
                cards
                    .iter()
                    .map(|card| card.radio_id.parse().unwrap())
                    .collect::<Vec<usize>>(),
                (1..=kept).collect::<Vec<_>>()
            );
        }
        assert!(project(json!({"code":200,"data":vec![json!({"id":1}); 121]})).is_err());
        assert!(project(json!({"code":200,"data":[{"radioId":1,"programId":2}]})).is_err());
        assert!(project(json!({"code":200,"data":null})).is_err());
        assert!(project(json!({"code":200,"data":{"id":"https://evil.invalid/123"}})).is_err());
        assert!(
            project(json!({"code":200,"data":{"id":1}}))
                .unwrap()
                .is_empty()
        );
    }
}
