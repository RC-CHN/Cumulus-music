//! Persistent queue semantics from playing/dbPlayingList. Entry identity is
//! distinct from song identity; the on-disk representation is ours.
mod fm_identity;
mod intelligence;
mod persistence;
mod reset;
use super::quality_preferences::QualityPreferences;
use crate::{Error, Result, storage::StateStore};
use intelligence::OwnedRead;
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
pub(crate) use reset::QueueResetPlan;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use uuid::Uuid;

pub(crate) const FM_BACKUP_QUEUE: &str = "fm-backup-queue";
pub(crate) const FM_BACKUP_POSITION: &str = "fm-backup-position";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMode {
    #[default]
    OrderLoop,
    SingleLoop,
    Random,
    Sequential,
}

#[derive(Debug, Clone, Copy)]
pub enum Step {
    Next,
    Previous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub entry_id: String,
    pub resource: Value,
    #[serde(default)]
    pub source: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueSnapshot {
    pub items: Vec<QueueEntry>,
    pub order: Vec<String>,
    pub current: Option<String>,
    pub mode: PlaybackMode,
    /// Coordinator identity survives an empty recommendation batch. This is
    /// native persistence, never a public IPC resource or authorization URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fm_source: Option<crate::fm::FmSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intelligence: Option<super::intelligence::IntelligenceContext>,
}

impl QueueSnapshot {
    pub(crate) fn load(value: Option<&Value>) -> Result<Self> {
        let mut state: Self = match value {
            Some(value) => serde_json::from_value(value.clone())?,
            None => Self::default(),
        };
        if value.is_some_and(|v| v.get("order").is_none()) {
            state.order = state.items.iter().map(|i| i.entry_id.clone()).collect();
        }
        state.validate()?;
        Ok(state)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        self.validate_fm_identity()?;
        self.validate_intelligence_identity()?;
        let ids: HashSet<_> = self.items.iter().map(|i| i.entry_id.as_str()).collect();
        let order: HashSet<_> = self.order.iter().map(String::as_str).collect();
        if ids.contains("")
            || ids.len() != self.items.len()
            || self.order.len() != ids.len()
            || ids != order
        {
            return Err(Error::State("invalid persisted queue order"));
        }
        if self.current.as_deref().is_some_and(|id| !ids.contains(id)) {
            return Err(Error::State("invalid persisted current entry"));
        }
        Ok(())
    }

    pub(crate) fn selected(&self) -> Option<QueueEntry> {
        self.items
            .iter()
            .find(|i| Some(&i.entry_id) == self.current.as_ref())
            .cloned()
    }
}

pub struct PlaybackQueue {
    store: Option<Arc<StateStore>>,
    owner: String,
    state: QueueSnapshot,
    persisted: Option<Value>,
    intelligence_raw: Option<String>,
    intelligence: Option<Arc<super::intelligence::IntelligenceSession>>,
    store_stamp: Option<(i64, i64)>,
    revision: u64,
    rng: StdRng,
}

impl PlaybackQueue {
    pub fn new(store: Option<Arc<StateStore>>, owner: impl Into<String>) -> Result<Self> {
        Self::with_rng(store, owner, StdRng::from_os_rng())
    }

    pub fn with_rng(
        store: Option<Arc<StateStore>>,
        owner: impl Into<String>,
        rng: StdRng,
    ) -> Result<Self> {
        let owner = owner.into();
        let OwnedRead {
            queue: persisted,
            session: intelligence_raw,
            stamp: store_stamp,
        } = Self::read_owned(store.as_deref(), &owner)?;
        let state = Self::load_current(store.as_deref(), &owner, persisted.as_ref())?;
        let intelligence = Self::load_intelligence(&state, intelligence_raw.as_deref())?;
        Ok(Self {
            store,
            owner,
            state,
            persisted,
            intelligence_raw,
            intelligence,
            store_stamp,
            revision: 0,
            rng,
        })
    }

    pub fn snapshot(&self) -> QueueSnapshot {
        self.state.clone()
    }
    pub(crate) fn install(&mut self, snapshot: QueueSnapshot) -> Result<()> {
        snapshot.validate()?;
        for item in &snapshot.items {
            super::PlaybackConstraint::from_resource(&item.resource)?;
        }
        if snapshot == self.state {
            return Ok(());
        }
        self.commit(snapshot)
    }
    pub fn len(&self) -> usize {
        self.state.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.state.items.is_empty()
    }
    pub fn is_fm(&self) -> bool {
        self.state.fm_source.is_some()
            || (!self.state.items.is_empty()
                && self.state.items.iter().all(|e| e.source["kind"] == "fm"))
    }
    pub fn mode(&self) -> PlaybackMode {
        self.state.mode
    }
    pub fn selected(&self) -> Option<QueueEntry> {
        self.state.selected()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn owner(&self) -> &str {
        &self.owner
    }

    fn next_revision(&self) -> Result<u64> {
        self.revision
            .checked_add(1)
            .ok_or(Error::State("queue revision exhausted"))
    }

    pub fn reload(&mut self) -> Result<bool> {
        let Some(store) = &self.store else {
            return Ok(false);
        };
        if Some(store.change_stamp()?) == self.store_stamp {
            return Ok(false);
        }
        let OwnedRead {
            queue: raw,
            session: intelligence_raw,
            stamp: store_stamp,
        } = Self::read_owned(Some(store), &self.owner)?;
        if raw == self.persisted && intelligence_raw == self.intelligence_raw {
            self.store_stamp = store_stamp;
            return Ok(false);
        }
        let state = Self::load_current(Some(store), &self.owner, raw.as_ref())?;
        let intelligence = Self::load_intelligence(&state, intelligence_raw.as_deref())?;
        let revision = self.next_revision()?;
        self.state = state;
        self.persisted = raw;
        self.intelligence_raw = intelligence_raw;
        self.intelligence = intelligence;
        self.store_stamp = store_stamp;
        self.revision = revision;
        Ok(true)
    }

    fn entries(resources: impl IntoIterator<Item = Value>, source: &Value) -> Vec<QueueEntry> {
        resources
            .into_iter()
            .map(|resource| QueueEntry {
                entry_id: Uuid::new_v4().simple().to_string(),
                resource,
                source: source.clone(),
            })
            .collect()
    }

    pub fn replace(&mut self, resources: Vec<Value>, index: usize, source: Value) -> Result<()> {
        if !resources.is_empty() && index >= resources.len() {
            return Err(Error::InvalidInput("invalid initial queue index"));
        }
        let items = Self::entries(resources, &source);
        let mut order: Vec<_> = items.iter().map(|i| i.entry_id.clone()).collect();
        let current = order.get(index).cloned();
        let mode = self
            .intelligence
            .as_ref()
            .filter(|s| s.context.active())
            .map_or(self.state.mode, |s| s.last_mode);
        if mode == PlaybackMode::Random {
            order.shuffle(&mut self.rng)
        }
        self.commit_with_fm_release(
            QueueSnapshot {
                items,
                order,
                current,
                mode,
                fm_source: None,
                intelligence: None,
            },
            source["kind"] != "fm",
        )
    }

    pub fn append(&mut self, resources: Vec<Value>, source: Value) -> Result<Vec<String>> {
        let added = Self::entries(resources, &source);
        let ids: Vec<_> = added.iter().map(|i| i.entry_id.clone()).collect();
        let mut order = ids.clone();
        if self.state.mode == PlaybackMode::Random {
            order.shuffle(&mut self.rng)
        }
        let mut state = self.snapshot();
        state.items.extend(added);
        state.order.extend(order);
        if state.current.is_none() {
            state.current = state.order.first().cloned()
        }
        self.commit(state)?;
        Ok(ids)
    }

    pub fn select(&mut self, entry_id: &str) -> Result<QueueEntry> {
        let selected = self
            .state
            .items
            .iter()
            .find(|i| i.entry_id == entry_id)
            .cloned()
            .ok_or(Error::InvalidInput("entry is not in queue"))?;
        if !super::PlaybackConstraint::playable(&selected.resource) {
            return Err(Error::State("resource is not jointly playable"));
        }
        let mut state = self.snapshot();
        state.current = Some(entry_id.into());
        if let Some(previous) = self
            .selected()
            .filter(|e| e.entry_id != entry_id && e.source[super::intelligence::TRANSIENT] == true)
        {
            state.items.retain(|e| e.entry_id != previous.entry_id);
            state.order.retain(|id| *id != previous.entry_id);
        }
        self.commit(state)?;
        Ok(selected)
    }

    pub fn set_mode(&mut self, mode: PlaybackMode) -> Result<()> {
        let mut state = self.snapshot();
        if state
            .intelligence
            .as_ref()
            .is_some_and(super::intelligence::IntelligenceContext::active)
        {
            return Err(Error::State("intelligence mode requires its coordinator"));
        }
        state.intelligence = None;
        if mode != state.mode {
            state.mode = mode;
            state.order = state.items.iter().map(|i| i.entry_id.clone()).collect();
            if mode == PlaybackMode::Random {
                state.order.shuffle(&mut self.rng)
            }
        }
        self.commit(state)
    }

    pub fn update_resource(&mut self, entry_id: &str, resource: Value) -> Result<()> {
        let mut state = self.snapshot();
        let item = state
            .items
            .iter_mut()
            .find(|i| i.entry_id == entry_id)
            .ok_or(Error::InvalidInput("unknown queue entry"))?;
        item.resource = resource;
        self.commit(state)
    }

    /// Commit both CAS-protected records together before interrupting audio.
    pub(crate) fn update_resource_with_quality(
        &mut self,
        entry_id: &str,
        resource: Value,
        expected: &str,
        preference: &QualityPreferences,
    ) -> Result<()> {
        let mut state = self.snapshot();
        let item = state
            .items
            .iter_mut()
            .find(|i| i.entry_id == entry_id)
            .ok_or(Error::InvalidInput("unknown queue entry"))?;
        item.resource = resource;
        let release_fm = state.items.iter().any(|e| e.source["kind"] != "fm");
        self.commit_with_quality(state, release_fm, Some((expected, preference)))
    }

    pub fn reorder(&mut self, entry_ids: &[String]) -> Result<()> {
        let mut state = self.snapshot();
        let mut by_id: HashMap<_, _> = state
            .items
            .into_iter()
            .map(|i| (i.entry_id.clone(), i))
            .collect();
        if entry_ids.len() != by_id.len() {
            return Err(Error::InvalidInput(
                "reordering must retain every entry exactly once",
            ));
        }
        state.items = entry_ids
            .iter()
            .map(|id| {
                by_id
                    .remove(id)
                    .ok_or(Error::InvalidInput("duplicate or unknown queue entry"))
            })
            .collect::<Result<_>>()?;
        if state.mode != PlaybackMode::Random {
            state.order = entry_ids.into()
        }
        self.commit(state)
    }

    pub fn remove(&mut self, entry_ids: &[String]) -> Result<()> {
        let removed: HashSet<_> = entry_ids.iter().collect();
        if removed.iter().any(|id| !self.state.order.contains(id)) {
            return Err(Error::InvalidInput("unknown queue entry"));
        }
        let mut state = self.snapshot();
        let position = state
            .current
            .as_ref()
            .and_then(|id| state.order.iter().position(|i| i == id))
            .unwrap_or(0);
        let remaining: Vec<_> = state
            .order
            .iter()
            .filter(|id| !removed.contains(id))
            .cloned()
            .collect();
        if state
            .current
            .as_ref()
            .is_some_and(|id| removed.contains(id))
        {
            state.current = state
                .order
                .iter()
                .skip(position + 1)
                .find(|id| !removed.contains(id))
                .cloned()
                .or_else(|| remaining.first().cloned());
        }
        state.items.retain(|i| !removed.contains(&i.entry_id));
        state.order = remaining;
        if state.items.is_empty() {
            state.intelligence = None;
            if let Some(session) = self.intelligence.as_ref().filter(|s| s.context.active()) {
                state.mode = session.last_mode;
            }
        }
        self.commit(state)
    }

    pub fn peek(&self, step: Step, ended: bool) -> Option<QueueEntry> {
        let current = self.state.current.as_ref()?;
        if ended
            && self.state.mode == PlaybackMode::SingleLoop
            && self.selected().is_some_and(|e| {
                super::PlaybackConstraint::from_resource(&e.resource).ok() == Some(None)
            })
        {
            return self.selected();
        }
        let index = self.state.order.iter().position(|id| id == current)?;
        let size = self.state.order.len();
        for distance in 1..=size {
            let next = match step {
                Step::Next if index + distance < size => index + distance,
                Step::Previous if index >= distance => index - distance,
                _ if self.state.mode == PlaybackMode::Sequential => return None,
                Step::Next => (index + distance) % size,
                Step::Previous => (index + size - distance) % size,
            };
            let id = &self.state.order[next];
            if let Some(item) = self.state.items.iter().find(|i| &i.entry_id == id)
                && super::PlaybackConstraint::playable(&item.resource)
                // A shared preview must not repeat forever if all others are blocked.
                && !(ended && next == index && item.resource.get("playback_constraint").is_some())
            {
                return Some(item.clone());
            }
        }
        None
    }

    pub fn advance(&mut self, step: Step, ended: bool) -> Result<Option<QueueEntry>> {
        match self.peek(step, ended) {
            Some(next) => self.select(&next.entry_id).map(Some),
            None => Ok(None),
        }
    }
}
