//! Native FM/audio coordination. Slow recommendation I/O does not hold the
//! playback actor, so a later stop, selection or account change remains final.
mod controls;
mod runtime;
mod state;
use super::*;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    membership::Quality,
    playback::{
        PlaybackMode, QueueEntry, QueueSnapshot,
        controller::{
            PlaybackCommand, PlaybackController, PlaybackSnapshot, PlaybackStatus, QueueTransport,
            QueueUpdate,
        },
    },
    storage::StateStore,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Mutex;

pub struct FmPlayer {
    inner: Arc<Inner>,
    worker: tokio::task::JoinHandle<()>,
}
struct Inner {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    service: Arc<FmService>,
    controller: Arc<PlaybackController>,
    intents: Arc<AtomicU64>,
    gate: Mutex<()>,
}
impl Drop for FmPlayer {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
impl FmPlayer {
    pub fn new(
        session: Arc<AccountSession>,
        controller: Arc<PlaybackController>,
        intents: Arc<AtomicU64>,
    ) -> Result<Self> {
        let inner = Arc::new(Inner {
            store: session.persistent_store()?,
            service: Arc::new(FmService::new(session.clone())?),
            session,
            controller,
            intents,
            gate: Mutex::new(()),
        });
        let worker = tokio::spawn(runtime::watch(inner.clone()));
        Ok(Self { inner, worker })
    }
    pub fn service(&self) -> &Arc<FmService> {
        &self.inner.service
    }
    /// Empty recommendations still own the FM screen and its saved normal
    /// queue. Explicit ordinary queue replacement clears this marker atomically.
    pub fn active_source_for(
        &self,
        token: &SessionToken,
        queue: &QueueSnapshot,
    ) -> Result<Option<FmSource>> {
        self.inner.active_source(token, queue)
    }
    pub fn pending_for(&self, token: &SessionToken, queue: &QueueSnapshot) -> Result<bool> {
        let Some(source) = self.active_source_for(token, queue)? else {
            return Ok(false);
        };
        let Some(desired) = self.inner.service.queue_for(token, &source)? else {
            return Ok(true);
        };
        Ok(!matches_queue(&desired, queue))
    }
    pub fn failure_for(&self, token: &SessionToken) -> Result<Option<String>> {
        self.inner.session.with_token(token, |uid| {
            Ok(self
                .inner
                .store
                .get(uid, "fm-player-v1", "failure")?
                .and_then(|v| v["code"].as_str().map(str::to_owned)))
        })
    }
    pub async fn enter_for(
        &self,
        token: &SessionToken,
        source: FmSource,
        quality: Quality,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        self.inner.enter(token, source, quality, intent).await
    }
    pub async fn step_for(
        &self,
        token: &SessionToken,
        step: FmStep,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        self.step_with_state_for(token, step, intent, false).await
    }
    pub(crate) async fn step_with_state_for(
        &self,
        token: &SessionToken,
        step: FmStep,
        intent: u64,
        preserve_state: bool,
    ) -> Result<PlaybackSnapshot> {
        let _gate = self
            .inner
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        self.inner.step(token, step, intent, preserve_state).await
    }
    pub async fn resume_for(
        &self,
        token: &SessionToken,
        restore: bool,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        self.resume_with_start_for(token, restore, intent, false)
            .await
    }
    pub(crate) async fn resume_with_start_for(
        &self,
        token: &SessionToken,
        restore: bool,
        intent: u64,
        restart_stopped: bool,
    ) -> Result<PlaybackSnapshot> {
        self.inner
            .resume(token, restore, intent, restart_stopped)
            .await
    }
    pub async fn startup_for(
        &self,
        token: &SessionToken,
        input: crate::playback::controller::StartupRestore,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        self.inner.startup(token, input, intent).await
    }
    pub async fn repeat_for(
        &self,
        token: &SessionToken,
        repeat: FmRepeat,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        self.inner.repeat(token, repeat, intent).await
    }
    pub async fn filter_for(&self, token: &SessionToken, intent: u64) -> Result<PlaybackSnapshot> {
        self.inner.filter(token, intent).await
    }
    pub async fn leave_for(&self, token: &SessionToken, intent: u64) -> Result<PlaybackSnapshot> {
        self.inner.leave(token, intent).await
    }
    pub async fn trash_for(
        &self,
        token: &SessionToken,
        entry_id: &str,
        intent: u64,
    ) -> Result<FmTrashMutation> {
        self.inner.trash(token, entry_id, None, intent).await
    }
    pub async fn feedback_for(
        &self,
        token: &SessionToken,
        entry_id: &str,
        reason_key: &str,
        intent: u64,
    ) -> Result<FmTrashMutation> {
        self.inner
            .trash(token, entry_id, Some(reason_key), intent)
            .await
    }
    pub async fn apply_confirmed_trash_for(
        &self,
        token: &SessionToken,
        id: &str,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        self.inner.apply_trash(token, id, intent).await
    }
}
pub fn source(queue: &QueueSnapshot) -> Result<Option<FmSource>> {
    let Some(first) = queue.items.first() else {
        return Ok(None);
    };
    if first.source["kind"] != "fm" {
        return Ok(None);
    }
    if queue.items.iter().any(|e| e.source != first.source) {
        return Err(Error::State("mixed FM queue sources"));
    }
    let source: FmSource = serde_json::from_value(first.source["source"].clone())?;
    source.validate()?;
    Ok(Some(source))
}
fn projection(queue: &FmQueue, quality: &str) -> Result<QueueSnapshot> {
    Quality::parse(quality)?;
    let source = serde_json::json!({"kind":"fm","source":queue.source});
    let items: Vec<_> = queue
        .tracks
        .iter()
        .map(|t| QueueEntry {
            entry_id: t.entry_id.clone(),
            source: source.clone(),
            resource: serde_json::json!({"id":t.id,"song":t.song,"level":quality,"trial_mode":queue.source.playback_trial_mode()}),
        })
        .collect();
    Ok(QueueSnapshot {
        order: items.iter().map(|e| e.entry_id.clone()).collect(),
        current: queue.current().map(|t| t.entry_id.clone()),
        items,
        mode: if queue.repeat == FmRepeat::One {
            PlaybackMode::SingleLoop
        } else {
            PlaybackMode::Sequential
        },
    })
}
fn matches_queue(desired: &FmQueue, actual: &QueueSnapshot) -> bool {
    actual.current == desired.current().map(|t| t.entry_id.clone())
        && actual.order
            == desired
                .tracks
                .iter()
                .map(|t| t.entry_id.clone())
                .collect::<Vec<_>>()
        && actual.mode
            == if desired.repeat == FmRepeat::One {
                PlaybackMode::SingleLoop
            } else {
                PlaybackMode::Sequential
            }
}
fn quality(view: &PlaybackSnapshot) -> &str {
    view.requested_quality.as_deref().unwrap_or("standard")
}
fn observation(view: &PlaybackSnapshot) -> Option<FmObservation> {
    view.selected.as_ref().map(|e| FmObservation {
        entry_id: e.entry_id.clone(),
        position_seconds: view.position.unwrap_or(0.0),
    })
}
