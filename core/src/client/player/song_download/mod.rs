//! Purchased downloads from the exact native song currently playing.
mod authority;
mod destination;
mod dto;
mod start;

use super::{MusicClient, song_purchase::Rights as SongRights};
use crate::{
    Error, Result,
    account::resources::{AccountResources, ResourceKind},
    client::dto::SongDownloadState,
    membership::Quality,
};
use authority::{Authority, CredentialScope};
pub use dto::*;
use std::sync::{Arc, Mutex, MutexGuard};

use CurrentSongDownloadDecision as Decision;
use CurrentSongDownloadResult as Reply;

fn receipt() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[derive(Default)]
struct HostState {
    generation: u64,
    plan: Option<Arc<Plan>>,
}

#[derive(Default)]
pub(super) struct Host(Mutex<HostState>);

impl Host {
    fn begin(&self) -> Result<u64> {
        let mut host = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(old) = host.plan.take() {
            old.state()?.retired = true;
        }
        host.generation = host
            .generation
            .checked_add(1)
            .ok_or(Error::State("current song download generation exhausted"))?;
        Ok(host.generation)
    }

    fn publish(&self, generation: u64, plan: Arc<Plan>) -> Result<()> {
        let mut host = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if host.generation != generation {
            return Err(Error::StaleResponse);
        }
        host.plan = Some(plan);
        Ok(())
    }

    fn plan(&self, epoch: &str, target: &CurrentSongDownloadTarget) -> Result<Arc<Plan>> {
        self.0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .plan
            .as_ref()
            .filter(|plan| {
                plan.authority.token.generation.to_string() == epoch
                    && plan.authority.target == *target
            })
            .cloned()
            .ok_or(Error::StaleResponse)
    }
}

struct Plan {
    authority: Authority,
    receipt: String,
    state: Mutex<PlanState>,
    operation: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct PlanState {
    retired: bool,
    choice: Option<Choice>,
}

struct Choice {
    proof: ChoiceProof,
    destinations: Vec<(String, String)>,
    attempt: Option<destination::Attempt>,
    consumed: bool,
}

#[derive(Clone)]
struct ChoiceProof {
    receipt: String,
    quality: Quality,
    admission: Decision,
}

impl PlanState {
    fn active(&self) -> Result<()> {
        if self.retired {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    fn choice(&self, id: &str) -> Result<&Choice> {
        self.active()?;
        self.choice
            .as_ref()
            .filter(|choice| !choice.consumed && choice.proof.receipt == id)
            .ok_or(Error::StaleResponse)
    }

    fn choice_mut(&mut self, id: &str) -> Result<&mut Choice> {
        self.choice(id)?;
        Ok(self.choice.as_mut().expect("validated choice"))
    }
}

impl Plan {
    fn new(authority: Authority) -> Self {
        Self {
            authority,
            receipt: receipt(),
            state: Default::default(),
            operation: Default::default(),
        }
    }

    fn state(&self) -> Result<MutexGuard<'_, PlanState>> {
        self.state.lock().map_err(|_| Error::LockPoisoned)
    }

    fn check_choice(&self, choice: &ChoiceProof, scope: CredentialScope) -> Result<()> {
        self.state()?.choice(&choice.receipt)?;
        if !self.authority.checked(scope)?.contains(&choice.quality) {
            return Err(Error::StaleResponse);
        }
        self.state()?.choice(&choice.receipt)?;
        Ok(())
    }

    fn cancel(&self, id: &str) -> Result<()> {
        let mut state = self.state()?;
        state.active()?;
        if id != self.receipt {
            state.choice(id)?;
        }
        state.retired = true;
        Ok(())
    }

    fn reply(&self, epoch: &str, result: Reply) -> CurrentSongDownloadReply {
        reply(
            epoch,
            &self.authority.target,
            Some(self.authority.song.clone()),
            result,
        )
    }
}

fn reply(
    epoch: &str,
    target: &CurrentSongDownloadTarget,
    track_id: Option<String>,
    result: Reply,
) -> CurrentSongDownloadReply {
    CurrentSongDownloadReply {
        epoch: epoch.into(),
        play_id: target.play_id.clone(),
        entry_id: target.entry_id.clone(),
        track_id,
        result,
    }
}

fn plan_result(decision: Decision) -> Reply {
    Reply::Plan {
        decision,
        offer: None,
        purchased_qualities: Vec::new(),
    }
}

fn membership(
    value: (
        Option<bool>,
        Option<crate::account::resources::MutationRecord>,
    ),
) -> Decision {
    let (liked, pending) = value;
    match liked.filter(|_| !pending.is_some_and(|record| record.phase.pending())) {
        Some(true) => Decision::Ready,
        Some(false) => Decision::PlaylistRequired,
        None => Decision::MembershipUnknown,
    }
}

impl MusicClient {
    /// Does not accept a song ID from the renderer. Existing ordinary/download
    /// operations are separate; no proof can never reach this write path.
    pub async fn player_current_song_download(
        &self,
        epoch: &str,
        target: &CurrentSongDownloadTarget,
        command: CurrentSongDownloadCommand,
    ) -> Result<CurrentSongDownloadReply> {
        use CurrentSongDownloadCommand as C;
        if matches!(command, C::Prepare) {
            return self.prepare_current_song_download(epoch, target).await;
        }
        self.player_token(epoch)?;
        let plan = self.player.song_download.plan(epoch, target)?;
        if let C::Cancel { receipt } = command {
            plan.cancel(&receipt)?;
            return Ok(plan.reply(epoch, Reply::Cancelled));
        }
        let _operation = plan.operation.try_lock().map_err(|_| Error::MutationBusy)?;
        let result = match command {
            C::Prepare | C::Cancel { .. } => unreachable!("handled before acquiring the operation"),
            C::Select { offer, quality } => {
                self.select_current_download_quality(epoch, &plan, &offer, quality)
                    .await?
            }
            C::Start {
                selection,
                continuation,
                options,
            } => {
                self.start_current_download(
                    epoch,
                    &plan,
                    &selection,
                    continuation.as_deref(),
                    options,
                )
                .await?
            }
            other => {
                self.current_download_destination(epoch, &plan, other)
                    .await?
            }
        };
        Ok(plan.reply(epoch, result))
    }

    /// M/31 opens only the fixed public M/40 help page for this live offer.
    pub fn current_song_purchased_quality_guide(
        &self,
        epoch: &str,
        target: &CurrentSongDownloadTarget,
        offer: &str,
    ) -> Result<&'static str> {
        self.player_token(epoch)?;
        let plan = self.player.song_download.plan(epoch, target)?;
        if plan.receipt != offer {
            return Err(Error::StaleResponse);
        }
        plan.state()?.active()?;
        let guide = plan.authority.guide()?;
        plan.state()?.active()?;
        Ok(guide)
    }

    async fn prepare_current_song_download(
        &self,
        epoch: &str,
        target: &CurrentSongDownloadTarget,
    ) -> Result<CurrentSongDownloadReply> {
        let previous = self.player.song_download.plan(epoch, target).ok();
        let _previous = previous
            .as_ref()
            .map(|plan| plan.operation.try_lock())
            .transpose()
            .map_err(|_| Error::MutationBusy)?;
        if let Some(previous) = &previous {
            let attempt = previous
                .state()?
                .choice
                .as_ref()
                .and_then(|choice| choice.attempt.clone());
            if let Some(attempt) = attempt
                && attempt.pending(
                    &crate::playlist::PlaylistService::new(self.session.clone())?,
                    &previous.authority.token,
                )?
            {
                return Err(Error::MutationUncertain);
            }
        }
        let mut authority = self.bind_current_song_download(epoch, target)?;
        let generation = self.player.song_download.begin()?;
        if authority.is_none() {
            self.observe_current_song_download(epoch, target).await?;
            authority = self.bind_current_song_download(epoch, target)?;
        }
        let Some(authority) = authority else {
            return Ok(reply(
                epoch,
                target,
                None,
                plan_result(Decision::Unavailable),
            ));
        };
        let unavailable = self.current_download_duplicate(epoch, &authority).await?;
        let qualities = authority.checked(CredentialScope::Download)?;
        let decision = if let Some(status) = unavailable {
            status
        } else {
            match authority.rights {
                SongRights::Ordinary => Decision::Ordinary,
                SongRights::Purchased(_) if !qualities.is_empty() => {
                    Decision::PurchasedQualityRequired
                }
                SongRights::Purchased(_) => Decision::Unavailable,
            }
        };
        let song = authority.song.clone();
        let result = if decision == Decision::PurchasedQualityRequired {
            let plan = Arc::new(Plan::new(authority));
            let offer = plan.receipt.clone();
            self.player.song_download.publish(generation, plan)?;
            Reply::Plan {
                decision,
                offer: Some(offer),
                purchased_qualities: qualities,
            }
        } else {
            plan_result(decision)
        };
        Ok(reply(epoch, target, Some(song), result))
    }

    async fn select_current_download_quality(
        &self,
        epoch: &str,
        plan: &Plan,
        offer: &str,
        quality: Quality,
    ) -> Result<Reply> {
        if offer != plan.receipt {
            return Err(Error::StaleResponse);
        }
        plan.state()?.active()?;
        if !plan
            .authority
            .checked(CredentialScope::Download)?
            .contains(&quality)
        {
            return Err(Error::InvalidInput(
                "quality is not offered for this purchase",
            ));
        }
        let mut decision = self
            .current_download_decision(epoch, &plan.authority)
            .await?;
        if decision == Decision::PlaylistRequired && !plan.authority.collectable {
            decision = Decision::Unavailable;
        }
        if !plan
            .authority
            .checked(CredentialScope::Download)?
            .contains(&quality)
        {
            return Err(Error::StaleResponse);
        }
        let mut state = plan.state()?;
        state.active()?;
        let selection = if matches!(decision, Decision::Ready | Decision::PlaylistRequired) {
            if let Some(old) = &state.choice {
                if old.attempt.is_some() || old.consumed {
                    return Err(Error::MutationUncertain);
                }
                if old.proof.quality == quality && old.proof.admission == decision {
                    return Ok(Reply::Selection {
                        quality,
                        decision,
                        selection: Some(old.proof.receipt.clone()),
                    });
                }
            }
            let id = receipt();
            state.choice = Some(Choice {
                proof: ChoiceProof {
                    receipt: id.clone(),
                    quality,
                    admission: decision,
                },
                destinations: Vec::new(),
                attempt: None,
                consumed: false,
            });
            Some(id)
        } else {
            // A newly observed duplicate/unknown cannot leave an earlier
            // unsubmitted choice available for another Start.
            if state
                .choice
                .as_ref()
                .is_some_and(|choice| choice.attempt.is_none())
            {
                state.choice = None;
            }
            None
        };
        Ok(Reply::Selection {
            quality,
            decision,
            selection,
        })
    }

    async fn current_download_duplicate(
        &self,
        epoch: &str,
        authority: &Authority,
    ) -> Result<Option<Decision>> {
        let state = self.song_download_state(epoch, &authority.song).await?;
        match state {
            SongDownloadState::Complete => return Ok(Some(Decision::Complete)),
            SongDownloadState::Waiting => return Ok(Some(Decision::Waiting)),
            SongDownloadState::Working => return Ok(Some(Decision::Working)),
            SongDownloadState::Paused | SongDownloadState::Failed => {
                return Ok(Some(Decision::ExistingTask));
            }
            SongDownloadState::None => {}
        }
        let local = self.local.context().await?;
        let session = self.session.clone();
        let token = authority.token.clone();
        let id = authority.song.clone();
        tokio::task::spawn_blocking(move || {
            let matches = local.library.files_for_online_song(&session, &token, &id)?;
            Ok((!matches.is_empty()).then_some(Decision::Complete))
        })
        .await
        .map_err(|_| Error::State("current download admission worker stopped"))?
    }

    async fn current_download_decision(
        &self,
        epoch: &str,
        authority: &Authority,
    ) -> Result<Decision> {
        if let Some(duplicate) = self.current_download_duplicate(epoch, authority).await? {
            return Ok(duplicate);
        }
        Ok(membership(
            AccountResources::new(self.session.clone())?.membership_for(
                &authority.token,
                ResourceKind::Liked,
                &authority.song,
            )?,
        ))
    }
}
