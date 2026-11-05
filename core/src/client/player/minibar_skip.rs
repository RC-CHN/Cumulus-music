//! Only the official main bottom bar's explicit forward/back triggers.
use super::MusicClient;
use crate::{
    Result,
    account::{SessionStatus, SessionToken},
    playback::{
        Step,
        controller::{PlaybackController, PlaybackSnapshot},
        engine::Cancellation,
        preflight::OrdinaryQueueEnvironment,
    },
};

impl MusicClient {
    pub(super) async fn try_minibar_skip(
        &self,
        token: &SessionToken,
        controller: &PlaybackController,
        current: &PlaybackSnapshot,
        direction: Step,
        transport: &Cancellation,
    ) -> Result<Option<PlaybackSnapshot>> {
        transport.check()?;
        if current.fm
            || current.intelligence.is_some()
            || token.uid.is_none()
            || self.session.snapshot()?.status != SessionStatus::Authenticated
            || self.together_attached(token, None)?
        {
            return Ok(None);
        }
        let Some(network) = self.pc_playback_connectivity(token)? else {
            return Ok(None);
        };
        let api = self.session.native_api(token)?;
        let Some(credentials) = api.verified_pc_experiment_revision()? else {
            return Ok(None);
        };
        let Some((version, groups)) =
            self.pc_experiments
                .observed_groups_with_version(token, &api, &credentials)?
        else {
            return Ok(None);
        };
        if groups.group("PH-PC-Blacklist") == Some("t1") {
            return Ok(None);
        }
        let skip = groups.skip_songs_assigned();
        let session = self.session.clone();
        let host = self.pc_experiments.clone();
        let owner = token.clone();
        let intent = transport.clone();
        let room = self.together_detached_guard(token);
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
        let outcome = controller
            .ordinary_minibar_skip(
                token,
                current.fence(),
                direction,
                &groups,
                OrdinaryQueueEnvironment::OnlineWithoutMvExperiment { app_online: true },
                authority,
            )
            .await?;
        transport.check()?;
        // An in-flight preflight failure isn't permission to send a second
        // transport intent. Only a decision of Unchanged falls through.
        Ok(outcome.applied)
    }
}
