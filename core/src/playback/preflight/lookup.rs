//! Interpret same-recording detail as a presence check, not authorization.
use crate::{
    Result,
    catalog::same_recording_details,
    protocol::api::{Api, identifier},
};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};

pub(super) async fn same_recordings(api: Arc<dyn Api>, ids: &[String]) -> Result<HashSet<String>> {
    let mut found = HashSet::new();
    let present = |row: &Value| identifier(&row["id"]);
    for batch in same_recording_details(api, ids).await? {
        for row in &batch.songs {
            found.insert(present(row)?);
        }
        // M/21.B appends privilege-only placeholders when its privileges
        // vector is longer than the returned song vector. M/59 sees IDs
        // from those placeholders, but they still are not authorizations.
        if batch.songs.len() < batch.privileges.len() {
            for row in &batch.privileges {
                found.insert(present(row)?);
            }
        }
    }
    let requested: HashSet<_> = ids.iter().map(String::as_str).collect();
    found.retain(|id| requested.contains(id.as_str()));
    Ok(found)
}
