//! Incomplete resource lists are errors, never successful partial results.
use super::api::Api;
use crate::{Error, Result};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub struct PageBounds {
    pub limit: usize,
    pub max_pages: usize,
}
impl Default for PageBounds {
    fn default() -> Self {
        Self {
            limit: 200,
            max_pages: 1000,
        }
    }
}

pub async fn pages(
    api: &dyn Api,
    path: &str,
    params: Value,
    key: &str,
    bounds: PageBounds,
) -> Result<Vec<Vec<Value>>> {
    if !(1..=1000).contains(&bounds.limit) || bounds.max_pages == 0 || !params.is_object() {
        return Err(Error::InvalidInput(
            "invalid pagination bounds or parameters",
        ));
    }
    let mut offset = 0;
    let mut seen = HashSet::new();
    let mut output = vec![];
    for _ in 0..bounds.max_pages {
        let mut payload = params.clone();
        payload["limit"] = json!(bounds.limit);
        payload["offset"] = json!(offset);
        let response = api.call(path, payload, false, &[200]).await?;
        let items = response
            .get(key)
            .and_then(Value::as_array)
            .ok_or(Error::Response("missing resource list"))?;
        let mut canonical = json!(items);
        canonical.sort_all_objects();
        if !items.is_empty() && !seen.insert(serde_json::to_string(&canonical)?) {
            return Err(Error::Response("server repeated a page"));
        }
        let more = response
            .get("more")
            .or_else(|| response.get("hasMore"))
            .unwrap_or(&Value::Null);
        if !more.is_null() && !more.is_boolean() {
            return Err(Error::Response("invalid pagination flag"));
        }
        if more.as_bool() == Some(true) && items.is_empty() {
            return Err(Error::Response("nonfinal page is empty"));
        }
        output.push(items.clone());
        if more.as_bool() == Some(false) || (more.is_null() && items.len() < bounds.limit) {
            return Ok(output);
        }
        offset += items.len();
    }
    Err(Error::Response(
        "pagination limit reached; result is incomplete",
    ))
}

pub async fn all(api: &dyn Api, path: &str, params: Value, key: &str) -> Result<Vec<Value>> {
    Ok(pages(api, path, params, key, PageBounds::default())
        .await?
        .into_iter()
        .flatten()
        .collect())
}
