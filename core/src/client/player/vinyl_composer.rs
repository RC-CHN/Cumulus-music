//! Explicit current-track publication/reply from the vinyl modal. The durable
//! comment journal retains an outcome even after the user leaves the vinyl.
use super::{
    MusicClient, PlaybackCommand, PlaybackController,
    vinyl_comments::{EXPERIMENT, song},
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::{
        comments::{CommentWriteAction, CommentWriteView, write_view},
        dto::AccountData,
    },
    comments::{
        CommentResource, CommentWriteCommand, CommentWritePhase, Comments, VinylWriteAuthority,
    },
    protocol::api::Api,
};
use std::sync::Arc;

#[derive(PartialEq, Eq)]
pub(super) struct VinylDraft {
    id: String,
    play_id: String,
    token: SessionToken,
    revision: String,
}

pub(super) struct Target {
    controller: Arc<PlaybackController>,
    pub(super) token: SessionToken,
    pub(super) api: Arc<dyn Api>,
    pub(super) revision: String,
    pub(super) resource: CommentResource,
}

impl Target {
    pub(super) async fn check(&self, client: &MusicClient, play_id: &str) -> Result<()> {
        let current = self
            .controller
            .command(&self.token, PlaybackCommand::Snapshot)
            .await?;
        if song(&current, play_id)? != self.resource
            || client.login_pending()?
            || self.api.verified_pc_experiment_revision()?.as_deref() != Some(&self.revision)
        {
            return Err(Error::StaleResponse);
        }
        client
            .session
            .with_verified_profile(&self.token, &self.api, |_, _| Ok(()))
    }

    pub(super) fn check_published_play(&self, play_id: &str) -> Result<()> {
        let current = self.controller.snapshot();
        if current.account != self.token || song(&current, play_id)? != self.resource {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    fn permit_dispatch(
        &self,
        client: &MusicClient,
        play_id: &str,
        draft: &VinylDraft,
    ) -> Result<()> {
        self.permit_live(client, play_id, || {
            let mut intent = client
                .player
                .vinyl_draft
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if intent.as_ref() != Some(draft) {
                return Err(Error::StaleResponse);
            }
            *intent = None;
            Ok(())
        })
    }

    pub(super) fn permit_live(
        &self,
        client: &MusicClient,
        play_id: &str,
        permit: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        if client.login_pending()? {
            return Err(Error::StaleResponse);
        }
        client
            .session
            .with_verified_profile(&self.token, &self.api, |_, _| {
                // The HTTP gate must not await an actor command that could need
                // this same transport. Inspect the already-published native state.
                self.check_published_play(play_id)?;
                permit()
            })
    }

    fn draft(&self, play_id: &str, id: &str) -> VinylDraft {
        VinylDraft {
            id: id.into(),
            play_id: play_id.into(),
            token: self.token.clone(),
            revision: self.revision.clone(),
        }
    }
}

impl MusicClient {
    pub(super) async fn vinyl_write_target(&self, epoch: &str, play_id: &str) -> Result<Target> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let controller = self.player.controller().await?.clone();
        let resource = song(
            &controller
                .command(&token, PlaybackCommand::Snapshot)
                .await?,
            play_id,
        )?;
        let (revision, groups) = self.verified_pc_experiment_groups_for(&token, &api).await?;
        if groups.group(EXPERIMENT) != Some("t1") {
            return Err(Error::State("vinyl comments experiment is unavailable"));
        }
        let target = Target {
            controller,
            token,
            api,
            revision,
            resource,
        };
        target.check(self, play_id).await?;
        Ok(target)
    }

    pub async fn player_vinyl_write_operations(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<AccountData<Vec<CommentWriteView>>> {
        let target = self.vinyl_write_target(epoch, play_id).await?;
        let records = Comments::new(self.session.clone())?
            .write_operations_for(&target.token, &target.resource)?;
        self.session.with_token(&target.token, |owner| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: records.into_iter().map(|r| write_view(r, owner)).collect(),
            })
        })
    }

    pub async fn player_vinyl_prepare_write(
        &self,
        epoch: &str,
        play_id: &str,
        command: CommentWriteCommand,
    ) -> Result<AccountData<CommentWriteView>> {
        if !matches!(command, CommentWriteCommand::Publish { .. }) {
            return Err(Error::InvalidInput(
                "vinyl composer accepts only publication",
            ));
        }
        let target = self.vinyl_write_target(epoch, play_id).await?;
        let record = Comments::new(self.session.clone())?
            .prepare_write_for(&target.token, &target.resource, command)
            .await?;
        target.check(self, play_id).await?;
        self.session.with_token(&target.token, |owner| {
            *self
                .player
                .vinyl_draft
                .lock()
                .map_err(|_| Error::LockPoisoned)? = Some(target.draft(play_id, &record.id));
            Ok(AccountData {
                epoch: epoch.into(),
                value: write_view(record, owner),
            })
        })
    }

    pub async fn player_vinyl_write_command(
        &self,
        epoch: &str,
        play_id: &str,
        id: &str,
        command: CommentWriteAction,
    ) -> Result<AccountData<CommentWriteView>> {
        if !matches!(command, CommentWriteAction::Submit) {
            return Err(Error::InvalidInput(
                "resolve old writes from the independent comments page",
            ));
        }
        let target = self.vinyl_write_target(epoch, play_id).await?;
        let draft = target.draft(play_id, id);
        if self
            .player
            .vinyl_draft
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            != Some(&draft)
        {
            return Err(Error::StaleResponse);
        }
        let service = Comments::new(self.session.clone())?;
        let record = service.write_operation_for(&target.token, id)?;
        if record.phase != CommentWritePhase::Prepared
            || record.resource != target.resource
            || !matches!(record.command, CommentWriteCommand::Publish { .. })
        {
            return Err(Error::StaleResponse);
        }
        let permission = || target.permit_dispatch(self, play_id, &draft);
        let authority = VinylWriteAuthority {
            api: &target.api,
            revision: &target.revision,
            permission: &permission,
        };
        let record = service
            .submit_write_guarded(&target.token, id, Some(authority), || {
                target.check(self, play_id)
            })
            .await?;
        // The journal already holds the original owner's acknowledgement;
        // only project the result while this is still the displayed play.
        target.check_published_play(play_id)?;
        self.session.with_token(&target.token, |owner| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: write_view(record, owner),
            })
        })
    }
}
