//! Native menus/media controls share the ordinary player use cases. A captured
//! target is deliberately neither serializable nor constructible by a WebView.
mod actions;
mod adjustments;
mod details;
mod likes;
mod shortcuts;
mod view;
use super::{
    MusicClient,
    focus::{FocusSnapshot, Owner},
};
use crate::{
    Error, Result,
    account::SessionToken,
    mv::playback::{VideoFence, VideoSnapshot},
    playback::{
        controller::{PlaybackFence, PlaybackSnapshot},
        engine::Cancellation,
    },
};
pub use adjustments::NativeMediaAdjustment;
pub use details::{NativeMediaDetails, NativeMediaItem, NativeMediaKind, NativeMediaStatus};
pub use likes::{NativeLikeResource, NativeLikeSelection, NativeLikeSnapshot};
pub use shortcuts::NativeShortcutInvocation;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeMediaAction {
    Play,
    Pause,
    Toggle,
    Previous,
    Next,
    Stop,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Media {
    Audio(PlaybackFence),
    Video(VideoFence),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeMediaTarget {
    token: SessionToken,
    pub(super) focus: FocusSnapshot,
    media: Media,
    playing: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeMediaSnapshot {
    pub title: String,
    pub playing: bool,
    pub can_previous: bool,
    pub can_next: bool,
    pub target: Option<NativeMediaTarget>,
}
impl NativeMediaTarget {
    pub(super) fn audio(&self, value: &PlaybackSnapshot) -> Result<PlaybackFence> {
        if self.token != value.account || self.media != Media::Audio(value.fence()) {
            return Err(Error::StaleResponse);
        }
        Ok(value.fence())
    }
    pub(super) fn video(&self, value: &VideoSnapshot) -> Result<VideoFence> {
        if self.token != value.account || self.media != Media::Video(value.fence()) {
            return Err(Error::StaleResponse);
        }
        Ok(value.fence())
    }
    pub(super) fn check(&self, client: &MusicClient) -> Result<()> {
        client.session.check_generation(&self.token)?;
        client.focus.check_snapshot(self.focus)?;
        match &self.media {
            Media::Audio(_) => {
                self.audio(
                    &client
                        .player
                        .controller
                        .get()
                        .ok_or(Error::StaleResponse)?
                        .snapshot(),
                )?;
            }
            Media::Video(_) => {
                self.video(
                    &client
                        .mv
                        .existing(&self.token)?
                        .ok_or(Error::StaleResponse)?
                        .snapshot()?,
                )?;
            }
        }
        Ok(())
    }
    pub(super) fn cancellation(
        &self,
        client: &MusicClient,
        lease: Option<u64>,
        owner: Owner,
        intent: Option<u64>,
    ) -> Cancellation {
        let expected = lease
            .map(|generation| FocusSnapshot {
                generation,
                owner: Some(owner),
            })
            .unwrap_or(self.focus);
        let focus = client.focus.clone();
        let selection = client.player.selection.clone();
        Cancellation::new(move || {
            focus.check_snapshot(expected).is_ok()
                && intent.is_none_or(|value| selection.load(Ordering::Acquire) == value)
        })
    }
}
