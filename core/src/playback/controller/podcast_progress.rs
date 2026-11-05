//! A voice play record (M/308, M/71) belongs to the departed native play,
//! independent of local history, ordinary analytics and device handoff.
use super::state::Actor;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    history::HistoryService,
    playback::TimelineMode,
    podcast::progress::{latest, playrecord, program_id},
    protocol::api::Api,
};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::{sync::mpsc, task::JoinHandle};

struct Remote {
    api: Arc<dyn Api>,
    revision: String,
    program_id: u64,
}

pub(super) struct Capture {
    owner: SessionToken,
    play_id: String,
    latest: Option<latest::Captured>,
    remote: Option<Remote>,
}

impl Capture {
    pub(super) fn new(
        session: &AccountSession,
        owner: &SessionToken,
        resource: &Value,
        play_id: &str,
    ) -> Option<Self> {
        owner.uid.as_ref()?;
        let latest = latest::Captured::from_resource(resource);
        let remote = (|| {
            let program_id = program_id(resource)?;
            let api = session.native_api(owner).ok()?;
            let revision = api.podcast_progress_revision().ok()??;
            Some(Remote {
                api,
                revision,
                program_id,
            })
        })();
        (latest.is_some() || remote.is_some()).then(|| Self {
            owner: owner.clone(),
            play_id: play_id.to_owned(),
            latest,
            remote,
        })
    }

    pub(super) fn reload(&mut self, id: &str) {
        self.play_id = id.to_owned();
    }
}

struct Write {
    owner: SessionToken,
    remote: Remote,
    payload: Value,
    sync_revision: u64,
}

fn sync_revision(session: Arc<AccountSession>, token: &SessionToken) -> Result<Option<u64>> {
    HistoryService::new(session)?.enabled_sync_revision_for(token)
}

impl Write {
    async fn send(self, session: &Arc<AccountSession>, shutdown: &Arc<AtomicBool>) -> Result<()> {
        let Self {
            owner,
            remote,
            payload,
            sync_revision: queued_revision,
        } = self;
        let mut changes = session.subscribe()?;
        let remote = &remote;
        let captured_owner = &owner;
        let request = session.run_for(captured_owner, |api, _| async move {
            if !Arc::ptr_eq(&api, &remote.api) {
                return Err(Error::StaleResponse);
            }
            let session = session.clone();
            let owner = captured_owner.clone();
            let bound = remote.api.clone();
            let shutdown = shutdown.clone();
            let allowed = move || {
                if shutdown.load(Ordering::Acquire) {
                    return Ok(false);
                }
                session.require_api(&owner, &bound)?;
                Ok(sync_revision(session.clone(), &owner)? == Some(queued_revision))
            };
            if !allowed()? {
                return Ok(());
            }
            api.upload_podcast_progress(payload, &remote.revision, &allowed)
                .await
        });
        tokio::select! {
            biased;
            _ = changes.wait_for(|token| token != captured_owner) => Ok(()),
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
            || loaded.timeline.snapshot().mode != TimelineMode::Full
        {
            return;
        }
        let Some(duration) = loaded.timeline.display_end() else {
            return;
        };
        let position = self.position.unwrap_or(0.0);
        if let Some(latest) = captured.latest
            && latest.matches(&loaded.item.resource)
        {
            // Local continuation is independent of multi-device privacy, the
            // signed progress write capability, and its network outcome.
            let _ = self.session.with_token(&self.token, |owner| {
                self.store
                    .transaction(|tx| latest.save(tx, owner, position, duration, completed))
            });
        }
        if let Some(remote) = captured.remote
            && program_id(&loaded.item.resource) == Some(remote.program_id)
            && let Ok(Some(sync_revision)) = sync_revision(self.session.clone(), &self.token)
            && let Some(payload) = playrecord(remote.program_id, position, duration, completed)
        {
            let publisher = self
                .podcast_publisher
                .get_or_insert_with(|| Publisher::new(self.session.clone(), self.shutdown.clone()));
            let _ = publisher.sender.try_send(Write {
                owner: captured.owner,
                remote,
                payload,
                sync_revision,
            });
        }
    }
}
