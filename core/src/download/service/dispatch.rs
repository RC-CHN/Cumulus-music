//! Private admission order, independent of the task's display/creation time.
use crate::{Error, Result, storage::StateTransaction};
use serde::{Deserialize, Serialize};
use serde_json::json;

const ORDER: &str = "download_dispatch_v1";
const LAST: &str = "last";

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub(super) struct Dispatch {
    sequence: u64,
    index: usize,
}

impl Dispatch {
    pub(super) fn reserve(tx: &mut StateTransaction<'_>, owner: &str) -> Result<u64> {
        let previous = match tx.get(owner, ORDER, LAST)? {
            Some(value) => value
                .as_u64()
                .ok_or(Error::State("invalid download dispatch counter"))?,
            None => 0,
        };
        let sequence = previous
            .checked_add(1)
            .ok_or(Error::State("download dispatch counter exhausted"))?;
        tx.put(owner, ORDER, LAST, &json!(sequence))?;
        Ok(sequence)
    }

    pub(super) fn new(sequence: u64, index: usize) -> Self {
        Self { sequence, index }
    }
}
