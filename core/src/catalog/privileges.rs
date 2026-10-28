use super::CatalogApi;
use crate::{Error, Result, protocol::api::identifier};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

/// M/21 B: last value wins, but the original/reply order and duplicates survive.
pub(crate) fn merge_song_privileges(songs: &[Value], privileges: &[Value]) -> Result<Vec<Value>> {
    let by_id = privileges
        .iter()
        .map(|p| Ok((identifier(&p["id"])?, p)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let ids = songs
        .iter()
        .map(|s| identifier(&s["id"]))
        .collect::<Result<Vec<_>>>()?;
    let original_ids = ids.iter().collect::<HashSet<_>>();
    let mut result = songs.to_vec();
    for (song, id) in result.iter_mut().zip(&ids) {
        if let Some(privilege) = by_id.get(id) {
            song["privilege"] = (*privilege).clone();
        } else {
            song.as_object_mut()
                .ok_or(Error::Response("invalid song"))?
                .remove("privilege");
        }
    }
    if songs.len() < privileges.len() {
        for privilege in privileges {
            let id = identifier(&privilege["id"])?;
            if !original_ids.contains(&id) {
                result.push(json!({"id":id,"privilege":by_id[&id],"artists":[]}));
            }
        }
    }
    Ok(result)
}

impl CatalogApi {
    /// Official M/15 Dg. M/612 omits the scene; MV explicitly supplies -1.
    pub async fn privileges(&self, ids: &[String], trial_mode: Option<i32>) -> Result<Vec<Value>> {
        if ids.len() > 50000 {
            return Err(Error::InvalidInput("too many song privileges"));
        }
        if trial_mode
            .is_some_and(|mode| ![-1, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 121].contains(&mode))
        {
            return Err(Error::InvalidInput("unknown privilege trial scene"));
        }
        let ids = ids
            .iter()
            .map(|id| identifier(&json!(id)))
            .collect::<Result<Vec<_>>>()?;
        let mut result = Vec::new();
        let batches = if ids.is_empty() {
            vec![ids.as_slice()]
        } else {
            ids.chunks(500).collect()
        };
        for batch in batches {
            let mut payload = json!({"ids":serde_json::to_string(batch)?});
            if let Some(mode) = trial_mode {
                payload["trialMode"] = json!(mode);
            }
            let response = self
                .api
                .call("/api/song/enhance/privilege", payload, false, &[200])
                .await?;
            for row in response["data"]
                .as_array()
                .ok_or(Error::Response("song privileges missing"))?
            {
                identifier(&row["id"])?;
                // M/21 B uses last-wins lookup and may append missing song
                // metadata in reply order. Preserve duplicates and extra IDs.
                result.push(row.clone());
            }
        }
        Ok(result)
    }
}
