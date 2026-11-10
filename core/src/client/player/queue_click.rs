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
    Local,
    OfflineDownload,
    DailyRecommendation,
    SearchCatalog,
    SearchMenuPlay,
    SearchSong,
    SearchVoice,
}

pub(super) struct QueueClickSnapshot {
    pub(super) focus: FocusSnapshot,
    pub(super) attempt: u64,
    before: PlaybackSnapshot,
    queue: QueueSnapshot,
}

impl QueueClickSnapshot {
    pub(super) fn matches(&self, expected: &PlaybackFence, observed: u64) -> bool {
        self.before.fence() == *expected && observed.checked_add(1) == Some(self.attempt)
    }
}
pub(super) struct AlbumTableRows {
    pub resources: Vec<Value>,
    pub index: usize,
    pub source: Value,
    pub double_click: bool,
}

#[derive(Clone, Copy)]
pub(super) enum TableSong<'a> {
    Catalog(&'a str),
    IndexedLocal(&'a Value),
    OfflineDownload(&'a Value),
}

fn same_offline_download(entry: &Value, row: &Value) -> bool {
    let marker = crate::download::offline::OFFLINE_PLAYLIST_DOWNLOAD;
    entry[marker].is_object()
        && entry[marker] == row[marker]
        && entry["id"] == row["id"]
        && entry["download_task_id"] == row["download_task_id"]
}

fn current_table_song(before: &PlaybackSnapshot, song: TableSong<'_>) -> bool {
    !before.fm
        && before.play_id.is_some()
        && before.selected.as_ref().is_some_and(|entry| match song {
            TableSong::Catalog(id) => online_song(&entry.resource, id),
            TableSong::IndexedLocal(row) => {
                entry.resource["local_id"] == row["local_id"]
                    && entry.resource["local_revision"] == row["local_revision"]
                    && entry.resource["local_id"].is_string()
            }
            TableSong::OfflineDownload(row) => same_offline_download(&entry.resource, row),
        })
        && matches!(
            before.state,
            PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
        )
}

impl MusicClient {
    /// M/3085 toggles the current ID before applying a double-click queue
    /// replacement. Call after validating the source row, before hydration.
    pub(super) async fn toggle_active_song_row(
        &self,
        token: &SessionToken,
        song: TableSong<'_>,
        attempt: u64,
        lease: u64,
        source_guard: &Cancellation,
        retained: Option<&QueueClickSnapshot>,
    ) -> Result<Option<PlaybackSnapshot>> {
        let (before, intelligence) = if let Some(snapshot) = retained {
            (
                snapshot.before.clone(),
                snapshot.queue.intelligence.is_some(),
            )
        } else {
            let Some(controller) = self.player.controller.get() else {
                return Ok(None);
            };
            if !current_table_song(&controller.snapshot(), song) {
                return Ok(None);
            }
            let (before, queue) = controller.queue_with_state(token).await?;
            (before, queue.intelligence.is_some())
        };
        if !current_table_song(&before, song) || intelligence {
            return Ok(None);
        }
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(token, None)? {
            return Ok(None);
        }
        if source_guard.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        self.submit_retained_row(
            token,
            PlaybackCommand::Pause(!before.paused),
            before.fence(),
            attempt,
            lease,
            Some(source_guard.clone()),
        )
        .await
        .map(Some)
    }

    pub(super) async fn play_retained_album_rows(
        &self,
        token: &SessionToken,
        snapshot: QueueClickSnapshot,
        rows: AlbumTableRows,
        lease: u64,
        source_guard: Cancellation,
    ) -> Result<PlaybackSnapshot> {
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(token, None)? {
            return Err(Error::State(
                "album table playback requires an ordinary queue",
            ));
        }
        if source_guard.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        let QueueClickSnapshot {
            before,
            mut queue,
            attempt,
            ..
        } = snapshot;
        let AlbumTableRows {
            resources,
            index,
            source,
            double_click,
        } = rows;
        let id = resources
            .get(index)
            .and_then(|row| row["id"].as_str())
            .ok_or(Error::QueueChanged)?
            .to_owned();
        let expected = before.fence();
        let command = if current_table_song(&before, TableSong::Catalog(&id)) {
            if double_click {
                PlaybackCommand::Pause(!before.paused)
            } else {
                PlaybackCommand::Play
            }
        } else {
            queue = super::queue_playlist::insert_collection_bound(
                queue, resources, source, false, true, false,
            );
            let entry = queue
                .items
                .iter()
                .find(|entry| online_song(&entry.resource, &id))
                .ok_or(Error::State("selected album song exceeds queue limit"))?;
            queue.current = Some(entry.entry_id.clone());
            PlaybackCommand::UpdateQueue(QueueUpdate {
                expected: expected.clone(),
                queue,
                transport: Some(QueueTransport {
                    position: None,
                    paused: false,
                }),
            })
        };
        self.submit_retained_row(token, command, expected, attempt, lease, Some(source_guard))
            .await
    }

    pub(super) async fn queue_click_snapshot(
        &self,
        token: &SessionToken,
    ) -> Result<QueueClickSnapshot> {
        self.queue_click_snapshot_for(token, None).await
    }

    pub(super) async fn search_menu_play_snapshot(
        &self,
        token: &SessionToken,
        play_id: Option<&str>,
        queue_revision: &str,
    ) -> Result<QueueClickSnapshot> {
        self.queue_click_snapshot_for(token, Some((play_id, queue_revision)))
            .await
    }

    async fn queue_click_snapshot_for(
        &self,
        token: &SessionToken,
        expected: Option<(Option<&str>, &str)>,
    ) -> Result<QueueClickSnapshot> {
        let observed = self.player.selection.load(Ordering::Acquire);
        let focus = self.focus.snapshot()?;
        let (before, queue) = self
            .player
            .controller()
            .await?
            .queue_with_state(token)
            .await?;
        if expected.is_some_and(|(play_id, revision)| {
            before.play_id.as_deref() != play_id || before.queue_revision.to_string() != revision
        }) {
            return Err(Error::QueueChanged);
        }
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
            None,
        )
        .await
        .map(Some)
    }

    pub(super) async fn resume_retained_search_menu_track(
        &self,
        token: &SessionToken,
        snapshot: &QueueClickSnapshot,
        id: &str,
        lease: u64,
    ) -> Result<Option<PlaybackSnapshot>> {
        if !current_table_song(&snapshot.before, TableSong::Catalog(id)) {
            return Ok(None);
        }
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(token, None)? {
            return Err(Error::State("search menu requires an ordinary queue"));
        }
        // Explicit menu Play resumes or leaves the active decoder playing.
        // It never toggles Pause or rebinds the current source for a new scene.
        self.submit_retained_row(
            token,
            PlaybackCommand::PlaySearchCatalog(None),
            snapshot.before.fence(),
            snapshot.attempt,
            lease,
            None,
        )
        .await
        .map(Some)
    }

    pub(super) async fn play_retained_queue_row(
        &self,
        token: &SessionToken,
        snapshot: QueueClickSnapshot,
        resource: (Value, Value),
        lease: u64,
        policy: RetainedRow,
    ) -> Result<PlaybackSnapshot> {
        self.play_retained_queue_row_with_source(token, snapshot, resource, lease, policy, None)
            .await
    }

    pub(super) async fn play_retained_queue_row_with_source(
        &self,
        token: &SessionToken,
        snapshot: QueueClickSnapshot,
        (resource, source): (Value, Value),
        lease: u64,
        policy: RetainedRow,
        source_guard: Option<crate::playback::engine::Cancellation>,
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
        let identity = match policy {
            RetainedRow::SearchVoice => "voice_id",
            RetainedRow::Local => "local_id",
            _ => "id",
        };
        let id = resource[identity]
            .as_str()
            .ok_or(Error::State("selected resource has no identifier"))?;
        if policy == RetainedRow::SearchMenuPlay {
            // M/3085 skips pure-list reuse for an explicit menu action.
            // M/1384 deduplicates by resource at the current display slot.
            let selected = id.to_owned();
            let queue = super::next_play::search_group_queue(
                queue,
                vec![resource],
                source,
                Some(&selected),
                &mut rand::rng(),
            )?;
            let expected = before.fence();
            return self
                .submit_retained_row(
                    token,
                    PlaybackCommand::PlaySearchCatalog(Some(QueueUpdate {
                        expected: expected.clone(),
                        queue,
                        transport: Some(QueueTransport {
                            position: None,
                            paused: false,
                        }),
                    })),
                    expected,
                    attempt,
                    lease,
                    source_guard,
                )
                .await;
        }
        if policy == RetainedRow::DailyRecommendation {
            let expected = before.fence();
            // M/308 offsets a non-clearing row play to the current display
            // position; M/1384 moves an older copy into that insertion slot.
            let selected = id.to_owned();
            let mut source = source;
            source["single_row"] = json!(true);
            let mut queue = super::queue_playlist::insert_collection_bound(
                queue,
                vec![resource],
                source,
                false,
                true,
                true,
            );
            queue.current = Some(
                queue
                    .items
                    .iter()
                    .find(|entry| online_song(&entry.resource, &selected))
                    .ok_or(Error::State("missing inserted Daily recommendation"))?
                    .entry_id
                    .clone(),
            );
            let command = PlaybackCommand::UpdateQueue(QueueUpdate {
                expected: expected.clone(),
                queue,
                transport: Some(QueueTransport {
                    position: None,
                    paused: false,
                }),
            });
            return self
                .submit_retained_row(token, command, expected, attempt, lease, source_guard)
                .await;
        }
        let existing = queue.items.iter().position(|entry| {
            entry.visible_in_queue()
                && match policy {
                    RetainedRow::SearchVoice => {
                        super::identity::kind(&entry.resource) == super::PlayerResourceKind::Voice
                            && entry.resource["voice_id"].as_str() == Some(id)
                    }
                    RetainedRow::Local => entry.resource["local_id"].as_str() == Some(id),
                    RetainedRow::OfflineDownload => {
                        same_offline_download(&entry.resource, &resource)
                    }
                    _ => online_song(&entry.resource, id),
                }
        });
        let search = !matches!(
            policy,
            RetainedRow::Catalog | RetainedRow::Local | RetainedRow::OfflineDownload
        );
        let entry = if let Some(index) = existing {
            let entry = &mut queue.items[index];
            let rebind = search && entry.source != source;
            let qualified_playlist = super::playlist_trial::inferred_playlist_source(&source);
            // When retaining the queue, selecting an already queued *other*
            // song from this verified playlist keeps its slot but uses the
            // newly selected playlist authorization context. Never reload the
            // currently selected decoder just to change its annotation.
            let rebind_playlist = qualified_playlist
                && queue.current.as_ref() != Some(&entry.entry_id)
                && (entry.source != source || entry.resource["trial_mode"] != source["trial_mode"]);
            let rebind_local = policy == RetainedRow::Local && entry.resource != resource;
            if rebind || rebind_playlist || rebind_local || entry.resource["missing"] == true {
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
            if search {
                insert_after_current(&mut queue, entry);
            } else {
                queue.order.push(entry_id.clone());
                queue.items.push(entry);
            }
            entry_id
        };
        let expected = before.fence();
        let unchanged = queue.current.as_ref() == Some(&entry)
            && before.selected.as_ref() == queue.selected().as_ref();
        let update = (!unchanged).then(|| {
            queue.current = Some(entry);
            QueueUpdate {
                expected: expected.clone(),
                queue,
                transport: Some(QueueTransport {
                    position: None,
                    paused: false,
                }),
            }
        });
        let command = if policy == RetainedRow::SearchCatalog {
            PlaybackCommand::PlaySearchCatalog(update)
        } else {
            // Resume an already selected song without replacing its decoder.
            update.map_or(PlaybackCommand::Play, PlaybackCommand::UpdateQueue)
        };
        self.submit_retained_row(token, command, expected, attempt, lease, source_guard)
            .await
    }

    pub(super) async fn submit_retained_row(
        &self,
        token: &SessionToken,
        command: PlaybackCommand,
        expected: PlaybackFence,
        attempt: u64,
        lease: u64,
        source_guard: Option<crate::playback::engine::Cancellation>,
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
                        && source_guard
                            .as_ref()
                            .is_none_or(|source| !source.is_cancelled())
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
