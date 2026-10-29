//! Recovered M116 shortcut actions. Capture the media/account or resource
//! before asynchronous dispatch; never reinterpret a queued key for new media.
use super::*;
use crate::preferences::shortcuts::{Action, Hotkey};
use serde::{Deserialize, Serialize};

/// The selections actually observed by a WebView before sending a key. This
/// does not grant control: current native ownership and the account still win.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShortcutSelection {
    pub audio_play_id: Option<String>,
    pub audio_entry_id: Option<String>,
    pub video_play_id: Option<String>,
}

#[derive(Clone, Debug)]
enum Command {
    Transport(NativeMediaTarget, NativeMediaAction),
    Volume(NativeMediaTarget, f64),
    Like(NativeLikeSnapshot),
}
/// Native-only capability. It cannot be serialized or constructed over IPC.
#[derive(Clone, Debug)]
pub struct NativeShortcutInvocation(Command);
impl NativeShortcutInvocation {
    pub fn supported(key: Hotkey) -> bool {
        matches!(
            key,
            Hotkey::Space
                | Hotkey::F5
                | Hotkey::MediaPlayPause
                | Hotkey::MediaPrevious
                | Hotkey::MediaNext
                | Hotkey::MediaStop
                | Hotkey::Configured(
                    Action::PlayPause
                        | Action::Previous
                        | Action::Next
                        | Action::VolumeUp
                        | Action::VolumeDown
                        | Action::Like
                )
        )
    }
    /// Notify the existing like readback surface even when a write reply is lost.
    /// No credential or media authorization is included in this projection.
    pub fn like_selection(&self) -> Option<&NativeLikeSelection> {
        match &self.0 {
            Command::Like(view) => Some(&view.selection),
            _ => None,
        }
    }
}
impl MusicClient {
    /// Public selection IDs for lightweight surfaces. This lazy projection
    /// conveys no native capability or media authorization URL.
    pub fn native_shortcut_selection(&self) -> Result<ShortcutSelection> {
        let transport = self.native_media_snapshot()?;
        let Some(target) = transport.target else {
            return Ok(ShortcutSelection::default());
        };
        let mut result = ShortcutSelection::default();
        match target.media {
            Media::Audio(_) => {
                let value = self
                    .player
                    .controller
                    .get()
                    .ok_or(Error::StaleResponse)?
                    .snapshot();
                target.audio(&value)?;
                result.audio_play_id = value.play_id;
                result.audio_entry_id = value.selected.map(|v| v.entry_id);
            }
            Media::Video(_) => {
                let value = self
                    .mv
                    .existing(&target.token)?
                    .ok_or(Error::StaleResponse)?
                    .snapshot()?;
                target.video(&value)?;
                result.video_play_id = Some(value.play_id);
            }
        }
        target.check(self)?;
        Ok(result)
    }
    pub fn capture_local_shortcut(
        &self,
        epoch: &str,
        selection: &ShortcutSelection,
        key: Hotkey,
    ) -> Result<Option<NativeShortcutInvocation>> {
        let token = self.player_token(epoch)?;
        let before = self.native_media_snapshot()?.target;
        if let Some(target) = &before {
            if target.token != token {
                return Err(Error::StaleResponse);
            }
            match &target.media {
                Media::Audio(_) => {
                    let audio = self
                        .player
                        .controller
                        .get()
                        .ok_or(Error::StaleResponse)?
                        .snapshot();
                    target.audio(&audio)?;
                    if audio.play_id != selection.audio_play_id
                        || audio.selected.as_ref().map(|s| &s.entry_id)
                            != selection.audio_entry_id.as_ref()
                    {
                        return Err(Error::StaleResponse);
                    }
                }
                Media::Video(_) => {
                    let video = self
                        .mv
                        .existing(&token)?
                        .ok_or(Error::StaleResponse)?
                        .snapshot()?;
                    target.video(&video)?;
                    if selection.video_play_id.as_ref() != Some(&video.play_id) {
                        return Err(Error::StaleResponse);
                    }
                }
            }
        }
        let invocation = self.capture_native_shortcut(key)?;
        if before != self.native_media_snapshot()?.target {
            return Err(Error::StaleResponse);
        }
        self.session.check_generation(&token)?;
        Ok(invocation)
    }
    pub fn capture_native_shortcut(&self, key: Hotkey) -> Result<Option<NativeShortcutInvocation>> {
        if !NativeShortcutInvocation::supported(key) {
            return Err(Error::InvalidInput("shortcut action unavailable"));
        }
        if key == Hotkey::Configured(Action::Like) {
            let Some(selection) = self.native_like_selection()? else {
                return Ok(None);
            };
            let view = self.native_like_snapshot(&selection)?;
            if view.pending {
                return Err(Error::MutationUncertain);
            }
            if view.observed.is_none() {
                return Err(Error::State("like state is unknown"));
            }
            return Ok(Some(NativeShortcutInvocation(Command::Like(view))));
        }
        let view = self.native_media_details()?;
        let Some(target) = view.transport.target else {
            return Ok(None);
        };
        // M116 ignores next/previous/stop for an active video. Shortcut stepping
        // intentionally plays the next track, unlike MPRIS's preserve-pause rule.
        if view
            .item
            .as_ref()
            .is_some_and(|i| i.kind == NativeMediaKind::Video)
            && matches!(
                key,
                Hotkey::MediaPrevious
                    | Hotkey::MediaNext
                    | Hotkey::MediaStop
                    | Hotkey::Configured(Action::Previous | Action::Next)
            )
        {
            return Ok(None);
        }
        let action = match key {
            Hotkey::Space
            | Hotkey::F5
            | Hotkey::MediaPlayPause
            | Hotkey::Configured(Action::PlayPause) => NativeMediaAction::Toggle,
            Hotkey::MediaPrevious | Hotkey::Configured(Action::Previous) => {
                NativeMediaAction::Previous
            }
            Hotkey::MediaNext | Hotkey::Configured(Action::Next) => NativeMediaAction::Next,
            Hotkey::MediaStop => NativeMediaAction::Stop,
            Hotkey::Configured(Action::VolumeUp | Action::VolumeDown) => {
                let step = if key == Hotkey::Configured(Action::VolumeUp) {
                    5.0
                } else {
                    -5.0
                };
                return Ok(Some(NativeShortcutInvocation(Command::Volume(
                    target,
                    (view.volume + step).clamp(0.0, 100.0),
                ))));
            }
            _ => return Err(Error::InvalidInput("shortcut action unavailable")),
        };
        Ok(Some(NativeShortcutInvocation(Command::Transport(
            target, action,
        ))))
    }
    pub async fn execute_native_shortcut(
        &self,
        invocation: &NativeShortcutInvocation,
    ) -> Result<()> {
        match &invocation.0 {
            Command::Transport(target, action) => self.native_media_action(target, *action).await,
            Command::Volume(target, value) => {
                self.native_media_adjust(target, NativeMediaAdjustment::Volume { value: *value })
                    .await
            }
            Command::Like(view) => self.native_like_toggle(view).await,
        }
    }
}
