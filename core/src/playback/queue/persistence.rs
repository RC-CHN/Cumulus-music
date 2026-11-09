//! CAS-protected publication of queue, quality and coordinator ownership.
use super::*;
use crate::playback::controller::FmRestore;
use serde_json::json;

enum SideEffect<'a> {
    None,
    Fm(FmCommit<'a>),
    More(Option<bool>),
    Recall(i64),
}

struct FmCommit<'a> {
    expected: &'a FmRestore,
    /// None means cleanup only; Some(None) removes the outgoing FM clock.
    position: Option<Option<Value>>,
}

impl PlaybackQueue {
    pub(crate) fn install_recall(&mut self, resources: Vec<Value>, now: i64) -> Result<()> {
        use rand::seq::SliceRandom;
        if !self.state.items.is_empty()
            || self.state.current.is_some()
            || self.state.fm_source.is_some()
            || self.state.intelligence.is_some()
            || resources.is_empty()
            || resources.len() > 12
        {
            return Err(Error::QueueChanged);
        }
        let store = self
            .store
            .as_ref()
            .ok_or(Error::State("transition requires persistent queue"))?;
        recall_due(store.get(&self.owner, RECALL_STATE, RECALL_LAST_TIME)?, now)?;
        let source = json!({"kind":"recall_prefill","scene":"recallPrefill","label":"新召推荐预填",
            "resourceType":"track","originalResourceType":"track"});
        let items = Self::entries(resources, &source);
        let mut order: Vec<_> = items.iter().map(|item| item.entry_id.clone()).collect();
        let mode = self.state.mode;
        if mode == PlaybackMode::Random {
            order.shuffle(&mut self.rng);
        }
        // M/1384 chooses randomly when no explicit song ID was supplied.
        let current = order.first().cloned();
        self.commit_owned_with_side_effect(
            QueueSnapshot {
                items,
                order,
                current,
                mode,
                ..Default::default()
            },
            false,
            None,
            None,
            SideEffect::Recall(now),
        )
    }

    /// Same durable queue snapshot as the task/ledger observation immediately
    /// before a catalog file is submitted to a native decoder.
    pub(crate) fn verify_search_file_target(
        tx: &crate::storage::StateTransaction<'_>,
        owner: &str,
        expected: &QueueEntry,
        song: &str,
    ) -> Result<()> {
        let raw = tx.get(owner, "playback", "queue")?;
        let queue = QueueSnapshot::load(raw.as_ref())?;
        if queue.fm_source.is_some()
            || queue.intelligence.is_some()
            || queue.selected().as_ref() != Some(expected)
            || expected.resource["id"].as_str() != Some(song)
            || !expected.resource["playback_constraint"].is_null()
            || expected.source["kind"] != "songs"
            || expected.source["scene"] != "search"
            || expected.resource["trial_mode"] != 42
        {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }

    pub(crate) fn install_fm_restore(&mut self, input: &FmRestore) -> Result<FmRestoreOutcome> {
        let restore =
            crate::fm::player::source(&self.state)?.is_some() || self.state.items.is_empty();
        let (candidate, position, checkpoint, intelligence) = if restore {
            let raw = input
                .backup_queue
                .as_ref()
                .ok_or(Error::State("FM has no previous queue"))?;
            let queue = QueueSnapshot::load(Some(raw))?;
            if queue.fm_source.is_some()
                || queue.intelligence.is_some()
                || queue.items.iter().any(|item| item.source["kind"] == "fm")
            {
                return Err(Error::State("invalid FM return queue"));
            }
            for item in &queue.items {
                crate::playback::PlaybackConstraint::from_resource(&item.resource)?;
            }
            let position = input
                .backup_position
                .as_ref()
                .filter(|value| {
                    queue.current.is_some()
                        && value["entry_id"].as_str() == queue.current.as_deref()
                })
                .and_then(|value| value["position"].as_f64());
            if position.is_some_and(|position| !position.is_finite() || position < 0.0) {
                return Err(Error::InvalidInput("invalid FM backup position"));
            }
            let checkpoint = position.map(|seconds| {
                json!({
                    "entry_id":queue.current, "seconds":seconds, "paused":true,
                    "state":"paused", "timeline_version":1
                })
            });
            (queue, position, Some(checkpoint), None)
        } else {
            (self.state.clone(), None, None, self.intelligence.clone())
        };
        self.commit_owned_with_side_effect(
            candidate,
            true,
            None,
            intelligence,
            SideEffect::Fm(FmCommit {
                expected: input,
                position: checkpoint,
            }),
        )?;
        Ok(if restore {
            FmRestoreOutcome::Restored(position)
        } else {
            FmRestoreOutcome::AlreadyOrdinary
        })
    }

    pub(super) fn commit(&mut self, candidate: QueueSnapshot) -> Result<()> {
        let release_fm = candidate.fm_source.is_none()
            && (candidate.items.iter().any(|e| e.source["kind"] != "fm")
                || (candidate.items.is_empty() && self.is_fm()));
        self.commit_with_fm_release(candidate, release_fm)
    }

    pub(super) fn commit_with_fm_release(
        &mut self,
        candidate: QueueSnapshot,
        release_fm: bool,
    ) -> Result<()> {
        self.commit_with_quality(candidate, release_fm, None)
    }
    pub(super) fn commit_with_quality(
        &mut self,
        candidate: QueueSnapshot,
        release_fm: bool,
        quality: Option<(&str, &QualityPreferences)>,
    ) -> Result<()> {
        let intelligence = if candidate.intelligence == self.state.intelligence {
            self.intelligence.clone()
        } else if candidate.intelligence.is_none() {
            None
        } else {
            return Err(Error::State(
                "intelligence transition requires its coordinator",
            ));
        };
        self.commit_owned(candidate, release_fm, quality, intelligence)
    }
    pub(super) fn commit_owned(
        &mut self,
        candidate: QueueSnapshot,
        release_fm: bool,
        quality: Option<(&str, &QualityPreferences)>,
        intelligence: Option<Arc<crate::playback::intelligence::IntelligenceSession>>,
    ) -> Result<()> {
        self.commit_owned_with_side_effect(
            candidate,
            release_fm,
            quality,
            intelligence,
            SideEffect::None,
        )
    }

    pub(super) fn commit_more(
        &mut self,
        candidate: QueueSnapshot,
        preference: Option<bool>,
    ) -> Result<()> {
        self.commit_owned_with_side_effect(
            candidate,
            false,
            None,
            None,
            SideEffect::More(preference),
        )
    }

    fn commit_owned_with_side_effect(
        &mut self,
        mut candidate: QueueSnapshot,
        release_fm: bool,
        quality: Option<(&str, &QualityPreferences)>,
        intelligence: Option<Arc<crate::playback::intelligence::IntelligenceSession>>,
        side_effect: SideEffect<'_>,
    ) -> Result<()> {
        super::super::queue_more::normalize(&mut candidate);
        candidate.validate()?;
        let unchanged_session = match (&intelligence, &self.intelligence) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => self.intelligence_raw.is_none(),
            _ => false,
        };
        if let Some(session) = &intelligence {
            if candidate.intelligence.as_ref() != Some(&session.context) {
                return Err(Error::State("intelligence queue/session mismatch"));
            }
            if !unchanged_session {
                session.validate(&candidate)?;
            }
        } else if candidate.intelligence.is_some() {
            return Err(Error::State("missing intelligence state"));
        }
        if (quality.is_some() || intelligence.is_some() || !matches!(side_effect, SideEffect::None))
            && self.store.is_none()
        {
            return Err(Error::State("transition requires persistent queue"));
        }
        let revision = self.next_revision()?;
        let snapshot = serde_json::to_value(&candidate)?;
        let intelligence_value = if unchanged_session {
            None
        } else {
            intelligence
                .as_deref()
                .map(serde_json::to_value)
                .transpose()?
        };
        let intelligence_raw = if unchanged_session {
            self.intelligence_raw.clone()
        } else {
            intelligence_value
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?
        };
        if let Some(store) = &self.store {
            let result = store.transaction(|tx| {
                if tx.get(&self.owner, "playback", "queue")? != self.persisted {
                    return Err(Error::QueueChanged);
                }
                if tx.get_raw(
                    &self.owner,
                    crate::playback::intelligence::KIND,
                    crate::playback::intelligence::KEY,
                )? != self.intelligence_raw
                {
                    return Err(Error::QueueChanged);
                }
                if let SideEffect::Fm(fm) = &side_effect {
                    if tx.get(&self.owner, "playback", FM_BACKUP_QUEUE)? != fm.expected.backup_queue
                        || tx.get(&self.owner, "playback", FM_BACKUP_POSITION)?
                            != fm.expected.backup_position
                    {
                        return Err(Error::QueueChanged);
                    }
                    if let Some(position) = &fm.position {
                        match position {
                            Some(value) => tx.put(&self.owner, "playback", "position", value)?,
                            None => tx.delete(&self.owner, "playback", "position")?,
                        }
                    }
                }
                if let Some((expected, preference)) = quality {
                    QualityPreferences::replace_in(tx, &self.owner, expected, preference)?;
                }
                if let SideEffect::More(preference) = side_effect {
                    use crate::playback::queue_more::{KIND, PREFERENCE, ROOM_BINDING};
                    if tx.get(&self.owner, ROOM_BINDING, "current")?.is_some() {
                        return Err(Error::State("room owns the playback queue"));
                    }
                    if let Some(preference) = preference {
                        tx.put(&self.owner, KIND, PREFERENCE, &json!(preference))?;
                    }
                }
                if let SideEffect::Recall(now) = side_effect {
                    if tx
                        .get(
                            &self.owner,
                            super::super::queue_more::ROOM_BINDING,
                            "current",
                        )?
                        .is_some()
                    {
                        return Err(Error::QueueChanged);
                    }
                    recall_due(tx.get(&self.owner, RECALL_STATE, RECALL_LAST_TIME)?, now)?;
                    tx.put(&self.owner, RECALL_STATE, RECALL_LAST_TIME, &json!(now))?;
                }
                tx.put(&self.owner, "playback", "queue", &snapshot)?;
                if !unchanged_session {
                    match &intelligence_value {
                        Some(value) => tx.put(
                            &self.owner,
                            crate::playback::intelligence::KIND,
                            crate::playback::intelligence::KEY,
                            value,
                        )?,
                        None => tx.delete(
                            &self.owner,
                            crate::playback::intelligence::KIND,
                            crate::playback::intelligence::KEY,
                        )?,
                    }
                }
                if release_fm {
                    tx.delete(&self.owner, "playback", FM_BACKUP_QUEUE)?;
                    tx.delete(&self.owner, "playback", FM_BACKUP_POSITION)?;
                    tx.delete(&self.owner, "fm-player-v1", "source")?;
                    tx.delete(&self.owner, "fm-player-v1", "failure")?;
                }
                tx.change_stamp()
            });
            if let Err(error) = result {
                // No candidate state is published after a failed write. On a
                // competing edit, expose the newest valid committed queue.
                if let Ok(OwnedRead {
                    queue: raw,
                    session: intelligence_raw,
                    stamp: store_stamp,
                }) = Self::read_owned(Some(store), &self.owner)
                    && let Ok(state) = Self::load_current(Some(store), &self.owner, raw.as_ref())
                    && let Ok(intelligence) =
                        Self::load_intelligence(&state, intelligence_raw.as_deref())
                {
                    self.state = state;
                    self.persisted = raw;
                    self.intelligence_raw = intelligence_raw;
                    self.intelligence = intelligence;
                    self.store_stamp = store_stamp;
                }
                self.revision = revision;
                return Err(error);
            }
            self.store_stamp = Some(result?);
            self.persisted = Some(snapshot);
        }
        self.state = candidate;
        self.intelligence_raw = intelligence_raw;
        self.intelligence = intelligence;
        self.revision = revision;
        Ok(())
    }
}

fn recall_due(previous: Option<Value>, now: i64) -> Result<()> {
    if let Some(previous) = previous {
        let previous = previous
            .as_i64()
            .filter(|value| *value >= 0)
            .ok_or(Error::State("invalid recall timestamp"))?;
        if previous != 0 && now.saturating_sub(previous) < 86_400_000 {
            return Err(Error::State("homepage recall is in its cooldown"));
        }
    }
    Ok(())
}
