//! Device-local policy for ordinary song-row double-clicks.
use super::super::focus::{FocusSnapshot, Owner};
use super::{
    Cancellation, MusicClient, PlaybackCommand, PlaybackSnapshot, SessionToken,
    identity::online_song,
};
use crate::{
    Error, Result,
    client::dto::AccountData,
    playback::{
        QueueEntry, QueueSnapshot,
        controller::{PlaybackFence, PlaybackStatus, QueueTransport, QueueUpdate},
    },
    storage::StateStore,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

const OWNER: &str = "local-device";
const KIND: &str = "playback-queue-click-preference-v1";
const DAMAGED: &str = "queue click settings are damaged";

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Preference {
    revision: u64,
    replace_queue: bool,
}
impl Default for Preference {
    fn default() -> Self {
        Self {
            revision: 0,
            replace_queue: true,
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueClickPreferencesView {
    revision: Option<String>,
    damaged: bool,
    replace_queue: bool,
}
impl From<Preference> for QueueClickPreferencesView {
    fn from(value: Preference) -> Self {
        Self {
            revision: Some(value.revision.to_string()),
            damaged: false,
            replace_queue: value.replace_queue,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueueClickPreferencesCommand {
    Change {
        expected_revision: String,
        replace_queue: bool,
    },
    Repair,
}

fn current(value: Option<Value>, counter: Option<&Value>) -> Result<Preference> {
    match (value, counter) {
        (None, None) => Ok(Preference::default()),
        (Some(value), Some(counter)) => {
            let pref: Preference = serde_json::from_value(value)?;
            if counter.as_u64() != Some(pref.revision) || pref.revision == 0 {
                return Err(Error::State(DAMAGED));
            }
            Ok(pref)
        }
        _ => Err(Error::State(DAMAGED)),
    }
}
fn read(store: &StateStore) -> Result<Preference> {
    store.transaction(|tx| {
        let counter = tx.get(OWNER, KIND, "revision")?;
        current(tx.get(OWNER, KIND, "current")?, counter.as_ref())
    })
}
fn change(store: &StateStore, command: QueueClickPreferencesCommand) -> Result<Preference> {
    store.transaction(|tx| {
        let counter = tx.get(OWNER, KIND, "revision")?;
        let (revision, replace_queue) = match command {
            QueueClickPreferencesCommand::Change {
                expected_revision,
                replace_queue,
            } => {
                let active = current(tx.get(OWNER, KIND, "current")?, counter.as_ref())?;
                if active.revision.to_string() != expected_revision {
                    return Err(Error::State("queue click settings changed"));
                }
                (active.revision, replace_queue)
            }
            QueueClickPreferencesCommand::Repair => {
                let text = tx.get_raw(OWNER, KIND, "current")?;
                let parsed = text.as_deref().map(serde_json::from_str).transpose();
                if let Ok(value) = &parsed
                    && current(value.clone(), counter.as_ref()).is_ok()
                {
                    return Err(Error::State("queue click settings changed"));
                }
                let raw: Value = match parsed {
                    Ok(Some(value)) => value,
                    Ok(None) => json!({"missing":true}),
                    Err(_) => json!({"current_raw":text}),
                };
                let revision = counter
                    .as_ref()
                    .and_then(Value::as_u64)
                    .filter(|revision| *revision > 0)
                    .ok_or(Error::State(DAMAGED))?
                    .max(raw["revision"].as_u64().unwrap_or(0));
                tx.put(
                    OWNER,
                    "playback-queue-click-recovery-v1",
                    &uuid::Uuid::new_v4().simple().to_string(),
                    &raw,
                )?;
                (revision, true)
            }
        };
        let next = Preference {
            revision: revision
                .checked_add(1)
                .ok_or(Error::State("queue click settings revision exhausted"))?,
            replace_queue,
        };
        tx.put(OWNER, KIND, "revision", &next.revision.into())?;
        tx.put(OWNER, KIND, "current", &serde_json::to_value(next)?)?;
        Ok(next)
    })
}

impl MusicClient {
    pub(super) fn replace_queue_on_double_click(&self, token: &SessionToken) -> Result<bool> {
        self.session.with_local_owner(token, |_| {
            read(&self.player.store)
                .map(|pref| pref.replace_queue)
                .map_err(|error| {
                    // A damaged saved setting must not silently clear a queue.
                    if matches!(error, Error::Json(_)) {
                        Error::State(DAMAGED)
                    } else {
                        error
                    }
                })
        })
    }

    pub fn queue_click_preferences(
        &self,
        epoch: &str,
    ) -> Result<AccountData<QueueClickPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            let value = match read(&self.player.store) {
                Ok(pref) => pref.into(),
                Err(Error::Json(_) | Error::State(DAMAGED)) => QueueClickPreferencesView {
                    revision: None,
                    damaged: true,
                    replace_queue: true,
                },
                Err(error) => return Err(error),
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub fn queue_click_preferences_command(
        &self,
        epoch: &str,
        command: QueueClickPreferencesCommand,
    ) -> Result<AccountData<QueueClickPreferencesView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            let value = change(&self.player.store, command)?.into();
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RetainedRow {
    Catalog,
    Search,
}

pub(super) struct QueueClickSnapshot {
    pub(super) focus: FocusSnapshot,
    pub(super) attempt: u64,
    before: PlaybackSnapshot,
    queue: QueueSnapshot,
}
impl MusicClient {
    pub(super) async fn queue_click_snapshot(
        &self,
        token: &SessionToken,
    ) -> Result<QueueClickSnapshot> {
        let observed = self.player.selection.load(Ordering::Acquire);
        let focus = self.focus.snapshot()?;
        let (before, queue) = self
            .player
            .controller()
            .await?
            .queue_with_state(token)
            .await?;
        if before.fm || queue.intelligence.is_some() || self.together_attached(token, None)? {
            return Err(Error::State("queue click requires an ordinary queue"));
        }
        let attempt = observed
            .checked_add(1)
            .ok_or(Error::State("playback selection exhausted"))?;
        self.player
            .selection
            .compare_exchange(observed, attempt, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::StaleResponse)?;
        Ok(QueueClickSnapshot {
            before,
            queue,
            focus,
            attempt,
        })
    }

    pub(super) async fn toggle_retained_search_track(
        &self,
        token: &SessionToken,
        snapshot: &QueueClickSnapshot,
        id: &str,
        lease: u64,
    ) -> Result<Option<PlaybackSnapshot>> {
        let before = &snapshot.before;
        if !before
            .selected
            .as_ref()
            .is_some_and(|entry| online_song(&entry.resource, id))
            || !matches!(
                before.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
            )
        {
            return Ok(None);
        }
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(token, None)? {
            return Err(Error::State("queue click requires an ordinary queue"));
        }
        self.submit_retained_row(
            token,
            PlaybackCommand::Pause(!before.paused),
            before.fence(),
            snapshot.attempt,
            lease,
        )
        .await
        .map(Some)
    }

    pub(super) async fn play_retained_queue_row(
        &self,
        token: &SessionToken,
        snapshot: QueueClickSnapshot,
        (resource, source): (Value, Value),
        lease: u64,
        policy: RetainedRow,
    ) -> Result<PlaybackSnapshot> {
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(token, None)? {
            return Err(Error::State("queue click requires an ordinary queue"));
        }
        let QueueClickSnapshot {
            before,
            mut queue,
            attempt,
            ..
        } = snapshot;
        let id = resource["id"]
            .as_str()
            .ok_or(Error::State("selected song has no identifier"))?;
        let existing = queue
            .items
            .iter()
            .position(|entry| entry.visible_in_queue() && online_song(&entry.resource, id));
        let entry = if let Some(index) = existing {
            let entry = &mut queue.items[index];
            let rebind = policy == RetainedRow::Search && entry.source != source;
            if rebind || entry.resource["missing"] == true {
                entry.resource = resource;
                entry.source = source;
            }
            let id = entry.entry_id.clone();
            if rebind && queue.current.as_ref() != Some(&id) {
                queue.order.retain(|entry_id| entry_id != &id);
                let entry = queue.items.remove(index);
                insert_after_current(&mut queue, entry);
            }
            id
        } else {
            if queue.items.len() >= 50_000 {
                return Err(Error::InvalidInput("playback queue is full"));
            }
            let entry_id = uuid::Uuid::new_v4().simple().to_string();
            let entry = QueueEntry {
                entry_id: entry_id.clone(),
                resource,
                source,
            };
            if policy == RetainedRow::Search {
                insert_after_current(&mut queue, entry);
            } else {
                queue.order.push(entry_id.clone());
                queue.items.push(entry);
            }
            entry_id
        };
        let expected = before.fence();
        let command = if queue.current.as_ref() == Some(&entry)
            && before.selected.as_ref() == queue.selected().as_ref()
        {
            // Resume an already selected song without replacing its decoder.
            PlaybackCommand::Play
        } else {
            queue.current = Some(entry);
            PlaybackCommand::UpdateQueue(QueueUpdate {
                expected: expected.clone(),
                queue,
                transport: Some(QueueTransport {
                    position: None,
                    paused: false,
                }),
            })
        };
        self.submit_retained_row(token, command, expected, attempt, lease)
            .await
    }

    async fn submit_retained_row(
        &self,
        token: &SessionToken,
        command: PlaybackCommand,
        expected: PlaybackFence,
        attempt: u64,
        lease: u64,
    ) -> Result<PlaybackSnapshot> {
        let selection = self.player.selection.clone();
        let focus = self.focus.clone();
        self.player
            .controller()
            .await?
            .command_guarded(
                token,
                command,
                expected,
                Cancellation::new(move || {
                    selection.load(Ordering::Acquire) == attempt
                        && focus.check(lease, Owner::Audio).is_ok()
                }),
            )
            .await
    }
}

fn insert_after_current(queue: &mut QueueSnapshot, entry: QueueEntry) {
    let order_next = queue
        .order
        .iter()
        .position(|id| Some(id) == queue.current.as_ref())
        .map_or(0, |index| index + 1);
    let display_next = queue
        .items
        .iter()
        .position(|item| Some(&item.entry_id) == queue.current.as_ref())
        .map_or(0, |index| index + 1);
    queue.order.insert(order_next, entry.entry_id.clone());
    queue.items.insert(display_next, entry);
}
