//! M/185 cloud-credit write is prepared from the live native lyric, never
//! from an arbitrary resource ID supplied by a host.
use super::contributors::ordinary_song;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    client::MusicClient,
    lyrics::{
        LyricBountyKind, LyricBountyStatus, LyricBountyWrite, LyricTarget, LyricsService,
        bounty::Bounties,
    },
    playback::{QueueEntry, controller::PlaybackController},
};
use std::sync::Arc;
use uuid::Uuid;

/// One explicit confirmation, bound to the same player, account, fresh lyric
/// result, and credential revision. This native type is not an IPC payload.
pub struct LyricBountyIntent {
    session: Arc<AccountSession>,
    token: SessionToken,
    controller: Arc<PlaybackController>,
    service: Arc<LyricsService>,
    play_id: String,
    entry: QueueEntry,
    target: LyricTarget,
    song_id: String,
    kind: LyricBountyKind,
    lyric_revision: Uuid,
    credential_revision: String,
    existing: Option<LyricBountyStatus>,
}

impl LyricBountyIntent {
    pub fn kind(&self) -> LyricBountyKind {
        self.kind
    }

    pub fn existing_status(&self) -> Option<LyricBountyStatus> {
        self.existing
    }

    fn authorize(&self) -> Result<()> {
        let view = self.controller.snapshot();
        if view.account != self.token
            || view.play_id.as_deref() != Some(self.play_id.as_str())
            || view.selected.as_ref() != Some(&self.entry)
        {
            return Err(Error::StaleResponse);
        }
        let (lyric, credential) = self.service.bounty_revision(&self.target, self.kind)?;
        if lyric != self.lyric_revision || credential != self.credential_revision {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

impl MusicClient {
    /// Reads local state only. A presentation host must show the separate
    /// one-credit confirmation before consuming this intent.
    pub async fn prepare_lyric_bounty(
        &self,
        epoch: &str,
        play_id: &str,
        kind: LyricBountyKind,
    ) -> Result<LyricBountyIntent> {
        let context = self.lyric_context(epoch, play_id).await?;
        let LyricTarget::Song(id) = &context.target else {
            return Err(Error::InvalidInput(
                "lyric bounty requires an ordinary song",
            ));
        };
        let song_id = id.clone();
        let (lyric_revision, credential_revision) =
            context.service.bounty_revision(&context.target, kind)?;
        let api = self.session.native_api(&context.token)?;
        if api.lyric_bounty_credential_revision()?.as_deref() != Some(credential_revision.as_str())
        {
            return Err(Error::CredentialChanged);
        }
        let existing = Bounties::new(self.session.clone(), context.token.clone())?
            .status(&LyricBountyWrite::new(&song_id, kind)?)?;
        let controller = self.player.controller().await?.clone();
        let entry = controller
            .snapshot()
            .selected
            .filter(|entry| ordinary_song(&entry.resource, &song_id))
            .ok_or(Error::InvalidInput(
                "lyric bounty requires an ordinary song",
            ))?;
        let intent = LyricBountyIntent {
            session: self.session.clone(),
            token: context.token,
            controller,
            entry,
            service: context.service,
            play_id: play_id.into(),
            target: context.target,
            song_id,
            kind,
            lyric_revision,
            credential_revision,
            existing,
        };
        intent.authorize()?;
        Ok(intent)
    }

    /// The caller owns the user's explicit spend confirmation. The journal
    /// deduplicates concurrent confirmations and permanently freezes an
    /// ambiguous attempt for the same account, song and bounty kind.
    pub async fn confirm_lyric_bounty(
        &self,
        intent: LyricBountyIntent,
    ) -> Result<LyricBountyStatus> {
        if !Arc::ptr_eq(&self.session, &intent.session)
            || !Arc::ptr_eq(self.player.controller().await?, &intent.controller)
        {
            return Err(Error::StaleResponse);
        }
        intent.authorize()?;
        Bounties::new(self.session.clone(), intent.token.clone())?
            .submit(
                LyricBountyWrite::new(&intent.song_id, intent.kind)?,
                &intent.credential_revision,
                &|| intent.authorize(),
            )
            .await
    }
}
