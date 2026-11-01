//! User-authorized task removal. Source audio and published cloud resources are
//! never deleted. Unconfirmed remote cancellation remains visible for recovery.
use super::*;
impl UploadScheduler {
    pub async fn remove(&self, id: &str, retry: bool) -> Result<CloudView> {
        let task = self.uploads.get(id)?;
        let existing = self.entry(id)?;
        if existing.as_ref().is_some_and(|entry| entry.removing) && !retry {
            return Ok(task); // Lost reply recovery never reissues a cancellation.
        }
        let terminal = matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled);
        if terminal {
            self.change(id, |state| {
                state.removing = true;
                state.hidden = true;
                state.enabled = false;
                Ok(())
            })?;
            return Ok(task);
        }
        // Check parent intent before disabling scheduling: an in-flight or
        // abandoned allocation/metadata/publication cannot be cancelled safely.
        if self.uploads.read(id)?.uncertain() || task.phase == CloudPhase::Published {
            return Err(Error::MutationUncertain);
        }
        let cancelled = self.control(id, UploadControl::Cancel).await?;
        // Cancel is already durable. A failed schedule commit can be retried:
        // cancelled tasks only need a local hide; child journals prevent replay.
        self.change(id, |state| {
            state.removing = true;
            state.hidden = matches!(
                cancelled.phase,
                CloudPhase::Complete | CloudPhase::Cancelled
            );
            state.enabled = false;
            state.error = None;
            Ok(())
        })?;
        Ok(cancelled)
    }
    pub(super) fn finish_removal(&self, id: &str, outcome: Result<CloudView>) -> Result<()> {
        if let Err(error @ Error::Storage(_)) = outcome {
            return Err(error);
        }
        let task = self.uploads.get(id)?;
        self.change(id, |state| {
            state.enabled = false;
            state.running = false;
            state.hidden = matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled);
            state.error = outcome.err().map(|error| {
                match error.root() {
                    Error::MutationUncertain => "write_outcome_unknown",
                    Error::Transport(_) | Error::HttpStatus { .. } => "network",
                    _ => "upload_removal_incomplete",
                }
                .into()
            });
            Ok(())
        })?;
        Ok(())
    }
}
