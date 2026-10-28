use super::actor::Actor;
use super::*;
use serde_json::json;
impl Actor {
    pub(super) async fn command(&mut self, command: VideoCommand) -> Result<()> {
        match command {
            VideoCommand::Open {
                id,
                resolution,
                download_task_id,
            } => {
                self.open(
                    resolve::Load {
                        id,
                        resolution,
                        task: download_task_id,
                        position: None,
                        remote: false,
                    },
                    true,
                )
                .await
            }
            VideoCommand::Quality { resolution } => {
                let id = self
                    .view
                    .id
                    .clone()
                    .ok_or(Error::State("no video selected"))?;
                self.open(
                    resolve::Load {
                        id,
                        resolution,
                        task: None,
                        position: self.view.position,
                        remote: true,
                    },
                    !self.view.paused,
                )
                .await
            }
            VideoCommand::Refresh => {
                let id = self
                    .view
                    .id
                    .clone()
                    .ok_or(Error::State("no video selected"))?;
                self.open(
                    resolve::Load {
                        id,
                        resolution: self.view.requested_resolution,
                        task: None,
                        position: self.view.position,
                        remote: true,
                    },
                    !self.view.paused,
                )
                .await
            }
            VideoCommand::Play => self.pause(false).await,
            VideoCommand::PlayFromStart => {
                if matches!(
                    self.view.status,
                    VideoStatus::Idle | VideoStatus::Ended | VideoStatus::Error
                ) {
                    let id = self
                        .view
                        .id
                        .clone()
                        .ok_or(Error::State("no video selected"))?;
                    self.open(
                        resolve::Load {
                            id,
                            resolution: self.view.requested_resolution,
                            task: None,
                            position: Some(0.0),
                            remote: false,
                        },
                        true,
                    )
                    .await
                } else {
                    self.pause(false).await
                }
            }
            VideoCommand::Pause { paused } => self.pause(paused).await,
            VideoCommand::Suspend => {
                if self.pending.is_some() {
                    self.cancel_load();
                    self.local_source = None;
                    self.waiting_local = false;
                    self.view.status = VideoStatus::Idle;
                    self.view.buffering = false;
                    self.view.paused = true;
                    self.wants_play = false;
                    Ok(())
                } else {
                    self.pause(true).await
                }
            }
            VideoCommand::Seek { seconds } => {
                if !seconds.is_finite()
                    || seconds < 0.0
                    || self.view.duration.is_none_or(|d| seconds > d)
                {
                    return Err(Error::InvalidInput("video seek is outside duration"));
                }
                let engine = self
                    .engine
                    .clone()
                    .ok_or(Error::State("video is not loaded"))?;
                let observed = device(engine, move |e| {
                    e.seek(seconds)?;
                    e.poll()
                })
                .await?;
                self.view.position = observed.position.or(self.view.position);
                self.view.seek_revision = self.view.control_revision;
                self.checkpoint(false)
            }
            VideoCommand::Volume { value } => self.preference(value, true).await,
            VideoCommand::Speed { value } => self.preference(value, false).await,
            VideoCommand::Stop => {
                let saved = self.checkpoint(self.view.status == VideoStatus::Ended);
                self.cancel_load();
                let stopped = self.release_engine().await;
                self.local_source = None;
                self.waiting_local = false;
                self.wants_play = false;
                self.view.status = VideoStatus::Idle;
                self.view.buffering = false;
                self.view.paused = true;
                saved.and(stopped)
            }
            VideoCommand::Snapshot => Ok(()),
        }
    }
    async fn pause(&mut self, paused: bool) -> Result<()> {
        if self.view.status == VideoStatus::Ended && paused {
            return Ok(());
        }
        if matches!(
            self.view.status,
            VideoStatus::Ended | VideoStatus::Idle | VideoStatus::Error
        ) && !paused
        {
            let id = self
                .view
                .id
                .clone()
                .ok_or(Error::State("no video selected"))?;
            return self
                .open(
                    resolve::Load {
                        id,
                        resolution: self.view.requested_resolution,
                        task: None,
                        position: if self.view.status == VideoStatus::Ended {
                            Some(0.0)
                        } else {
                            self.view.position
                        },
                        remote: false,
                    },
                    true,
                )
                .await;
        }
        if let Some(engine) = self.engine.clone() {
            let observed = device(engine, move |e| {
                e.pause(paused)?;
                e.poll()
            })
            .await?;
            self.view.position = observed.position.or(self.view.position);
            self.view.status = if paused {
                VideoStatus::Paused
            } else {
                VideoStatus::Playing
            };
            self.checkpoint(false)?;
        } else if self.pending.is_none() && !self.waiting_local && !paused {
            return Err(Error::State("video is not loaded"));
        }
        self.view.paused = paused;
        self.wants_play = !paused;
        Ok(())
    }
    async fn preference(&mut self, value: f64, volume: bool) -> Result<()> {
        if !value.is_finite()
            || volume && !(0.0..=100.0).contains(&value)
            || !volume && !(0.25..=4.0).contains(&value)
        {
            return Err(Error::InvalidInput("video preference is out of bounds"));
        }
        let before = if volume {
            self.view.volume
        } else {
            self.view.speed
        };
        if let Some(engine) = self.engine.clone() {
            device(engine, move |e| {
                if volume {
                    e.volume(value)
                } else {
                    e.speed(value)
                }
            })
            .await?;
        }
        let next_volume = if volume { value } else { self.view.volume };
        let next_speed = if volume { self.view.speed } else { value };
        if let Err(error) = self.session.save_result(
            &self.token,
            "mv-player-v1",
            "settings",
            &json!({"volume":next_volume,"speed":next_speed}),
        ) {
            if let Some(engine) = self.engine.clone()
                && device(engine, move |e| {
                    if volume {
                        e.volume(before)
                    } else {
                        e.speed(before)
                    }
                })
                .await
                .is_err()
            {
                // Do not advertise old preferences while an unconfirmed live
                // output remains at the rejected value.
                self.cancel_load();
                let _ = self.release_engine().await;
                self.wants_play = false;
                self.failure(Error::State("video preference rollback failed"));
            }
            return Err(error);
        }
        self.view.volume = next_volume;
        self.view.speed = next_speed;
        Ok(())
    }
}
