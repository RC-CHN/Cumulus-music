//! A local-only playback session, separate from account queues and online history.
use super::{
    MusicClient,
    focus::{MediaFocus, Owner},
};
use crate::{
    Error, Result,
    account::profiles::ColdOfflineGrant,
    download::offline::{ColdOfflineAudioRow, ColdOfflineCursor},
    playback::engine::{AudioEngine, Cancellation, EndReason, EngineEvent},
};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColdOfflinePageView {
    items: Vec<ColdOfflineAudioRow>,
    has_more: bool,
}
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ColdOfflinePlayerView {
    phase: Phase,
    playing_id: Option<String>,
    position: Option<f64>,
}
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
enum Phase {
    #[default]
    Idle,
    Playing,
}
struct Lease {
    focus: Arc<MediaFocus>,
    revision: u64,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.focus.release_cold(self.revision);
    }
}
struct Playing {
    engine: Box<dyn AudioEngine>,
    lease: Lease,
    entry: i64,
    id: String,
}
impl Drop for Playing {
    fn drop(&mut self) {
        let _ = self.engine.stop();
    }
}
struct Mode {
    grant: Arc<ColdOfflineGrant>,
    life: Cancellation,
    next: Option<ColdOfflineCursor>,
    playing: Option<Playing>,
    failed: bool,
}
impl Drop for Mode {
    fn drop(&mut self) {
        self.life.cancel();
        self.grant.revoke();
    }
}
impl Mode {
    fn page(&mut self, next: bool) -> Result<ColdOfflinePageView> {
        if next && self.next.is_none() {
            return Err(Error::State("no more offline downloads"));
        }
        let page = self
            .grant
            .list_plain_audio(if next { self.next.as_ref() } else { None }, 32)?;
        self.next = page.next;
        Ok(ColdOfflinePageView {
            items: page.rows,
            has_more: self.next.is_some(),
        })
    }
    fn poll(&mut self) -> Result<ColdOfflinePlayerView> {
        if self.failed {
            return Err(Error::Media("offline decoder stopped unexpectedly"));
        }
        let Some(active) = self.playing.as_mut() else {
            return Ok(ColdOfflinePlayerView::default());
        };
        if active
            .lease
            .focus
            .check(active.lease.revision, Owner::ColdOffline)
            .is_err()
        {
            self.playing = None;
            return Ok(ColdOfflinePlayerView::default());
        }
        let snapshot = match active.engine.poll() {
            Ok(value) => value,
            Err(error) => {
                self.playing = None;
                self.failed = true;
                return Err(error);
            }
        };
        if let Some(failed) = snapshot.events.iter().find_map(|event| match event {
            EngineEvent::Ended {
                entry_id, reason, ..
            } if *entry_id == active.entry => {
                Some(*reason != EndReason::Eof && *reason != EndReason::Stop)
            }
            EngineEvent::OutputReload { entry_id } if *entry_id == active.entry => Some(true),
            EngineEvent::Shutdown => Some(true),
            _ => None,
        }) {
            self.playing = None;
            self.failed = failed;
            if failed {
                return Err(Error::Media("offline decoder stopped unexpectedly"));
            }
            return Ok(ColdOfflinePlayerView::default());
        }
        Ok(ColdOfflinePlayerView {
            phase: Phase::Playing,
            playing_id: Some(active.id.clone()),
            position: snapshot.position.filter(|p| p.is_finite() && *p >= 0.0),
        })
    }
}
#[derive(Default)]
pub(super) struct Host {
    mode: Mutex<Option<Mode>>,
}
impl Host {
    async fn run<T: Send + 'static>(
        self: &Arc<Self>,
        operation: impl FnOnce(&mut Option<Mode>) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let host = self.clone();
        tokio::task::spawn_blocking(move || {
            operation(&mut *host.mode.lock().map_err(|_| Error::LockPoisoned)?)
        })
        .await
        .map_err(|_| Error::State("offline playback worker stopped"))?
    }
    pub(super) async fn stop_engine(self: &Arc<Self>) -> Result<()> {
        if self
            .mode
            .try_lock()
            .is_ok_and(|mode| mode.as_ref().is_none_or(|mode| mode.playing.is_none()))
        {
            return Ok(());
        }
        self.run(|mode| {
            if let Some(mode) = mode {
                mode.playing = None;
            }
            Ok(())
        })
        .await
    }
    fn monitor(self: &Arc<Self>, grant: Arc<ColdOfflineGrant>, life: Cancellation) {
        let host = Arc::downgrade(self);
        tokio::spawn(async move {
            while !life.is_cancelled() {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let Some(host) = host.upgrade() else { break };
                let check = grant.clone();
                let valid = tokio::task::spawn_blocking(move || check.validate().is_ok())
                    .await
                    .unwrap_or(false);
                if !valid {
                    life.cancel();
                }
                let expected = grant.clone();
                let _ = host
                    .run(move |mode| {
                        if let Some(current) =
                            mode.as_mut().filter(|m| Arc::ptr_eq(&m.grant, &expected))
                        {
                            if !valid {
                                *mode = None;
                            } else {
                                let _ = current.poll();
                            }
                        }
                        Ok(())
                    })
                    .await;
            }
        });
    }
}
fn valid_mode(mode: &mut Option<Mode>) -> Result<&mut Mode> {
    if let Some(state) = mode
        && let Err(error) = state.grant.validate()
    {
        *mode = None;
        return Err(error);
    }
    mode.as_mut()
        .ok_or(Error::State("offline downloads are not open"))
}
impl MusicClient {
    pub async fn cold_offline_open(&self) -> Result<ColdOfflinePageView> {
        let registry = self.registry.clone();
        let session = self.session.clone();
        let monitor = self.cold_offline.clone();
        self.cold_offline
            .run(move |state| {
                if state.is_some() {
                    return Err(Error::MutationBusy);
                }
                let grant = Arc::new(registry.grant_cold_offline(session)?);
                let life = Cancellation::default();
                let mut mode = Mode {
                    grant: grant.clone(),
                    life: life.clone(),
                    next: None,
                    playing: None,
                    failed: false,
                };
                let page = mode.page(false)?;
                *state = Some(mode);
                monitor.monitor(grant, life);
                Ok(page)
            })
            .await
    }
    pub async fn cold_offline_next(&self) -> Result<ColdOfflinePageView> {
        self.cold_offline
            .run(|state| valid_mode(state)?.page(true))
            .await
    }
    pub async fn cold_offline_status(&self) -> Result<ColdOfflinePlayerView> {
        self.cold_offline
            .run(|state| valid_mode(state)?.poll())
            .await
    }
    pub async fn cold_offline_stop(&self) -> Result<ColdOfflinePlayerView> {
        self.cold_offline
            .run(|state| {
                let mode = valid_mode(state)?;
                mode.playing = None;
                mode.failed = false;
                Ok(ColdOfflinePlayerView::default())
            })
            .await
    }
    pub async fn cold_offline_close(&self) -> Result<()> {
        if self
            .cold_offline
            .mode
            .try_lock()
            .is_ok_and(|mode| mode.is_none())
        {
            return Ok(());
        }
        self.cold_offline
            .run(|state| {
                *state = None;
                Ok(())
            })
            .await
    }
    pub async fn cold_offline_play(&self, id: String) -> Result<ColdOfflinePlayerView> {
        let token = self.session.token()?;
        let task_id = id.clone();
        let (grant, file) = self
            .cold_offline
            .run(move |state| {
                let mode = valid_mode(state)?;
                let audio = mode.grant.open_plain_audio(&task_id)?;
                Ok((mode.grant.clone(), audio.file()?.try_clone()?))
            })
            .await?;
        let revision = self.claim_media(&token, Owner::ColdOffline).await?;
        let lease = Lease {
            focus: self.focus.clone(),
            revision,
        };
        let factory = self.player.factory.clone();
        let volume = self.player.controller.get().and_then(|controller| {
            let source = controller.subscribe();
            let snapshot = source.borrow();
            (snapshot.account == token).then_some(snapshot.volume)
        });
        self.cold_offline
            .run(move |state| {
                let mode = valid_mode(state)?;
                if !Arc::ptr_eq(&mode.grant, &grant) {
                    return Err(Error::StaleResponse);
                }
                lease.focus.check(revision, Owner::ColdOffline)?;
                mode.playing = None;
                mode.failed = false;
                let life = mode.life.clone();
                let focus = lease.focus.clone();
                let authority = Cancellation::new(move || {
                    !life.is_cancelled() && focus.cold_is_current(revision)
                });
                let mut engine = factory()?;
                if let Some(volume) = volume {
                    engine.volume(volume)?;
                }
                let media = engine.load_pinned_audio(file, authority.clone(), &authority)?;
                mode.grant.validate()?;
                lease.focus.start_cold(revision, || engine.pause(false))?;
                mode.playing = Some(Playing {
                    engine,
                    lease,
                    entry: media.entry_id,
                    id,
                });
                mode.poll()
            })
            .await
    }
}
