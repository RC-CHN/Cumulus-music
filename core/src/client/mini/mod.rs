//! A second surface over the existing player, never a second player. Hosts retain
//! the opaque snapshot and pass only its public view to an untrusted UI.
mod actions;
mod queue;
use super::{MusicClient, native_media::*};
use crate::playback::mode::ModeFamily;
use crate::{Error, Result, account::SessionToken, playback::PlaybackMode};
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct MiniSnapshot {
    token: SessionToken,
    target: Option<NativeMediaTarget>,
    like: Option<NativeLikeSnapshot>,
    pub view: MiniView,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniView {
    pub epoch: String,
    pub selection: ShortcutSelection,
    pub item: Option<NativeMediaItem>,
    pub state: NativeMediaStatus,
    pub playing: bool,
    pub can_previous: bool,
    pub can_next: bool,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub seek_start: f64,
    pub seek_end: Option<f64>,
    pub can_seek: bool,
    pub volume: f64,
    pub mode: Option<PlaybackMode>,
    pub mode_family: Option<ModeFamily>,
    pub like: Option<MiniLikeView>,
    pub error: Option<&'static str>,
}
#[derive(Clone, Debug, Serialize)]
pub struct MiniLikeView {
    pub observed: Option<bool>,
    pub pending: bool,
}
impl MiniSnapshot {
    /// Clock ticks do not revoke controls; changes of account, playback intent,
    /// queue or observed like state do. The host still validates every command.
    pub fn same_controls(&self, other: &Self) -> bool {
        self.token == other.token
            && self.target == other.target
            && self.like == other.like
            && self.view.mode_family == other.view.mode_family
    }
    fn check(&self, client: &MusicClient) -> Result<()> {
        client.session.check_generation(&self.token)?;
        if let Some(target) = &self.target {
            target.check(client)?;
        } else if client.native_media_snapshot()?.target.is_some() {
            return Err(Error::StaleResponse);
        }
        if client.mini_mode_family(&self.token, self.view.mode)? != self.view.mode_family {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
    fn audio(&self, client: &MusicClient) -> Result<&NativeMediaTarget> {
        self.check(client)?;
        if self.view.item.as_ref().map(|v| v.kind) != Some(NativeMediaKind::Music) {
            return Err(Error::InvalidInput("mini queue requires music ownership"));
        }
        self.target.as_ref().ok_or(Error::StaleResponse)
    }
}
impl MusicClient {
    fn mini_mode_family(
        &self,
        token: &SessionToken,
        mode: Option<PlaybackMode>,
    ) -> Result<Option<ModeFamily>> {
        mode.map(|_| {
            self.together_attached(token, None).map(|attached| {
                if attached {
                    ModeFamily::Together
                } else if self.player.controller.get().is_some_and(|controller| {
                    let state = controller.snapshot();
                    state.account == *token
                        && state.intelligence.as_ref().is_some_and(|v| v.active())
                }) {
                    ModeFamily::Intelligence
                } else {
                    ModeFamily::Ordinary
                }
            })
        })
        .transpose()
    }
    /// Lazy, local projection. No HTTP, audio initialization or playback startup.
    pub fn mini_snapshot(&self) -> Result<MiniSnapshot> {
        let token = self.session.token()?;
        let details = self.native_media_details()?;
        let like = self
            .native_like_selection()?
            .map(|selection| self.native_like_snapshot(&selection))
            .transpose()?;
        let error = if details
            .item
            .as_ref()
            .is_some_and(|v| v.kind == NativeMediaKind::Music)
        {
            let value = self
                .player
                .controller
                .get()
                .ok_or(Error::StaleResponse)?
                .snapshot();
            details
                .transport
                .target
                .as_ref()
                .ok_or(Error::StaleResponse)?
                .audio(&value)?;
            super::player::project(&value, &token)?.error
        } else {
            None
        };
        let snapshot = MiniSnapshot {
            view: MiniView {
                epoch: token.generation.to_string(),
                selection: self.native_shortcut_selection()?,
                item: details.item,
                state: details.state,
                playing: details.transport.playing,
                can_previous: details.transport.can_previous,
                can_next: details.transport.can_next,
                position: details.position,
                duration: details.duration,
                seek_start: details.seek_start,
                seek_end: details.seek_end,
                can_seek: details.can_seek,
                volume: details.volume,
                mode: details.mode,
                mode_family: self.mini_mode_family(&token, details.mode)?,
                like: like.as_ref().map(|v| MiniLikeView {
                    observed: v.observed,
                    pending: v.pending,
                }),
                error,
            },
            token,
            target: details.transport.target,
            like,
        };
        snapshot.check(self)?;
        Ok(snapshot)
    }
}
pub use actions::MiniAction;
pub use queue::MiniQueuePage;
