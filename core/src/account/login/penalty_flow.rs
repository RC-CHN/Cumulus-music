//! An explicit read of recovered penalty state. The absent RN completion
//! callbacks are never synthesized and saved credentials are never replayed.
use super::{
    CredentialLogin, LoginProtocol, LoginStatus,
    penalty::{PenaltyStatusOutcome, PenaltyView},
    state::Operation,
};
use crate::{Error, Result};

impl CredentialLogin {
    pub fn penalty_snapshot(&self) -> Result<Option<PenaltyView>> {
        Ok(self.lock()?.penalty.as_ref().map(|attempt| attempt.view()))
    }

    pub async fn penalty_check(&self, id: &str) -> Result<()> {
        self.check_login_base()?;
        let operation =
            Operation::begin_checked(&self.state, LoginStatus::Penalty, None, |state| {
                let attempt = state
                    .penalty
                    .as_ref()
                    .filter(|a| a.view().id == id)
                    .ok_or(Error::StaleResponse)?;
                if !attempt.view().can_check_status {
                    return Err(Error::MutationBusy);
                }
                Ok(())
            })?;
        let (request, mut cancelled) = {
            let mut state = operation.lock()?;
            let attempt = state.penalty.as_mut().ok_or(Error::StaleResponse)?;
            (attempt.begin_status_check()?, attempt.cancel.subscribe())
        };
        let work = async {
            let response = if self.protocol == LoginProtocol::Jsk {
                self.api.request(request.path, request.payload, false).await
            } else {
                // The recovered status call still uses the SDK login adapter
                // after a legacy credential response. Lifecycle JSK only
                // accepts refresh endpoints. Start an empty native context;
                // never recover a MUSIC_TU header from persistent cookies.
                let context = crate::protocol::http::JskLoginContext::default();
                self.candidate
                    .client()
                    .request_jsk_login(request.path, request.payload, &context)
                    .await
            };
            operation.check()?;
            let outcome = {
                let mut state = operation.lock()?;
                let attempt = state.penalty.as_mut().ok_or(Error::StaleResponse)?;
                match response {
                    Ok(response) => attempt.apply_status(&response),
                    Err(error) => {
                        attempt.status_failed();
                        Err(error)
                    }
                }?
            };
            if outcome == PenaltyStatusOutcome::AllPassed {
                // The user may have changed the password in the official app.
                // account/get is the only allowed continuation of this check.
                self.verify_identity(&operation).await?;
            }
            Ok(())
        };
        // A single deadline and cancellation boundary cover both reads. In
        // particular, a stalled identity read must release the caller's account
        // transition gate when the user closes or replaces this attempt.
        let result = tokio::select! {
            biased;
            _ = cancelled.changed() => Err(Error::Cancelled),
            result = tokio::time::timeout(std::time::Duration::from_secs(45), work) => {
                result.unwrap_or_else(|_| Err(Error::Transport("penalty check timeout".into())))
            }
        };
        if result.is_err()
            && let Ok(mut state) = operation.lock()
            && let Some(attempt) = &mut state.penalty
        {
            attempt.status_failed();
        }
        operation.finish(result)
    }
}
