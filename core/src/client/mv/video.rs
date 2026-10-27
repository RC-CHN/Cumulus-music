use super::*;
use crate::{mv::playback::VideoCommand, playback::engine::VideoFrame};
impl MusicClient {
    pub async fn video_snapshot(&self, epoch: &str) -> Result<VideoView> {
        let token = self.mv.token(epoch)?;
        self.mv
            .controller(&token)
            .await?
            .snapshot()
            .map(view::video)
    }
    pub async fn video_action(&self, epoch: &str, action: VideoAction) -> Result<VideoView> {
        let token = self.mv.token(epoch)?;
        if let VideoAction::Open {
            id,
            resolution,
            download_task_id,
        } = &action
        {
            crate::protocol::api::identifier(&serde_json::json!(id))?;
            if *resolution == 0
                || *resolution > 8640
                || download_task_id.as_ref().is_some_and(|id| id.is_empty())
            {
                return Err(Error::InvalidInput("invalid MV playback selection"));
            }
        }
        if matches!(
            action,
            VideoAction::Play | VideoAction::Pause { paused: false }
        ) && self.mv.controller(&token).await?.snapshot()?.id.is_none()
        {
            return Err(Error::State("no video selected"));
        }
        let lease = if matches!(
            action,
            VideoAction::Open { .. } | VideoAction::Play | VideoAction::Pause { paused: false }
        ) {
            Some(self.claim_media(&token, Owner::Video).await?)
        } else {
            None
        };
        let command = match action {
            VideoAction::Open {
                id,
                resolution,
                download_task_id,
            } => VideoCommand::Open {
                id,
                resolution,
                download_task_id,
            },
            VideoAction::Play => VideoCommand::Play,
            VideoAction::Pause { paused } => VideoCommand::Pause { paused },
            VideoAction::Seek { seconds } => VideoCommand::Seek { seconds },
            VideoAction::Volume { value } => VideoCommand::Volume { value },
            VideoAction::Speed { value } => VideoCommand::Speed { value },
            VideoAction::Quality { resolution } => VideoCommand::Quality { resolution },
            VideoAction::Refresh => VideoCommand::Refresh,
            VideoAction::Stop => VideoCommand::Stop,
        };
        if let Some(lease) = lease {
            self.focus.check(lease, Owner::Video)?;
        }
        self.mv
            .controller(&token)
            .await?
            .command(&token, command)
            .await
            .map(view::video)
    }
    pub fn video_frame(
        &self,
        epoch: &str,
        play_id: &str,
        after: u64,
        width: u32,
        height: u32,
    ) -> Result<Option<Arc<VideoFrame>>> {
        let token = self.mv.token(epoch)?;
        let Some(controller) = self.mv.existing(&token)? else {
            return Ok(None);
        };
        controller.frame(&token, play_id, after, width, height)
    }
}
