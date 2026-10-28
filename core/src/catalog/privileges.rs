use super::CatalogApi;
use crate::{Error, Result, protocol::api::identifier};
use serde_json::{Value, json};

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
