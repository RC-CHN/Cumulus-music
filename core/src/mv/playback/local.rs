//! Follow the durable download journal, never a stale path or a UI callback.
use super::{actor::Actor, *};
use crate::download::{playback::SourceState, service::DownloadStatus};
use std::path::PathBuf;

pub(super) struct LocalSource {
    pub task: String,
    pub path: PathBuf,
}

impl Actor {
    pub(super) async fn sync_local(&mut self) -> Result<()> {
        let Some(source) = &self.local_source else {
            return Ok(());
        };
        let state = crate::download::playback::state(&self.session, &self.token, &source.task)?;
        let changed = match &state {
            SourceState::Ready(task) => task.target.as_ref() != Some(&source.path),
            SourceState::Changing | SourceState::Missing => true,
        };
        if changed && !self.waiting_local {
            // Persist the last observed clock before releasing all decoder/frame
            // references. A file worker cannot unlink until its lease is free.
            let saved = self.checkpoint(self.view.status == VideoStatus::Ended);
            self.cancel_load();
            let stopped = self.release_engine().await;
            self.waiting_local = true;
            self.view.status = VideoStatus::FileOperation;
            self.view.buffering = false;
            if !self.wants_play {
                self.view.paused = true;
            }
            saved.and(stopped)?;
        }
        if !self.waiting_local {
            return Ok(());
        }
        match state {
            SourceState::Changing => Ok(()),
            SourceState::Missing => {
                // Delete and Forget retire the downloaded source. Neither may
                // silently fall back to a network stream after the operation.
                self.local_source = None;
                self.waiting_local = false;
                self.wants_play = false;
                self.view.status = VideoStatus::Idle;
                self.view.paused = true;
                self.view.buffering = false;
                Ok(())
            }
            SourceState::Ready(task) => {
                if !task.origin.is_mv() || task.status != DownloadStatus::Complete {
                    return Err(Error::State("local video task is no longer complete"));
                }
                self.open(
                    resolve::Load {
                        id: task.song_id,
                        resolution: self.view.requested_resolution,
                        task: Some(task.id),
                        position: self.view.position,
                        remote: false,
                    },
                    self.wants_play,
                )
                .await
            }
        }
    }
}
