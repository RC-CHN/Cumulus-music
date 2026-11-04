//! A local-only playback session, separate from account queues and online history.
use super::{
    MusicClient,
    focus::{MediaFocus, Owner},
    player::EngineFactory,
};
use crate::{
    Error, Result,
    account::profiles::ColdOfflineGrant,
    download::offline::{ColdOfflineAudioRow, ColdOfflineCursor, ColdOfflineFlacReader},
    effects::material::MaterialLimiterPlan,
    membership::ColdEntitlement,
    playback::engine::{
        AudioEngine, Cancellation, ColdLimiterConfig, ColdLimiterRender, ColdLimiterSource,
        EndReason, EngineEvent,
    },
};
use serde::Serialize;
use std::{
    path::PathBuf,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_reason: Option<StopReason>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    limiter_active: bool,
}
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
enum Phase {
    #[default]
    Idle,
    Playing,
    Paused,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum StopReason {
    CachedRightsUnavailable,
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
    authority: Cancellation,
    entry: i64,
    id: String,
    entitlement: Option<ColdEntitlement>,
    limiter: Option<Limited>,
    paused: bool,
    volume: Option<f64>,
}
struct Limited {
    source: ColdLimiterSource,
    origin: f64,
}
impl Drop for Playing {
    fn drop(&mut self) {
        let _ = self.engine.stop();
    }
}
impl Playing {
    fn set_pause(&mut self, paused: bool) -> Result<()> {
        self.authority.check()?;
        if paused {
            self.engine.pause(true)?;
        } else {
            self.lease.focus.start_cold(self.lease.revision, || {
                self.authority.check()?;
                self.engine.pause(false)
            })?;
        }
        self.authority.check()?;
        self.paused = paused;
        Ok(())
    }

    fn position(&self, native: f64) -> f64 {
        self.limiter.as_ref().map_or(native, |limited| {
            (limited.origin + native).min(limited.source.duration())
        })
    }
}
struct Mode {
    generation: u64,
    grant: Arc<ColdOfflineGrant>,
    life: Cancellation,
    next: Option<ColdOfflineCursor>,
    playing: Option<Playing>,
    failed: bool,
    stop_reason: Option<StopReason>,
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
    fn stop_for_cached_rights(&mut self) -> ColdOfflinePlayerView {
        self.playing = None;
        self.stop_reason = Some(StopReason::CachedRightsUnavailable);
        self.idle()
    }

    fn idle(&self) -> ColdOfflinePlayerView {
        ColdOfflinePlayerView {
            stop_reason: self.stop_reason,
            ..Default::default()
        }
    }

    fn poll(&mut self) -> Result<ColdOfflinePlayerView> {
        if self.failed {
            return Err(Error::Media("offline decoder stopped unexpectedly"));
        }
        let Some(active) = self.playing.as_mut() else {
            return Ok(self.idle());
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
            && self.grant.validate_entitlement(entitlement).is_err()
        {
            return Ok(self.stop_for_cached_rights());
        }
        if active.authority.is_cancelled() {
            self.playing = None;
            return Err(Error::Cancelled);
        }
        let snapshot = active.engine.poll();
        // The stream can revoke its own reads while engine.poll is running.
        // Classify that stop before interpreting its decoder error or events.
        if active
            .entitlement
            .as_ref()
            .is_some_and(|rights| !rights.valid())
        {
            return Ok(self.stop_for_cached_rights());
        }
        if active.authority.is_cancelled() {
            self.playing = None;
            return Err(Error::Cancelled);
        }
        let snapshot = match snapshot {
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
            phase: if active.paused {
                Phase::Paused
            } else {
                Phase::Playing
            },
            playing_id: Some(active.id.clone()),
            position: snapshot
                .position_for(active.entry)
                .filter(|p| p.is_finite() && *p >= 0.0)
                .map(|position| active.position(position)),
            stop_reason: None,
            limiter_active: active.limiter.is_some(),
        })
    }

    fn seek(
        &mut self,
        id: &str,
        seconds: f64,
        factory: EngineFactory,
    ) -> Result<ColdOfflinePlayerView> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(Error::InvalidInput("invalid cold offline seek position"));
        }
        self.poll()?;
        let active = self
            .playing
            .as_mut()
            .ok_or(Error::State("offline audio is not playing"))?;
        if active.id != id {
            return Err(Error::StaleResponse);
        }
        let Some(limited) = &mut active.limiter else {
            active.engine.seek(seconds)?;
            return self.poll();
        };
        let first = limited.source.frame(seconds)?;
        let target = ColdLimiterSource::seconds(first);
        // Keep the old entry but suspend output during the blocking render.
        // The monitor cannot run while this operation holds the mode lock.
        // An unsuccessful render restores the prior user intent only after
        // current profile, focus and persisted rights are checked again.
        active.engine.pause(true)?;
        let prepared = limited.source.render(first, &active.authority);
        if let Err(error) = self.grant.validate() {
            self.playing = None;
            return Err(error);
        }
        self.poll()?;
        let next = match prepared {
            Ok(next) => next,
            Err(error) => {
                if let Some(active) = &mut self.playing
                    && let Err(restore) = active.set_pause(active.paused)
                {
                    self.playing = None;
                    self.failed = true;
                    return Err(restore);
                }
                return Err(error);
            }
        };
        let mut active = self.playing.take().ok_or(Error::StaleResponse)?;
        active.engine.stop()?;
        if !active.engine.supports_cold_limiter() {
            // A rejected Stop synchronously destroys mpv. Recreate its
            // output only after that destruction, using the same host.
            active.engine = factory()?;
            if let Some(volume) = active.volume {
                active.engine.volume(volume)?;
            }
        }
        let media = match next {
            ColdLimiterRender::Pcm(wave) => {
                let media = active.engine.load_cold_limiter(wave, &active.authority)?;
                active
                    .limiter
                    .as_mut()
                    .expect("active limiter source")
                    .origin = target;
                media
            }
            ColdLimiterRender::Stock => {
                let source = active.limiter.take().expect("active limiter source").source;
                active.engine.load_cold_stock_flac(
                    source.into_reader(),
                    target,
                    &active.authority,
                )?
            }
        };
        active.entry = media.entry_id;
        self.grant.validate()?;
        if let Some(rights) = &active.entitlement {
            self.grant.validate_entitlement(rights)?;
        }
        active.set_pause(active.paused)?;
        self.playing = Some(active);
        self.poll()
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
    /// Explicit native-only final limiter opt-in for eligible cold downloads.
    /// The host must trust the absolute SDK path and be authorized to use the
    /// recovered material. Neither renderer IPC nor environment selects this.
    pub fn with_native_cold_offline_limiter(
        mut self,
        library: PathBuf,
        material: MaterialLimiterPlan,
    ) -> Result<Self> {
        self.cold_limiter = Some(Arc::new(ColdLimiterConfig::new(library, material)?));
        Ok(self)
    }

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
                    stop_reason: None,
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
    /// Native transport control, scoped to the current cold mode and focus.
    pub async fn cold_offline_pause(
        &self,
        id: String,
        paused: bool,
    ) -> Result<ColdOfflinePlayerView> {
        self.cold_offline
            .run_current(move |mode| {
                mode.poll()?;
                let active = mode
                    .playing
                    .as_mut()
                    .ok_or(Error::State("offline audio is not playing"))?;
                if active.id != id {
                    return Err(Error::StaleResponse);
                }
                active.set_pause(paused)?;
                mode.poll()
            })
            .await
    }

    /// Native absolute source-clock seek. No task/path is selected by a seek.
    pub async fn cold_offline_seek(
        &self,
        id: String,
        seconds: f64,
    ) -> Result<ColdOfflinePlayerView> {
        let factory = self.player.factory.clone();
        self.cold_offline
            .run_current(move |mode| mode.seek(&id, seconds, factory))
            .await
    }
    pub async fn cold_offline_stop(&self) -> Result<ColdOfflinePlayerView> {
        self.cold_offline
            .run_current(|mode| {
                mode.playing = None;
                mode.failed = false;
                mode.stop_reason = None;
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
        let (grant, life, file, entitlement, format) = self
            .cold_offline
            .run_current(move |mode| {
                let (file, entitlement, format) =
                    mode.grant.open_audio(&task_id)?.into_playback()?;
                mode.life.check()?;
                Ok((
                    mode.grant.clone(),
                    mode.life.clone(),
                    file,
                    entitlement,
                    format,
                ))
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
        let limiter_config = self.cold_limiter.clone();
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
                mode.stop_reason = None;
                let life = mode.life.clone();
                let focus = lease.focus.clone();
                if let Some(rights) = &entitlement {
                    mode.grant.validate_entitlement(rights)?;
                }
                let fast_rights = entitlement.clone();
                let fast_grant = mode.grant.clone();
                let authority = Cancellation::new(move || {
                    !life.is_cancelled()
                        && focus.cold_is_current(revision)
                        && fast_grant.playback_current()
                        && fast_rights.as_ref().is_none_or(ColdEntitlement::valid)
                });
                let mut engine = factory()?;
                if let Some(volume) = volume {
                    engine.volume(volume)?;
                }
                let runtime = if format == "flac" && engine.supports_cold_limiter() {
                    limiter_config
                        .as_ref()
                        .and_then(|config| config.available())
                } else {
                    None
                };
                let mut limiter = None;
                let media = if let Some(runtime) = runtime {
                    let reader = ColdOfflineFlacReader::new(
                        grant.clone(),
                        file,
                        entitlement.clone(),
                        authority.clone(),
                    )?;
                    match runtime.select(reader)? {
                        Ok(mut source) => match source.render(0, &authority)? {
                            ColdLimiterRender::Pcm(wave) => {
                                let media = engine.load_cold_limiter(wave, &authority)?;
                                limiter = Some(Limited {
                                    source,
                                    origin: 0.0,
                                });
                                media
                            }
                            ColdLimiterRender::Stock => engine.load_cold_stock_flac(
                                source.into_reader(),
                                0.0,
                                &authority,
                            )?,
                        },
                        Err(reader) => engine.load_cold_stock_flac(reader, 0.0, &authority)?,
                    }
                } else if entitlement.is_some() {
                    engine.load_pinned_ncm(file, authority.clone(), &authority)?
                } else {
                    engine.load_pinned_audio(file, authority.clone(), &authority)?
                };
                if let Some(rights) = &entitlement {
                    mode.grant.validate_entitlement(rights)?;
                }
                mode.grant.validate()?;
                lease.focus.start_cold(revision, || {
                    authority.check()?;
                    engine.pause(false)
                })?;
                mode.playing = Some(Playing {
                    engine,
                    lease,
                    authority,
                    entry: media.entry_id,
                    id,
                    entitlement,
                    limiter,
                    paused: false,
                    volume,
                });
                mode.poll()
            })
            .await
    }
}
