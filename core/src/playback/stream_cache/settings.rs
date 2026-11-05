//! Device-local capacity preference. Neither byte-cache entries nor playback
//! credentials enter SQLite. Each host's controls have a separate CAS token.
use super::store::{Cache, MAX_BUDGET};
use crate::{Error, Result, storage::StateStore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const OWNER: &str = "local-device";
const KIND: &str = "streaming-media-cache-preferences-v1";
const CURRENT: &str = "current";
const REVISION: &str = "revision";
const STEP: u64 = 512 * 1024 * 1024;
const DEFAULT: u64 = 2 * STEP;
const DAMAGED: Error = Error::State("streaming cache preferences are damaged");

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingMediaCacheStatus {
    pub revision: String,
    pub directory: String,
    pub capacity_bytes: u64,
    pub occupied_bytes: u64,
    pub reserved_bytes: u64,
    pub reclaimable_bytes: u64,
    pub over_capacity: bool,
    pub min_capacity_bytes: u64,
    pub max_capacity_bytes: u64,
    pub capacity_step_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Policy {
    revision: u64,
    capacity_bytes: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            revision: 0,
            capacity_bytes: DEFAULT,
        }
    }
}
impl Policy {
    fn decode(current: Option<Value>, recorded: Option<Value>) -> Result<Self> {
        match (current, recorded) {
            (None, None) => Ok(Self::default()),
            (Some(value), Some(recorded)) => {
                let policy: Self = serde_json::from_value(value).map_err(|_| DAMAGED)?;
                if policy.revision == 0
                    || recorded.as_u64() != Some(policy.revision)
                    || !capacity_is_supported(policy.capacity_bytes)
                {
                    return Err(DAMAGED);
                }
                Ok(policy)
            }
            _ => Err(DAMAGED),
        }
    }

    fn read(state: &StateStore) -> Result<Self> {
        let mut rows = state.values(OWNER, KIND)?;
        Self::decode(rows.remove(CURRENT), rows.remove(REVISION))
    }
}

fn capacity_is_supported(bytes: u64) -> bool {
    (STEP..=MAX_BUDGET).contains(&bytes) && bytes.is_multiple_of(STEP)
}

struct Observed {
    policy: Policy,
    // Not stored: a reopened host must not accept an old window's token even
    // when its durable preference happens to be unchanged.
    revision: String,
}
impl Observed {
    fn new(policy: Policy) -> Self {
        Self {
            policy,
            revision: uuid::Uuid::new_v4().simple().to_string(),
        }
    }

    fn require(&self, expected: &str) -> Result<()> {
        if self.revision != expected {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    fn view(&self, cache: &Cache) -> Result<StreamingMediaCacheStatus> {
        let usage = cache.usage()?;
        Ok(StreamingMediaCacheStatus {
            revision: self.revision.clone(),
            directory: cache.directory().to_string_lossy().into_owned(),
            capacity_bytes: usage.capacity,
            occupied_bytes: usage.occupied,
            reserved_bytes: usage.reserved,
            reclaimable_bytes: usage.reclaimable,
            over_capacity: usage.reserved > usage.capacity,
            min_capacity_bytes: STEP,
            max_capacity_bytes: MAX_BUDGET,
            capacity_step_bytes: STEP,
        })
    }
}

pub(super) struct Settings {
    store: Arc<StateStore>,
    current: Mutex<Observed>,
}
impl Settings {
    pub(super) fn open(store: Arc<StateStore>) -> Result<(Self, u64)> {
        let policy = Policy::read(&store)?;
        let settings = Self {
            store,
            current: Mutex::new(Observed::new(policy)),
        };
        Ok((settings, policy.capacity_bytes))
    }

    fn refresh(&self, current: &mut Observed, cache: &Cache) -> Result<()> {
        let saved = Policy::read(&self.store)?;
        if saved != current.policy {
            cache.change_capacity(saved.capacity_bytes)?;
            *current = Observed::new(saved);
        }
        Ok(())
    }

    pub(super) fn status(&self, cache: &Cache) -> Result<StreamingMediaCacheStatus> {
        let mut current = self.current.lock().map_err(|_| Error::LockPoisoned)?;
        self.refresh(&mut current, cache)?;
        current.view(cache)
    }

    pub(super) fn set(
        &self,
        cache: &Cache,
        expected: &str,
        capacity_bytes: u64,
    ) -> Result<StreamingMediaCacheStatus> {
        if !capacity_is_supported(capacity_bytes) {
            return Err(Error::InvalidInput(
                "streaming cache capacity must be 512 MiB to 10 GiB in 512 MiB steps",
            ));
        }
        let mut current = self.current.lock().map_err(|_| Error::LockPoisoned)?;
        self.refresh(&mut current, cache)?;
        current.require(expected)?;
        if current.policy.capacity_bytes == capacity_bytes {
            return current.view(cache);
        }
        let saved = self.store.transaction(|transaction| {
            let prior = Policy::decode(
                transaction.get(OWNER, KIND, CURRENT)?,
                transaction.get(OWNER, KIND, REVISION)?,
            )?;
            if prior != current.policy {
                return Err(Error::StaleResponse);
            }
            let policy = Policy {
                capacity_bytes,
                revision: prior.revision.checked_add(1).ok_or(Error::State(
                    "streaming cache preference revision exhausted",
                ))?,
            };
            transaction.put(OWNER, KIND, CURRENT, &serde_json::to_value(policy)?)?;
            transaction.put(OWNER, KIND, REVISION, &json!(policy.revision))?;
            Ok(policy)
        })?;
        cache.change_capacity(saved.capacity_bytes)?;
        *current = Observed::new(saved);
        current.view(cache)
    }

    pub(super) fn clear(
        &self,
        cache: &Cache,
        expected: Option<&str>,
    ) -> Result<StreamingMediaCacheStatus> {
        let mut current = self.current.lock().map_err(|_| Error::LockPoisoned)?;
        self.refresh(&mut current, cache)?;
        if let Some(expected) = expected {
            current.require(expected)?;
        }
        cache.clear()?;
        *current = Observed::new(current.policy);
        current.view(cache)
    }
}

#[cfg(test)]
mod tests;
