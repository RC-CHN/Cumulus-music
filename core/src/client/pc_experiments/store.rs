//! Presentation-only bootstrap. It contains no live authority or credentials.
use super::presentation::Flags;
use crate::{Result, storage::StateStore};
use serde::{Deserialize, Serialize};

const KIND: &str = "pc-presentation-v1";
const KEY: &str = "cache";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    flags: Flags,
}

pub(super) fn load(store: &StateStore, owner: &str) -> Result<Flags> {
    if let Some(raw) = store.get(owner, KIND, KEY)? {
        return Ok(serde_json::from_value::<Record>(raw)
            .ok()
            .filter(|record| record.schema == 1)
            .map(|record| record.flags)
            .unwrap_or_default());
    }
    // Previous releases persisted the three endpoint/layout flags but never a
    // footer grant. Preserve those compatible UI choices during an upgrade.
    let old = store.get(owner, "search-experiment-v1", "assignment-v2")?;
    Ok(Flags {
        search_v3: old.as_ref().is_some_and(|value| value["v3"] == true),
        search_expanded: old.as_ref().is_some_and(|value| value["expanded"] == true),
        search_dense: old.as_ref().is_some_and(|value| value["dense"] == true),
        search_footer: false,
    })
}

pub(super) fn save(store: &StateStore, owner: &str, flags: Flags) -> Result<()> {
    store.put(
        owner,
        KIND,
        KEY,
        &serde_json::to_value(Record { schema: 1, flags })?,
    )
}
