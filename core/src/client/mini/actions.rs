use super::*;
use crate::client::dto::PlayerAction;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MiniAction {
    Toggle,
    Previous,
    Next,
    Stop,
    Seek { seconds: f64 },
    Volume { value: f64 },
    Mode { mode: PlaybackMode },
    Select { entry_id: String },
    Remove { entry_id: String },
    Clear,
    Like,
    RefreshLike,
}
impl MusicClient {
    /// Execute once against what the user saw. Never recapture a newer target
    /// after rejection, or replay an uncertain remote mutation.
    pub async fn mini_action(&self, displayed: &MiniSnapshot, action: MiniAction) -> Result<()> {
        displayed.check(self)?;
        if let MiniAction::Mode { mode } = action {
            // The snapshot's policy is part of the displayed controls. Do not
            // reinterpret an old ordinary cycle after a room attaches/exits.
            let _room = self
                .together
                .gate
                .try_lock()
                .map_err(|_| Error::MutationBusy)?;
            displayed.check(self)?;
            let family = displayed
                .view
                .mode_family
                .ok_or(Error::InvalidInput("mini mode unavailable"))?;
            if !family.supports(mode) {
                return Err(Error::InvalidInput(
                    "mode unavailable in mini playback context",
                ));
            }
            let target = displayed.target.as_ref().ok_or(Error::StaleResponse)?;
            self.native_media_adjust(target, NativeMediaAdjustment::Mode { mode })
                .await?;
            return Ok(());
        }
        match action {
            MiniAction::Like | MiniAction::RefreshLike => {
                let like = displayed
                    .like
                    .as_ref()
                    .ok_or(Error::InvalidInput("mini like unavailable"))?;
                if matches!(action, MiniAction::Like) {
                    self.native_like_toggle(like).await?;
                } else {
                    self.native_like_refresh(&like.selection).await?;
                }
            }
            MiniAction::Select { .. } | MiniAction::Remove { .. } | MiniAction::Clear => {
                let target = displayed.audio(self)?;
                if displayed.view.mode.is_none() {
                    return Err(Error::InvalidInput("FM owns the mini queue"));
                }
                let action = match action {
                    MiniAction::Select { entry_id } => PlayerAction::Select { entry_id },
                    MiniAction::Remove { entry_id } => PlayerAction::Remove {
                        entry_ids: vec![entry_id],
                    },
                    MiniAction::Clear => PlayerAction::Clear,
                    _ => unreachable!(),
                };
                self.player_action_checked(&displayed.view.epoch, action, Some(target))
                    .await?;
            }
            MiniAction::Seek { .. } | MiniAction::Volume { .. } => {
                let target = displayed
                    .target
                    .as_ref()
                    .ok_or(Error::InvalidInput("mini media unavailable"))?;
                let adjustment = match action {
                    MiniAction::Seek { seconds } => NativeMediaAdjustment::Seek { seconds },
                    MiniAction::Volume { value } => NativeMediaAdjustment::Volume { value },
                    _ => unreachable!(),
                };
                self.native_media_adjust(target, adjustment).await?;
            }
            MiniAction::Mode { .. } => {
                unreachable!("mode context handled before generic adjustment")
            }
            _ => {
                let target = displayed
                    .target
                    .as_ref()
                    .ok_or(Error::InvalidInput("mini media unavailable"))?;
                let action = match action {
                    MiniAction::Toggle => NativeMediaAction::Toggle,
                    MiniAction::Previous => NativeMediaAction::Previous,
                    MiniAction::Next => NativeMediaAction::Next,
                    MiniAction::Stop => NativeMediaAction::Stop,
                    _ => unreachable!(),
                };
                self.native_media_action(target, action).await?;
            }
        }
        Ok(())
    }
}
