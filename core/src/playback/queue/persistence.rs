//! CAS-protected publication of queue, quality and coordinator ownership.
use super::*;

impl PlaybackQueue {
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
        if (quality.is_some() || intelligence.is_some()) && self.store.is_none() {
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
                if let Some((expected, preference)) = quality {
                    QualityPreferences::replace_in(tx, &self.owner, expected, preference)?;
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
