//! Shared signed wire projection. Queue and FM caches own separate lifecycles.
use crate::{
    Error, Result,
    protocol::api::{Api, PlaybackChorusPermission, PlaybackChorusRead, identifier},
};
use futures_util::future::try_join_all;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(crate) type Points = BTreeMap<String, f64>;
type BatchPoints = BTreeMap<String, Option<f64>>;

pub(crate) async fn read(
    api: &Arc<dyn Api>,
    credentials: &str,
    batches: &[Vec<Value>],
    permission: &PlaybackChorusPermission<'_>,
) -> Result<Vec<BatchPoints>> {
    // M86 starts all groups together, then folds in request order regardless
    // of response arrival. An error yields no partial batch.
    try_join_all(batches.iter().map(|ids| async {
        permission()?;
        let request = PlaybackChorusRead::new(ids.clone())?;
        let response = api
            .request_playback_chorus(request, credentials, permission)
            .await?;
        permission()?;
        batch_points(ids, &response)
    }))
    .await
}

pub(crate) fn merge(points: &mut Points, batches: Vec<BatchPoints>) {
    for (id, seconds) in batches.into_iter().flatten() {
        if let Some(seconds) = seconds {
            points.insert(id, seconds);
        } else {
            points.remove(&id);
        }
    }
}

fn batch_points(request: &[Value], response: &Value) -> Result<BTreeMap<String, Option<f64>>> {
    let mut points = BTreeMap::new();
    let requested: BTreeSet<_> = request.iter().map(identifier).collect::<Result<_>>()?;
    let rows = response["data"]
        .as_array()
        .ok_or(Error::Response("invalid chorus batch response"))?;
    for row in rows {
        let Ok(id) = identifier(&row["id"]) else {
            continue;
        };
        if requested.contains(&id) {
            let seconds = row["startTime"]
                .as_f64()
                .filter(|time| time.is_finite() && *time > 0.0)
                .map(|time| time / 1000.0);
            points.insert(id, seconds);
        }
    }
    Ok(points)
}
