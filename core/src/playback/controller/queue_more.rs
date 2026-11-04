//! Official queue triggers and owner-fenced native recommendation publication.
use super::{Message, PlaybackStatus, state::Actor};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::queue_more::{QueueMoreRead, QueueMoreSong},
    playback::{
        PlaybackMode,
        queue_more::{KIND, PREFERENCE, QueueMoreView, recommended, seed},
    },
    protocol::pc_experiments::PcExperimentGroups,
};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};
use tokio::task::JoinHandle;
use uuid::Uuid;

const ASSIGNMENT: &str = "PH-PC-recommendMore";
const DEFAULT_OFF: &str = "PH-PC-recMoreOff";
const READ_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Default)]
pub(super) struct Coordinator {
    pub supported: bool,
    owner: Option<SessionToken>,
    groups: Option<PcExperimentGroups>,
    assignment: Option<JoinHandle<()>>,
    pending: Option<Pending>,
    deferred_toggle: bool,
    observation: Option<Observation>,
    played: HashSet<String>,
}

#[derive(PartialEq, Eq)]
struct Observation {
    root: [u8; 16],
    playing: bool,
    play_id: Option<String>,
    len: usize,
}

struct Pending {
    task: JoinHandle<()>,
    id: Uuid,
    owner: SessionToken,
    revision: u64,
    root: [u8; 16],
    play_id: Option<String>,
    current: Option<String>,
    seed_count: usize,
}

pub(super) struct GroupsReady {
    owner: SessionToken,
    groups: Result<PcExperimentGroups>,
}
pub(super) struct RowsReady {
    id: Uuid,
    rows: Result<Vec<QueueMoreSong>>,
}

impl Coordinator {
    pub(super) fn abort(&mut self) {
        if let Some(task) = self.assignment.take() {
            task.abort();
        }
        if let Some(pending) = self.pending.take() {
            pending.task.abort();
        }
    }
}

impl Actor {
    pub(super) fn queue_more_available(&self) -> bool {
        self.queue_more.supported
            && self.queue_more.owner.as_ref() == Some(&self.token)
            && self
                .queue_more
                .groups
                .as_ref()
                .and_then(|groups| groups.group(ASSIGNMENT))
                == Some("t1")
    }

    fn queue_more_enabled(&self) -> Result<bool> {
        let uid = self
            .token
            .uid
            .as_deref()
            .ok_or(Error::State("queue recommendations require an account"))?;
        let default = self
            .queue_more
            .groups
            .as_ref()
            .and_then(|groups| groups.group(DEFAULT_OFF))
            != Some("t1");
        Ok(self
            .store
            .get(uid, KIND, PREFERENCE)?
            .and_then(|value| value.as_bool())
            .unwrap_or(default))
    }

    pub(super) fn queue_more_view(&self) -> Option<QueueMoreView> {
        if !self.queue_more_available() || self.queue.more_snapshot().is_none() {
            return None;
        }
        self.queue.more_view(self.queue_more_enabled().ok()?)
    }

    pub(super) fn queue_more_action(&mut self, revision: u64, toggle: Option<bool>) -> Result<()> {
        if revision != self.queue.revision() {
            return Err(Error::QueueChanged);
        }
        let view = self
            .queue_more_view()
            .ok_or(Error::State("queue does not support recommendations"))?;
        if let Some(enabled) = toggle {
            if enabled != view.enabled {
                let session = self.session.clone();
                session.with_token(&self.token, |_| self.queue.set_more_preference(enabled))?;
                if enabled {
                    if self.queue_more.pending.is_some() {
                        self.queue_more.deferred_toggle = true;
                    } else {
                        self.fetch_queue_more();
                    }
                } else {
                    self.queue_more.deferred_toggle = false;
                }
            }
        } else if view.prefetch_on_open {
            self.fetch_queue_more();
        }
        Ok(())
    }

    pub(super) fn observe_queue_more(&mut self) {
        if !self.queue_more.supported {
            return;
        }
        if self.queue_more.owner.as_ref() != Some(&self.token) {
            self.queue_more.abort();
            self.queue_more = Coordinator {
                supported: true,
                owner: Some(self.token.clone()),
                ..Default::default()
            };
        }
        let Some(root) = self.queue.more_snapshot().map(|origin| origin.nonce) else {
            self.queue_more.observation = None;
            self.queue_more.played.clear();
            return;
        };
        if self.token.uid.is_none() {
            return;
        }
        if self.queue_more.groups.is_none() {
            if self.queue_more.assignment.is_none() {
                let session = self.session.clone();
                let owner = self.token.clone();
                let sender = self.sender.clone();
                self.queue_more.assignment = Some(tokio::spawn(async move {
                    let groups = tokio::time::timeout(
                        READ_TIMEOUT,
                        session.run_for(&owner, |api, _| async move {
                            api.queue_more_experiment_groups().await
                        }),
                    )
                    .await
                    .unwrap_or(Err(Error::State("queue experiment read timed out")));
                    let _ = sender.send(Message::QueueMoreGroups(GroupsReady { owner, groups }));
                }));
            }
            return;
        }
        if !self.queue_more_available()
            || self.queue.is_fm()
            || self.queue.intelligence_context().is_some()
        {
            return;
        }
        let previous = self.queue_more.observation.as_ref();
        let playing = self.state == PlaybackStatus::Playing
            || self.state == PlaybackStatus::Buffering
                && previous.is_some_and(|before| before.playing && before.play_id == self.play_id);
        let observation = Observation {
            root,
            playing,
            play_id: self.play_id.clone(),
            len: self.queue.len(),
        };
        let changed_root = previous.is_none_or(|before| before.root != root);
        let entered = playing
            && previous.is_none_or(|before| {
                changed_root || !before.playing || before.play_id != observation.play_id
            });
        let removed = !changed_root && previous.is_some_and(|before| observation.len < before.len);
        self.queue_more.observation = Some(observation);
        if changed_root {
            self.queue_more.played.clear();
            self.queue_more.deferred_toggle = false;
        }
        if !entered && !removed {
            return;
        }
        let queue = self.queue.snapshot();
        if let Some(current) = queue.current.as_ref().filter(|_| entered) {
            self.queue_more.played.insert(current.clone());
        }
        if removed {
            let existing: HashSet<_> = queue.items.iter().map(|entry| &entry.entry_id).collect();
            self.queue_more.played.retain(|id| existing.contains(id));
        }
        let finished = match queue.mode {
            PlaybackMode::Random => queue
                .items
                .iter()
                .all(|entry| self.queue_more.played.contains(&entry.entry_id)),
            _ => queue
                .items
                .last()
                .is_some_and(|entry| Some(&entry.entry_id) == queue.current.as_ref()),
        };
        if finished {
            self.fetch_queue_more();
        }
    }

    pub(super) fn queue_more_groups(&mut self, value: GroupsReady) {
        if self.queue_more.owner.as_ref() != Some(&value.owner) || self.token != value.owner {
            return;
        }
        self.queue_more.assignment = None;
        if self.session.check_generation(&value.owner).is_ok() {
            self.queue_more.groups = Some(
                value
                    .groups
                    .unwrap_or_else(|_| PcExperimentGroups::from_startup_cache(&Value::Null)),
            );
        }
    }

    fn fetch_queue_more(&mut self) {
        if !self.queue_more_view().is_some_and(|view| view.enabled) {
            return;
        }
        if let Some(pending) = &self.queue_more.pending {
            if self.queue.revision() != pending.revision || self.play_id != pending.play_id {
                self.queue_more.deferred_toggle = true;
            }
            return;
        }
        let queue = self.queue.snapshot();
        let Some(origin) = &queue.more else { return };
        let Some(current) = queue.selected() else {
            return;
        };
        // Do not forward a private local/download current identity. The
        // recovered material only guarantees this online track projection.
        if ["local_id", "download_task_id", "cloud_song_id", "voice_id"]
            .iter()
            .any(|key| current.resource.get(key).is_some())
        {
            return;
        }
        let Some(current_track) = seed(&current) else {
            return;
        };
        let seeds = queue.items.iter().filter_map(seed).collect::<Vec<_>>();
        if seeds.is_empty() || seeds.len() > 10_000 {
            return;
        }
        let seed_count = seeds.len();
        let owner = self.token.clone();
        let task_owner = owner.clone();
        let service = QueueMoreRead::new(self.session.clone());
        let sender = self.sender.clone();
        let id = Uuid::new_v4();
        let task = tokio::spawn(async move {
            let rows = tokio::time::timeout(
                READ_TIMEOUT,
                service.for_queue(&task_owner, &seeds, &current_track),
            )
            .await
            .unwrap_or(Err(Error::State("queue recommendation read timed out")));
            let _ = sender.send(Message::QueueMoreRows(RowsReady { id, rows }));
        });
        self.queue_more.pending = Some(Pending {
            task,
            id,
            owner,
            revision: self.queue.revision(),
            root: origin.nonce,
            play_id: self.play_id.clone(),
            current: queue.current,
            seed_count,
        });
    }

    pub(super) async fn queue_more_rows(&mut self, value: RowsReady) {
        if self
            .queue_more
            .pending
            .as_ref()
            .is_none_or(|pending| pending.id != value.id)
        {
            return;
        }
        let pending = self
            .queue_more
            .pending
            .take()
            .expect("checked pending recommendation");
        if self.rebind().await.is_err()
            || self.token != pending.owner
            || self.sync_queue().await.is_err()
        {
            self.queue_more.deferred_toggle = false;
            return;
        }
        if let Ok(rows) = value.rows
            && self.queue.revision() == pending.revision
            && self.play_id == pending.play_id
            && self
                .queue
                .more_snapshot()
                .is_some_and(|origin| origin.nonce == pending.root)
            && self.queue_more_view().is_some_and(|view| view.enabled)
            && let Some(current) = self
                .queue
                .selected()
                .filter(|entry| Some(&entry.entry_id) == pending.current.as_ref())
        {
            let level = current.resource["level"].as_str().unwrap_or("standard");
            let rows = rows
                .into_iter()
                .map(|song| recommended(song, level))
                .collect();
            let session = self.session.clone();
            // An unsuccessful CAS, changed account or room cannot publish
            // rows, interrupt the decoder, or become a playback error.
            let _ = session.with_token(&pending.owner, |_| {
                self.queue.insert_more(rows, pending.seed_count)
            });
        }
        if std::mem::take(&mut self.queue_more.deferred_toggle) {
            self.fetch_queue_more();
        }
    }
}
