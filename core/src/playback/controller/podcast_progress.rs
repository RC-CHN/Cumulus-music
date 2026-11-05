//! A voice play record (M/308, M/71) belongs to the departed native play,
//! independent of local history, ordinary analytics and device handoff.
use super::state::Actor;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    history::{HistoryService, HistorySyncAction},
    playback::TimelineMode,
    podcast::progress::{playrecord, program_id},
    protocol::api::Api,
};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::{sync::mpsc, task::JoinHandle};

pub(super) struct Capture {
    api: Arc<dyn Api>,
    owner: SessionToken,
    revision: String,
    program_id: u64,
    play_id: String,
}

impl Capture {
    pub(super) fn new(
        session: &AccountSession,
        owner: &SessionToken,
        resource: &Value,
        play_id: &str,
    ) -> Option<Self> {
        let program_id = program_id(resource)?;
        owner.uid.as_ref()?;
        let api = session.native_api(owner).ok()?;
        let revision = api.podcast_progress_revision().ok()??;
        Some(Self {
            api,
            owner: owner.clone(),
            revision,
            program_id,
            play_id: play_id.to_owned(),
        })
    }

    pub(super) fn reload(&mut self, id: &str) {
        self.play_id = id.to_owned();
    }
}

struct Write {
    captured: Capture,
    payload: Value,
}

fn sync_enabled(session: Arc<AccountSession>, token: &SessionToken) -> Result<bool> {
    Ok(HistoryService::new(session)?
        .sync_setting_for(token, HistorySyncAction::Read)?
        .enabled)
}

impl Write {
    async fn send(self, session: &Arc<AccountSession>, shutdown: &Arc<AtomicBool>) -> Result<()> {
        let Self { captured, payload } = self;
        let mut changes = session.subscribe()?;
        let captured = &captured;
        let request = session.run_for(&captured.owner, |api, _| async move {
            if !Arc::ptr_eq(&api, &captured.api) {
                return Err(Error::StaleResponse);
            }
            let session = session.clone();
            let owner = captured.owner.clone();
            let bound = captured.api.clone();
            let shutdown = shutdown.clone();
            let allowed = move || {
                if shutdown.load(Ordering::Acquire) {
                    return Ok(false);
                }
                session.require_api(&owner, &bound)?;
                sync_enabled(session.clone(), &owner)
            };
            if !allowed()? {
                return Ok(());
            }
            api.upload_podcast_progress(payload, &captured.revision, &allowed)
                .await
        });
        tokio::select! {
            biased;
            _ = changes.wait_for(|token| token != &captured.owner) => Ok(()),
            result = request => result,
        }
    }
}

pub(super) struct Publisher {
    sender: mpsc::Sender<Write>,
    task: JoinHandle<()>,
}

impl Publisher {
    fn new(session: Arc<AccountSession>, shutdown: Arc<AtomicBool>) -> Self {
        // Keep writes in departure order without allowing a slow network to
        // block local playback or grow an unbounded history/identity queue.
        let (sender, mut pending) = mpsc::channel::<Write>(16);
        let task = tokio::spawn(async move {
            while let Some(write) = pending.recv().await {
                // A failed/ambiguous write is never automatically repeated.
                let _ = write.send(&session, &shutdown).await;
            }
        });
        Self { sender, task }
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Actor {
    /// Take exactly one checkpoint from the current, successfully played
    /// decoder before a terminal event discards its clock or its identity.
    pub(super) fn depart_podcast(&mut self, completed: bool) {
        let Some(loaded) = self.loaded.as_mut() else {
            return;
        };
        let Some(captured) = loaded.podcast_progress.take() else {
            return;
        };
        if self.play_id.as_deref() != Some(&captured.play_id)
            || self.token != captured.owner
            || program_id(&loaded.item.resource) != Some(captured.program_id)
            || loaded.timeline.snapshot().mode != TimelineMode::Full
            || !sync_enabled(self.session.clone(), &self.token).unwrap_or(false)
        {
            return;
        }
        if let Some(duration) = loaded.timeline.display_end()
            && let Some(payload) = playrecord(
                captured.program_id,
                self.position.unwrap_or(0.0),
                duration,
                completed,
            )
        {
            let publisher = self
                .podcast_publisher
                .get_or_insert_with(|| Publisher::new(self.session.clone(), self.shutdown.clone()));
            let _ = publisher.sender.try_send(Write { captured, payload });
        }
    }
}
