//! M3085's first-program playId and M3160 resume authority for empty shuffle.
use super::{Cancellation, Error, MusicClient, Result, SessionToken, ShuffleAnchor, Value};
use crate::{playback::controller::HistoryShuffleAnchor, podcast::resume};
use std::time::Duration;

impl MusicClient {
    pub(super) async fn history_shuffle_anchor(
        &self,
        token: &SessionToken,
        first: &Value,
        pending: &Cancellation,
        room: &Cancellation,
    ) -> Result<Option<HistoryShuffleAnchor>> {
        if token.uid.is_none() || self.login_pending()? {
            return Ok(None);
        }
        let Some(anchor) = ShuffleAnchor::capture(&self.session, token, first)? else {
            return Ok(None);
        };
        let api = self.session.native_api(token)?;
        let Some(credentials) = api.podcast_resume_observation_revision().ok().flatten() else {
            return Ok(None);
        };
        let Some(group) =
            self.pc_experiments
                .observed_group(token, &api, &credentials, resume::EXPERIMENT)?
        else {
            return Ok(None);
        };
        let in_scope = |pc: &crate::client::pc_experiments::PcExperimentHost,
                        token: &SessionToken,
                        api: &std::sync::Arc<dyn crate::protocol::api::Api>,
                        credentials: &str,
                        group: &str|
         -> Result<bool> {
            // This check also runs under the account lock at commit. Only the
            // transport and assignment are touched; no recursive session read.
            Ok(
                api.podcast_resume_observation_revision()?.as_deref() == Some(credentials)
                    && pc
                        .observed_group(token, api, credentials, resume::EXPERIMENT)?
                        .as_deref()
                        == Some(group),
            )
        };
        let authorize = || -> Result<()> {
            pending.check()?;
            room.check()?;
            self.session.require_api(token, &api)?;
            if self.login_pending()?
                || !in_scope(&self.pc_experiments, token, &api, &credentials, &group)?
            {
                return Err(Error::StaleResponse);
            }
            let store = self.session.persistent_store()?;
            self.session
                .with_token(token, |owner| anchor.require_current(&store, owner))
        };
        authorize()?;
        if group == "t1" {
            match self
                .session
                .podcast_resume
                .known(token, &api, &credentials, anchor.radio_id())?
            {
                Some(false) => return Ok(None),
                Some(true) => {}
                None => {
                    // Existing signed PC scoped-read transport verifies the
                    // same receipt before and after dispatch. No write is used.
                    let result = tokio::time::timeout(
                        Duration::from_secs(8),
                        resume::read_player_for(
                            &self.session,
                            token,
                            anchor.radio_id(),
                            resume::PlayerBinding {
                                revision: &credentials,
                                permission: &authorize,
                            },
                        ),
                    )
                    .await;
                    authorize()?;
                    if !matches!(result, Ok(Ok(true)))
                        || self.session.podcast_resume.known(
                            token,
                            &api,
                            &credentials,
                            anchor.radio_id(),
                        )? != Some(true)
                    {
                        return Ok(None);
                    }
                }
            }
        }
        let session = self.session.clone();
        let pc = self.pc_experiments.clone();
        let token = token.clone();
        let radio = anchor.radio_id().to_owned();
        let setting = Cancellation::new(move || {
            in_scope(&pc, &token, &api, &credentials, &group).unwrap_or(false)
                && (group != "t1"
                    || session
                        .podcast_resume
                        .known(&token, &api, &credentials, &radio)
                        .is_ok_and(|value| value == Some(true)))
        });
        setting.check()?;
        Ok(Some(HistoryShuffleAnchor { anchor, setting }))
    }
}
