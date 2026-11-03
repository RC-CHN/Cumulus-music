//! Native search experiment assignment and last successful tab order.
use crate::catalog::search::results::{self as api, SearchCategory};
use crate::{
    Result,
    account::{AccountSession, SessionToken, login_policy::AB_PATH},
    protocol::api::Api,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

const EXPERIMENT: &str = "PH-PC-searchHot";
const NAMESPACE: &str = "search-experiment-v1";
const REFRESH_SECONDS: u64 = 30 * 60;

#[derive(Default)]
pub(crate) struct SearchHost {
    pub(crate) enabled: bool,
    gate: tokio::sync::Mutex<()>,
}

#[derive(Deserialize, Serialize, Default)]
struct Assignment {
    attempted_at: u64,
    v3: bool,
}

fn assignment(value: &Value) -> Option<bool> {
    let rows = value["data"].as_array().filter(|rows| rows.len() <= 4096)?;
    Some(
        rows.iter()
            .find(|row| row["expName"] == EXPERIMENT)
            .is_some_and(|row| row["expGroupName"] == "t1"),
    )
}

fn parse_tabs(value: &Value) -> Option<Vec<SearchCategory>> {
    let data = value["data"].as_object()?;
    let rows = data
        .get("algTabs")
        .filter(|v| !v.is_null())
        .or_else(|| data.get("defaultTabs"))?
        .as_array()?;
    let mut tabs = Vec::new();
    for row in rows.iter().take(64) {
        if let Some(kind) = row["title"].as_str().and_then(SearchCategory::from_title)
            && !tabs.contains(&kind)
        {
            tabs.push(kind);
        }
    }
    (!tabs.is_empty()).then_some(tabs)
}

impl SearchHost {
    pub(super) async fn enabled_for(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        api: &dyn Api,
    ) -> Result<bool> {
        if !self.enabled {
            return Ok(false);
        }
        let _lock = self.gate.lock().await;
        let store = session.persistent_store()?;
        let old: Assignment = session.with_token(token, |owner| {
            Ok(store
                .get(owner, NAMESPACE, "assignment")?
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default())
        })?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default();
        if now >= old.attempted_at && now - old.attempted_at < REFRESH_SECONDS {
            return Ok(old.v3);
        }
        let response = api
            .call(AB_PATH, json!({"expNames":EXPERIMENT}), false, &[200])
            .await;
        let updated = Assignment {
            attempted_at: now,
            v3: response
                .as_ref()
                .ok()
                .and_then(assignment)
                .unwrap_or(old.v3),
        };
        session.with_token(token, |owner| {
            store.put(
                owner,
                NAMESPACE,
                "assignment",
                &serde_json::to_value(&updated)?,
            )
        })?;
        Ok(updated.v3)
    }

    pub(super) async fn tabs_for(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        api: &dyn Api,
        keyword: &str,
    ) -> Result<Vec<SearchCategory>> {
        let response = api::tabs(api, keyword).await;
        let store = session.persistent_store()?;
        session.with_token(token, |owner| {
            if let Some(tabs) = response.as_ref().ok().and_then(parse_tabs) {
                store.put(owner, NAMESPACE, "tabs", &serde_json::to_value(&tabs)?)?;
                return Ok(tabs);
            }
            Ok(store
                .get(owner, NAMESPACE, "tabs")?
                .and_then(|value| serde_json::from_value::<Vec<SearchCategory>>(value).ok())
                .filter(|tabs| !tabs.is_empty() && tabs.len() <= SearchCategory::ALL.len())
                .unwrap_or_else(|| SearchCategory::ALL.to_vec()))
        })
    }
}
