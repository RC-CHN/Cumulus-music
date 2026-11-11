use super::*;
use crate::{
    download::service::{DownloadOptions, ScopedTaskAdmission, ScopedTaskLease},
    playlist::PlaylistService,
    storage::StateTransaction,
};

struct Admission {
    plan: Arc<Plan>,
    choice: ChoiceProof,
}

struct Lease<'a> {
    admission: &'a Admission,
    state: MutexGuard<'a, PlanState>,
}

impl ScopedTaskAdmission for Admission {
    fn acquire(&self, song_id: &str) -> Result<Box<dyn ScopedTaskLease + '_>> {
        if song_id != self.plan.authority.song {
            return Err(Error::StaleResponse);
        }
        let state = self.plan.state()?;
        state.choice(&self.choice.receipt)?;
        self.plan.authority.playback()?;
        Ok(Box::new(Lease {
            admission: self,
            state,
        }))
    }
}

impl ScopedTaskLease for Lease<'_> {
    fn verify(&self, tx: &StateTransaction<'_>, owner: &str) -> Result<()> {
        self.before_commit()?;
        if self.admission.choice.admission == Decision::Ready
            && membership(AccountResources::membership_in(
                tx,
                owner,
                ResourceKind::Liked,
                &self.admission.plan.authority.song,
            )?) != Decision::Ready
        {
            return Err(Error::State("liked download admission changed"));
        }
        Ok(())
    }

    fn before_commit(&self) -> Result<()> {
        let choice = self.state.choice(&self.admission.choice.receipt)?;
        if choice.proof.quality != self.admission.choice.quality
            || choice.proof.admission != self.admission.choice.admission
        {
            return Err(Error::StaleResponse);
        }
        self.admission.plan.authority.playback()
    }
}

impl MusicClient {
    pub(super) async fn start_current_download(
        &self,
        epoch: &str,
        plan: &Arc<Plan>,
        selection: &str,
        continuation: Option<&str>,
        options: DownloadOptions,
    ) -> Result<Reply> {
        let (choice, attempt) = {
            let state = plan.state()?;
            let choice = state.choice(selection)?;
            (choice.proof.clone(), choice.attempt.clone())
        };
        if options.quality != choice.quality {
            return Err(Error::InvalidInput(
                "download options differ from the selected purchased quality",
            ));
        }
        let scope = match (choice.admission, &attempt, continuation) {
            (Decision::Ready, None, None) => CredentialScope::Download,
            (Decision::PlaylistRequired, Some(_), Some(_)) => CredentialScope::Picker,
            _ => return Err(Error::StaleResponse),
        };
        let verify_continuation = |plan: &Plan| {
            if let (Some(attempt), Some(continuation)) = (&attempt, continuation) {
                attempt.check_continuation(
                    &PlaylistService::new(self.session.clone())?,
                    plan,
                    continuation,
                )?;
            }
            Ok::<(), Error>(())
        };
        plan.check_choice(&choice, scope)?;
        verify_continuation(plan)?;
        let available = self.configured_download_directory_available(epoch).await?;
        plan.check_choice(&choice, scope)?;
        verify_continuation(plan)?;
        if !available {
            return Ok(Reply::DirectoryRequired);
        }
        let local = self.local.context().await?;
        let session = self.session.clone();
        let continuation = continuation.map(str::to_owned);
        let permit = plan.clone();
        let checked_choice = choice.clone();
        let consumed = plan.clone();
        let committed_choice = choice.receipt.clone();
        let result = self
            .start_scoped_song_download(
                epoch,
                plan.authority.song.clone(),
                options,
                move || {
                    let authority = &permit.authority;
                    if !local
                        .library
                        .files_for_online_song(&session, &authority.token, &authority.song)?
                        .is_empty()
                    {
                        return Err(Error::State("song already has a local file"));
                    }
                    if let (Some(attempt), Some(continuation)) = (&attempt, &continuation) {
                        attempt.check_continuation(
                            &PlaylistService::new(session.clone())?,
                            &permit,
                            continuation,
                        )?;
                    }
                    permit.check_choice(&checked_choice, scope)
                },
                move || {
                    if let Ok(mut state) = consumed.state()
                        && let Ok(choice) = state.choice_mut(&committed_choice)
                    {
                        choice.consumed = true;
                    }
                },
                Admission {
                    plan: plan.clone(),
                    choice,
                },
            )
            .await?;
        Ok(Reply::Started { result })
    }
}
