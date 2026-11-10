//! M/3085's explicit purchased tier, frozen before any destination can write.
use super::*;
use crate::{
    account::resources::{AccountResources, ResourceKind},
    client::{
        downloads::purchased::{purchased_quality_guide_url, vivid_available},
        pc_experiments::PcExperimentHost,
    },
    storage::StateTransaction,
};

type Decision = SearchGroupDownloadDecision;
type Reply = SearchGroupDownloadReply;

pub(super) struct Choice {
    pub(super) quality: Quality,
    destination: bool,
}

pub(super) fn common_existing_quality(
    existing: &BTreeMap<String, ExistingSongAdmission>,
) -> Option<Quality> {
    let quality = existing.values().next()?.options().quality;
    existing
        .values()
        .all(|task| task.has_bound_prefix() && task.options().quality == quality)
        .then_some(quality)
}

pub(super) fn vivid(experiments: &PcExperimentHost, observed: &Observed) -> Result<bool> {
    let groups = experiments.observed_groups(&observed.owner, &observed.api, &observed.revision)?;
    Ok(vivid_available(groups.as_ref()))
}

pub(super) fn check_quality(
    experiments: &PcExperimentHost,
    observed: &Observed,
    quality: Option<Quality>,
) -> Result<()> {
    if quality == Some(Quality::Vivid) && !vivid(experiments, observed)? {
        return Err(Error::State("purchased vivid quality assignment changed"));
    }
    Ok(())
}

pub(super) fn anchor_membership(
    tx: &StateTransaction<'_>,
    owner: &str,
    anchor: &str,
) -> Result<Decision> {
    let (liked, pending) = AccountResources::membership_in(tx, owner, ResourceKind::Liked, anchor)?;
    Ok(
        match liked.filter(|_| !pending.is_some_and(|record| record.phase.pending())) {
            Some(true) => Decision::Ready,
            Some(false) => Decision::PlaylistRequired,
            None => Decision::MembershipUnknown,
        },
    )
}

impl Plan {
    pub(super) fn permits_destination(&self) -> bool {
        self.purpose == Decision::PlaylistRequired
            || self.quality.get().is_some_and(|choice| choice.destination)
    }
}

impl MusicClient {
    pub fn search_group_purchased_quality_guide(
        &self,
        epoch: &str,
        receipt: &str,
    ) -> Result<&'static str> {
        self.auth.pending_guard()()?;
        let token = self.player_token(epoch)?;
        let host = &self.search.group_downloads;
        let plan = host.use_plan(&token, receipt, false)?;
        let observed = &plan.observed;
        authorization(&self.session, observed)?;
        host.current(observed)?;
        let vivid = vivid(&self.pc_experiments, observed)?;
        if plan.quality.get().is_some()
            || plan.busy.load(Ordering::Acquire)
            || !plan
                .purchased_qualities
                .iter()
                .any(|quality| *quality != Quality::Vivid || vivid)
        {
            return Err(Error::State(
                "search plan has no current purchased quality chooser",
            ));
        }
        Ok(purchased_quality_guide_url(vivid))
    }

    pub(super) async fn confirm_search_group_quality(
        &self,
        epoch: &str,
        lease: Lease,
        quality: Quality,
    ) -> Result<Reply> {
        let plan = lease.0.clone();
        let observed = &plan.observed;
        let check = || {
            self.auth.pending_guard()()?;
            authorization(&self.session, observed)?;
            self.search.group_downloads.current(observed)
        };
        check()?;
        if !plan.purchased_qualities.contains(&quality)
            || plan
                .quality
                .get()
                .is_some_and(|choice| choice.quality != quality)
        {
            return Err(Error::InvalidInput(
                "search download requires the offered purchased quality",
            ));
        }
        if let Some(ids) = plan.committed.get() {
            return Ok(accepted(epoch, &plan, ids.clone()));
        }
        if let Some(required_quality) = plan.existing_purchased_quality
            && quality != required_quality
        {
            return Ok(Reply::ExistingQualityMismatch {
                epoch: epoch.into(),
                receipt: plan.receipt.clone(),
                required_quality,
            });
        }
        check_quality(&self.pc_experiments, observed, Some(quality))?;
        let confirmed = |decision, has_destination: bool| Reply::QualityConfirmed {
            epoch: epoch.into(),
            decision,
            receipt: has_destination.then(|| plan.receipt.clone()),
            quality,
        };
        let store = self.session.persistent_store()?;
        let membership = self.session.with_token(&observed.owner, |owner| {
            store.read_snapshot(|tx| {
                for task in plan.existing.values() {
                    task.check(tx, owner)?;
                }
                anchor_membership(tx, owner, &observed.anchor)
            })
        })?;
        if plan.quality.get().is_some_and(|choice| choice.destination) {
            if membership != Decision::PlaylistRequired {
                return Err(Error::StaleResponse);
            }
            return Ok(confirmed(Decision::PlaylistRequired, true));
        }
        if plan.quality.get().is_some() && membership != Decision::Ready {
            return Err(Error::StaleResponse);
        }
        match membership {
            Decision::Ready => {
                if !self.configured_download_directory_available(epoch).await? {
                    check()?;
                    return Ok(Reply::DirectoryRequired);
                }
                check()?;
                check_quality(&self.pc_experiments, observed, Some(quality))?;
                let _ = plan.quality.set(Choice {
                    quality,
                    destination: false,
                });
                self.enqueue_search_download(
                    epoch,
                    lease,
                    plan.songs.clone(),
                    SongBatchAdmission::LikedAnchor(observed.anchor.clone()),
                    false,
                )
                .await
            }
            Decision::PlaylistRequired => {
                if plan.collection.is_empty()
                    || destination::authorize_collection(observed).is_err()
                {
                    return Ok(confirmed(Decision::PlaylistUnavailable, false));
                }
                let _ = plan.quality.set(Choice {
                    quality,
                    destination: true,
                });
                Ok(confirmed(Decision::PlaylistRequired, true))
            }
            _ => Ok(confirmed(membership, false)),
        }
    }
}
