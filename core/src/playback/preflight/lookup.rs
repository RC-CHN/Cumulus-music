//! M/120.a → M/15.ki: the read of declared same-recording IDs is separate
//! from source authorization. Keep duplicates in the exact packed wire.
use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

pub(super) async fn same_recordings(api: Arc<dyn Api>, ids: &[String]) -> Result<HashSet<String>> {
    let mut tasks = tokio::task::JoinSet::new();
    for chunk in ids.chunks(500) {
        let api = api.clone();
        let rows: Vec<_> = chunk.iter().map(|id| json!({"id":id,"v":0})).collect();
        let packed = serde_json::to_string(&rows)?;
        tasks.spawn(async move {
            let response = api
                .call(
                    "/api/v3/song/detail",
                    json!({"c":packed,"trialMode":-1}),
                    false,
                    &[200],
                )
                .await?;
            let (Some(songs), Some(privileges)) = (
                response["songs"].as_array(),
                response["privileges"].as_array(),
            ) else {
                return Err(Error::Response(
                    "missing same-recording detail or privileges",
                ));
            };
            if songs.len().max(privileges.len()) > 500 {
                return Err(Error::Response("too many same-recording details"));
            }
            let present = |row: &Value| identifier(&row["id"]);
            let mut ids = songs.iter().map(present).collect::<Result<HashSet<_>>>()?;
            // M/21.B appends privilege-only placeholders when its privileges
            // vector is longer than the returned song vector. M/59 sees IDs
            // from those placeholders, but they still are not authorizations.
            if songs.len() < privileges.len() {
                for row in privileges {
                    ids.insert(present(row)?);
                }
            }
            Ok(ids)
        });
    }
    let mut found = HashSet::new();
    while let Some(joined) = tasks.join_next().await {
        found.extend(joined.map_err(|_| Error::State("same-recording detail worker stopped"))??);
    }
    let requested: HashSet<_> = ids.iter().map(String::as_str).collect();
    found.retain(|id| requested.contains(id.as_str()));
    Ok(found)
}
