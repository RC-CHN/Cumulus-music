//! Revision-bound selected rows from the official playlist batch-operation page.
use crate::{Error, Result, protocol::api::identifier};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub(super) fn selected_ids(
    id: &str,
    detail: &Value,
    ids: &[String],
    revision: &str,
) -> Result<Vec<Value>> {
    if ids.is_empty() || ids.len() > 10_000 {
        return Err(Error::InvalidInput("invalid playlist batch size"));
    }
    let tracks = detail["trackIds"]
        .as_array()
        .ok_or(Error::Response("missing playlist tracks"))?;
    let actual = hex::encode(Sha256::digest(serde_json::to_vec(&json!([id, tracks]))?));
    if actual != revision {
        return Err(Error::QueueChanged);
    }
    let mut wanted: HashSet<_> = ids
        .iter()
        .map(|id| identifier(&json!(id)))
        .collect::<Result<_>>()?;
    let mut selected = Vec::new();
    // The page selects in displayed playlist order, never click order.
    for row in tracks.iter().filter(|row| row["v"].as_i64() != Some(-1)) {
        let id = identifier(&row["id"])?;
        if wanted.remove(&id) {
            selected.push(json!(id));
        }
    }
    if !wanted.is_empty() {
        return Err(Error::QueueChanged);
    }
    Ok(selected)
}
