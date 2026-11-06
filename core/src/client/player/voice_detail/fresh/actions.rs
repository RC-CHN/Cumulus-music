//! Read-only routes and subscription authority from a signed, current VoicePage detail.
use super::{MusicClient, PlayerVoiceDetail, stale_credentials, voice_scope};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::engine::Cancellation,
    podcast::{FreshVinylSubscription, PodcastMutation, PodcastService},
    protocol::api::{Api, VinylPodcastSubscriptionRead},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex, Weak};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VinylVoiceDetailTarget {
    Creator,
    Radio,
}

#[derive(Default)]
pub(in crate::client::player) struct Host(Mutex<State>);

#[derive(Default)]
struct State {
    read: u64,
    receipt: Option<Arc<Receipt>>,
}

pub(super) struct Authority {
    pub(super) token: SessionToken,
    pub(super) api: Weak<dyn Api>,
    pub(super) revision: String,
    pub(super) play_id: String,
    pub(super) seed: voice_scope::VoicePlaybackSeed,
    pub(super) source: Value,
    pub(super) room: Cancellation,
}

pub(super) struct Receipt {
    id: String,
    authority: Authority,
    creator: Option<String>,
    radio: Option<RadioTarget>,
}

struct RadioTarget {
    id: String,
    owned_by_viewer: bool,
}

impl Receipt {
    pub(super) fn new(authority: Authority, detail: &PlayerVoiceDetail) -> Option<Arc<Self>> {
        let creator = detail
            .creator
            .as_ref()
            .and_then(|creator| creator.id.as_ref())
            .filter(|id| canonical_id(id))
            .cloned();
        let radio = detail
            .radio
            .as_ref()
            .filter(|radio| VinylPodcastSubscriptionRead::new(&radio.id).is_ok())
            .map(|radio| RadioTarget {
                id: radio.id.clone(),
                owned_by_viewer: radio.owned_by_viewer,
            });
        (creator.is_some() || radio.is_some()).then(|| {
            Arc::new(Self {
                id: uuid::Uuid::new_v4().simple().to_string(),
                authority,
                creator,
                radio,
            })
        })
    }

    pub(super) fn id(&self) -> &str {
        &self.id
    }
}

fn canonical_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && !id.starts_with('0')
        && id.bytes().all(|byte| byte.is_ascii_digit())
}

impl Host {
    pub(super) fn begin(&self, current: impl FnOnce() -> Result<()>) -> Result<u64> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        current()?;
        state.read = state
            .read
            .checked_add(1)
            .ok_or(Error::State("vinyl voice detail reads exhausted"))?;
        state.receipt = None;
        Ok(state.read)
    }

    pub(super) fn check_read(&self, read: u64) -> Result<()> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.read != read {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(super) fn install(
        &self,
        read: u64,
        receipt: Option<Arc<Receipt>>,
        current: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.read != read {
            return Err(Error::StaleResponse);
        }
        current()?;
        state.receipt = receipt;
        Ok(())
    }

    fn receipt(&self, token: &SessionToken, play_id: &str, id: &str) -> Result<Arc<Receipt>> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state
            .receipt
            .as_ref()
            .filter(|receipt| {
                receipt.id == id
                    && &receipt.authority.token == token
                    && receipt.authority.play_id == play_id
            })
            .cloned()
            .ok_or(Error::StaleResponse)
    }

    fn still_installed(&self, receipt: &Arc<Receipt>) -> Result<()> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if !state
            .receipt
            .as_ref()
            .is_some_and(|bound| Arc::ptr_eq(bound, receipt))
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

impl MusicClient {
    fn ensure_fresh_voice_action(&self, receipt: &Arc<Receipt>) -> Result<()> {
        let bound = &receipt.authority;
        let api = self.session.native_api(&bound.token)?;
        self.session
            .with_verified_profile(&bound.token, &api, |_, _| Ok(()))?;
        let snapshot = self
            .player
            .controller
            .get()
            .ok_or(Error::StaleResponse)?
            .snapshot();
        if self.login_pending()?
            || bound.room.is_cancelled()
            || !bound
                .api
                .upgrade()
                .is_some_and(|old| Arc::ptr_eq(&old, &api))
            || voice_scope::seed(&snapshot, &bound.token, &bound.play_id)? != bound.seed
            || snapshot.selected.as_ref().map(|entry| &entry.source) != Some(&bound.source)
            || api
                .vinyl_voice_action_revision()
                .map_err(stale_credentials)?
                .as_deref()
                != Some(bound.revision.as_str())
        {
            return Err(Error::StaleResponse);
        }
        self.player.voice_detail_read.still_installed(receipt)
    }

    fn fresh_voice_action(&self, epoch: &str, play_id: &str, id: &str) -> Result<Arc<Receipt>> {
        let token = self.player_token(epoch)?;
        let receipt = self.player.voice_detail_read.receipt(&token, play_id, id)?;
        self.ensure_fresh_voice_action(&receipt)?;
        Ok(receipt)
    }

    pub fn player_vinyl_voice_detail_target(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        target: VinylVoiceDetailTarget,
    ) -> Result<String> {
        let receipt = self.fresh_voice_action(epoch, play_id, receipt)?;
        match target {
            VinylVoiceDetailTarget::Creator => receipt.creator.as_ref(),
            VinylVoiceDetailTarget::Radio => receipt.radio.as_ref().map(|radio| &radio.id),
        }
        .cloned()
        .ok_or(Error::StaleResponse)
    }

    pub(in crate::client) async fn fresh_vinyl_podcast_operation(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        expected: Option<bool>,
    ) -> Result<PodcastMutation> {
        let receipt = self.fresh_voice_action(epoch, play_id, receipt)?;
        let radio = receipt.radio.as_ref().ok_or(Error::StaleResponse)?;
        if expected.is_some() && radio.owned_by_viewer {
            return Err(Error::InvalidInput("cannot subscribe to an owned podcast"));
        }
        let permission = || self.ensure_fresh_voice_action(&receipt);
        let binding = FreshVinylSubscription {
            revision: &receipt.authority.revision,
            permission: &permission,
        };
        let service = PodcastService::new(self.session.clone())?;
        let token = &receipt.authority.token;
        let result = match expected {
            Some(expected) => {
                service
                    .change_fresh_vinyl_subscription_for(token, &radio.id, expected, binding)
                    .await
            }
            None => {
                service
                    .reconcile_fresh_vinyl_subscription_for(token, &radio.id, binding)
                    .await
            }
        };
        permission()?;
        result
    }
}
