//! M/120.a → M/15.ki: read declared same-recording detail, never media.
use crate::{Error, Result, protocol::api::Api};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::task::JoinSet;

const BATCH_SIZE: usize = 500;
const CONCURRENT_BATCHES: usize = 4;

pub(crate) struct SameRecordingDetailBatch {
    pub(crate) songs: Vec<Value>,
    pub(crate) privileges: Vec<Value>,
}

pub(crate) async fn same_recording_details(
    api: Arc<dyn Api>,
    ids: &[String],
) -> Result<Vec<SameRecordingDetailBatch>> {
    let mut chunks = ids.chunks(BATCH_SIZE).enumerate();
    let mut tasks = JoinSet::new();
    let mut batches = Vec::with_capacity(ids.len().div_ceil(BATCH_SIZE));
    loop {
        while tasks.len() < CONCURRENT_BATCHES {
            let Some((index, chunk)) = chunks.next() else {
                break;
            };
            let api = api.clone();
            // The wire keeps every declared occurrence, including repeats.
            let rows: Vec<_> = chunk.iter().map(|id| json!({"id":id,"v":0})).collect();
            let packed = serde_json::to_string(&rows)?;
            tasks.spawn(async move {
                let mut response = api
                    .call(
                        "/api/v3/song/detail",
                        json!({"c":packed,"trialMode":-1}),
                        false,
                        &[200],
                    )
                    .await?;
                let mut array = |key| match response.get_mut(key).map(Value::take) {
                    Some(Value::Array(rows)) => Some(rows),
                    _ => None,
                };
                let (Some(songs), Some(privileges)) = (array("songs"), array("privileges")) else {
                    return Err(Error::Response(
                        "missing same-recording detail or privileges",
                    ));
                };
                if songs.len().max(privileges.len()) > BATCH_SIZE {
                    return Err(Error::Response("too many same-recording details"));
                }
                // The caller owns identity matching and the meaning of any
                // privilege-only rows. Neither list is media authorization.
                Ok((index, SameRecordingDetailBatch { songs, privileges }))
            });
        }
        let Some(joined) = tasks.join_next().await else {
            break;
        };
        batches.push(joined.map_err(|_| Error::State("same-recording detail worker stopped"))??);
    }
    batches.sort_unstable_by_key(|(index, _)| *index);
    Ok(batches.into_iter().map(|(_, batch)| batch).collect())
}
