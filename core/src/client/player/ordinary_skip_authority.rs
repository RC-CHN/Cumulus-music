//! Native M119 + verified PC experiment admission shared by both M308 triggers.
use crate::{
    Error, Result,
    account::{AccountSession, SessionStatus, SessionToken},
    client::{
        focus::{MediaFocus, Owner},
        messages::PcMessagePollHost,
        pc_experiments::PcExperimentHost,
        together::RoomDetachment,
    },
    playback::{
        controller::{OrdinaryEofAuthority, VerifiedOrdinarySkip},
        engine::Cancellation,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone)]
pub(in crate::client) struct OrdinarySkipHost {
    session: Arc<AccountSession>,
    messages: Arc<PcMessagePollHost>,
    experiments: Arc<PcExperimentHost>,
    room: RoomDetachment,
}

impl OrdinarySkipHost {
    pub(in crate::client) fn new(
        session: Arc<AccountSession>,
        messages: Arc<PcMessagePollHost>,
        experiments: Arc<PcExperimentHost>,
        room: RoomDetachment,
    ) -> Self {
        Self {
            session,
            messages,
            experiments,
            room,
        }
    }

    pub(super) fn admit(
        &self,
        token: &SessionToken,
        intent: Cancellation,
    ) -> Result<Option<VerifiedOrdinarySkip>> {
        intent.check()?;
        self.session.check_generation(token)?;
        if token.uid.is_none()
            || self.session.snapshot()?.status != SessionStatus::Authenticated
            || !self.room.is_detached(token)?
        {
            return Ok(None);
        }
        let room = self.room.authority(token);
        let Some(network) = self.messages.playback_authority(token)? else {
            return Ok(None);
        };
        let api = self.session.native_api(token)?;
        let Some(credentials) = api.verified_pc_experiment_revision()? else {
            return Ok(None);
        };
        let Some((version, groups)) =
            self.experiments
                .observed_groups_with_version(token, &api, &credentials)?
        else {
            return Ok(None);
        };
        if groups.group("PH-PC-Blacklist") == Some("t1") {
            return Ok(None);
        }
        let skip = groups.skip_songs_assigned();
        let session = self.session.clone();
        let host = self.experiments.clone();
        let owner = token.clone();
        let authority = Cancellation::new(move || {
            !intent.is_cancelled()
                && !network.is_cancelled()
                && !room.is_cancelled()
                && session.require_api(&owner, &api).is_ok()
                && api
                    .verified_pc_experiment_revision()
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some(&credentials)
                && host
                    .observed_groups_with_version(&owner, &api, &credentials)
                    .is_ok_and(|current| {
                        current.is_some_and(|(at, groups)| {
                            at == version
                                && groups.skip_songs_assigned() == skip
                                && groups.group("PH-PC-Blacklist") != Some("t1")
                        })
                    })
        });
        Ok(Some(VerifiedOrdinarySkip { groups, authority }))
    }

    pub(super) fn eof(
        &self,
        focus: Arc<MediaFocus>,
        selection: Arc<AtomicU64>,
    ) -> OrdinaryEofAuthority {
        let host = self.clone();
        Arc::new(move |token| {
            let observed = focus.snapshot()?;
            if observed.owner != Some(Owner::Audio) {
                return Err(Error::StaleResponse);
            }
            let generation = selection.load(Ordering::Acquire);
            let focus = focus.clone();
            let selection = selection.clone();
            let intent = Cancellation::new(move || {
                selection.load(Ordering::Acquire) == generation
                    && focus.check_snapshot(observed).is_ok()
            });
            let verified = host.admit(token, intent)?;
            // An absent group is unresolved for this new EOF branch.
            Ok(verified.filter(|value| value.groups.group("PH-PC-skipSongs2").is_some()))
        })
    }
}
