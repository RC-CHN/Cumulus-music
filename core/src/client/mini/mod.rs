//! A second surface over the existing player, never a second player. Hosts retain
//! the opaque snapshot and pass only its public view to an untrusted UI.
mod actions;
mod feedback;
mod fm;
mod intelligence;
mod mode;
mod queue;
mod reports;
use super::{MusicClient, native_media::*};
use crate::playback::mode::ModeFamily;
use crate::{Error, Result, account::SessionToken, playback::PlaybackMode};
use mode::MiniModeContext;
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct MiniSnapshot {
    token: SessionToken,
    target: Option<NativeMediaTarget>,
    like: Option<NativeLikeSnapshot>,
    mode_context: Option<MiniModeContext>,
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
    pub unmute_volume: Option<f64>,
    pub mode: Option<PlaybackMode>,
    pub mode_family: Option<ModeFamily>,
    pub fm_mode_pending: Option<MiniFmModePending>,
    pub queue_revision: Option<String>,
    pub can_intelligence: bool,
    pub can_lyric_report: bool,
    pub intelligence: Option<crate::playback::intelligence::IntelligencePhase>,
    pub intelligence_operation: Option<String>,
    pub intelligence_read: Option<MiniIntelligenceRead>,
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
            && self.mode_context == other.mode_context
            && self.view.mode_family == other.view.mode_family
            && self.view.fm_mode_pending == other.view.fm_mode_pending
            && self.view.can_intelligence == other.view.can_intelligence
    }
    fn check(&self, client: &MusicClient) -> Result<()> {
        client.session.check_generation(&self.token)?;
        if let Some(target) = &self.target {
            target.check(client)?;
        } else if client.native_media_snapshot()?.target.is_some() {
            return Err(Error::StaleResponse);
        }
        if let Some(context) = &self.mode_context {
            context.check(client, &self.token)?;
        }
        let state = client.player.controller.get().map(|v| v.snapshot());
        let state = state
            .as_ref()
            .filter(|v| v.account == self.token && self.view.mode_family == Some(ModeFamily::Fm));
        if client.mini_fm_mode_pending(&self.token, state)? != self.view.fm_mode_pending {
            return Err(Error::StaleResponse);
        }
        if client.mini_mode_family(&self.token, self.view.mode)? != self.view.mode_family {
            return Err(Error::StaleResponse);
        }
        if client.mini_heart_hint(&self.token, self.view.mode)? != self.view.can_intelligence {
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
                } else {
                    let state = self.player.controller.get().map(|v| v.snapshot());
                    match state.filter(|v| v.account == *token) {
                        Some(state) if state.fm => ModeFamily::Fm,
                        Some(state) if state.intelligence.as_ref().is_some_and(|v| v.active()) => {
                            ModeFamily::Intelligence
                        }
                        _ => ModeFamily::Ordinary,
                    }
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
        let audio = self
            .player
            .controller
            .get()
            .map(|v| v.snapshot())
            .filter(|v| v.account == token);
        let focus = self.focus.snapshot()?;
        let mode_audio = audio.as_ref().filter(|state| {
            details.mode.is_some() || (state.fm && focus.owner != Some(super::focus::Owner::Video))
        });
        let mode = mode_audio.map(|state| state.mode);
        let can_lyric_report = token.uid.is_some()
            && details
                .item
                .as_ref()
                .is_some_and(|i| i.kind == NativeMediaKind::Music)
            && audio
                .as_ref()
                .and_then(|s| s.selected.as_ref())
                .is_some_and(|s| {
                    s.resource["voice_id"].is_null() && s.resource["song"]["voice_id"].is_null()
                });
        let activity = self.intelligence_read_operation(&token)?;
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
                unmute_volume: details.unmute_volume,
                mode,
                mode_family: self.mini_mode_family(&token, mode)?,
                fm_mode_pending: self.mini_fm_mode_pending(&token, mode_audio)?,
                queue_revision: mode_audio.map(|v| v.queue_revision.to_string()),
                can_intelligence: self.mini_heart_hint(&token, mode)?,
                can_lyric_report,
                intelligence: mode_audio.and_then(|v| v.intelligence.as_ref().map(|s| s.phase)),
                intelligence_operation: mode_audio.and_then(|v| v.intelligence_operation.clone()),
                intelligence_read: activity.map(|operation_id| {
                    let preparing = audio
                        .as_ref()
                        .is_none_or(|v| v.intelligence.as_ref().is_none_or(|s| !s.active()));
                    MiniIntelligenceRead {
                        operation_id,
                        preparing,
                    }
                }),
                like: like.as_ref().map(|v| MiniLikeView {
                    observed: v.observed,
                    pending: v.pending,
                }),
                error,
            },
            token,
            target: details.transport.target,
            like,
            mode_context: mode_audio.map(|state| MiniModeContext {
                focus,
                expected: state.fence(),
            }),
        };
        snapshot.check(self)?;
        Ok(snapshot)
    }
}
pub use actions::MiniAction;
pub use feedback::MiniFmFeedbackView;
pub use fm::MiniFmModePending;
pub use intelligence::{MiniIntelligenceCommand, MiniIntelligenceRead};
pub use mode::MiniPlaybackReceipt;
pub use queue::MiniQueuePage;
