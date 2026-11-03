//! A local-only playback session, separate from account queues and online history.
use super::{
    MusicClient,
    focus::{MediaFocus, Owner},
};
use crate::{
    Error, Result,
    account::profiles::ColdOfflineGrant,
    download::offline::{ColdOfflineAudioRow, ColdOfflineCursor},
    membership::ColdEntitlement,
    playback::engine::{AudioEngine, Cancellation, EndReason, EngineEvent},
};
use serde::Serialize;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
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
    entitlement: Option<ColdEntitlement>,
}
impl Drop for Playing {
    fn drop(&mut self) {
        let _ = self.engine.stop();
    }
}
struct Mode {
    generation: u64,
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
            .list_audio(if next { self.next.as_ref() } else { None }, 32)?;
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
        if let Some(entitlement) = &active.entitlement
            && let Err(error) = self.grant.validate_entitlement(entitlement)
        {
            self.playing = None;
            return Err(error);
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
    close_generation: Arc<AtomicU64>,
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
    async fn run_current<T: Send + 'static>(
        self: &Arc<Self>,
        operation: impl FnOnce(&mut Mode) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let generation = self.close_generation.load(Ordering::Acquire);
        let closes = self.close_generation.clone();
        self.run(move |state| {
            // An older queued status/next/stop/play must never apply to a
            // grant opened after its caller left the previous mode.
            if closes.load(Ordering::Acquire) != generation {
                return Err(Error::StaleResponse);
            }
            operation(valid_mode(state)?)
        })
        .await
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
                if life.is_cancelled() {
                    break;
                }
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
        && let Err(error) = state.life.check().and_then(|()| state.grant.validate())
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
        let generation = monitor.close_generation.load(Ordering::Acquire);
        self.cold_offline
            .run(move |state| {
                if monitor.close_generation.load(Ordering::Acquire) != generation {
                    return Err(Error::StaleResponse);
                }
                if state.is_some() {
                    return Err(Error::MutationBusy);
                }
                let grant = Arc::new(registry.grant_cold_offline(session)?);
                let closes = monitor.close_generation.clone();
                let life = Cancellation::new(move || closes.load(Ordering::Acquire) == generation);
                let mut mode = Mode {
                    generation,
                    grant: grant.clone(),
                    life: life.clone(),
                    next: None,
                    playing: None,
                    failed: false,
                };
                let page = mode.page(false)?;
                life.check()?;
                *state = Some(mode);
                monitor.monitor(grant, life);
                Ok(page)
            })
            .await
    }
    pub async fn cold_offline_next(&self) -> Result<ColdOfflinePageView> {
        self.cold_offline.run_current(|mode| mode.page(true)).await
    }
    pub async fn cold_offline_status(&self) -> Result<ColdOfflinePlayerView> {
        self.cold_offline.run_current(Mode::poll).await
    }
    pub async fn cold_offline_stop(&self) -> Result<ColdOfflinePlayerView> {
        self.cold_offline
            .run_current(|mode| {
                mode.playing = None;
                mode.failed = false;
                Ok(ColdOfflinePlayerView::default())
            })
            .await
    }
    pub async fn cold_offline_close(&self) -> Result<()> {
        // Revoke the mode immediately, even while a decoder owns the worker
        // mutex. The decoder may still be preparing its first audio frame.
        let generation = self
            .cold_offline
            .close_generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| Error::State("offline playback generation exhausted"))?
            + 1;
        self.cold_offline
            .run(move |state| {
                if state
                    .as_ref()
                    .is_some_and(|mode| mode.generation < generation)
                {
                    *state = None;
                }
                Ok(())
            })
            .await
    }
    pub async fn cold_offline_play(&self, id: String) -> Result<ColdOfflinePlayerView> {
        let token = self.session.token()?;
        let task_id = id.clone();
        let (grant, life, file, entitlement) = self
            .cold_offline
            .run_current(move |mode| {
                let (file, entitlement) = mode.grant.open_audio(&task_id)?.into_playback()?;
                mode.life.check()?;
                Ok((mode.grant.clone(), mode.life.clone(), file, entitlement))
            })
            .await?;
        // Snapshot before checking the mode so a newer focus claim cannot be
        // overwritten if this play is delayed after its file was verified.
        let expected = self.focus.snapshot()?;
        life.check()?;
        let revision = self
            .claim_native_media(&token, Owner::ColdOffline, expected)
            .await?;
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
                if let Some(rights) = &entitlement {
                    mode.grant.validate_entitlement(rights)?;
                }
                let fast_rights = entitlement.clone();
                let authority = Cancellation::new(move || {
                    !life.is_cancelled()
                        && focus.cold_is_current(revision)
                        && fast_rights.as_ref().is_none_or(ColdEntitlement::valid)
                });
                let mut engine = factory()?;
                if let Some(volume) = volume {
                    engine.volume(volume)?;
                }
                let media = if let Some(rights) = &entitlement {
                    let media = engine.load_pinned_ncm(file, authority.clone(), &authority)?;
                    mode.grant.validate_entitlement(rights)?;
                    media
                } else {
                    engine.load_pinned_audio(file, authority.clone(), &authority)?
                };
                mode.grant.validate()?;
                lease.focus.start_cold(revision, || {
                    authority.check()?;
                    engine.pause(false)
                })?;
                mode.playing = Some(Playing {
                    engine,
                    lease,
                    entry: media.entry_id,
                    id,
                    entitlement,
                });
                mode.poll()
            })
            .await
    }
}
