//! Native search experiment assignment and last successful tab order.
use crate::catalog::search::results::{self as api, SearchCategory};
use crate::{
    Result,
    account::{AccountSession, SessionToken, login_policy::AB_PATH},
    protocol::api::Api,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const EXPERIMENTS: [&str; 3] = [
    "PH-PC-searchHot",
    "PH-PC-searchMore2",
    "PH-PC-searchPageEfficiency",
];
const NAMESPACE: &str = "search-experiment-v1";
const ASSIGNMENT_KEY: &str = "assignment-v2";
const REFRESH_SECONDS: u64 = 30 * 60;

#[derive(Default)]
pub(crate) struct SearchHost {
    pub(crate) enabled: bool,
    pub(super) audio_toplist: super::audio_toplist::AudioToplist,
    pub(super) default_contexts: super::default_context::DefaultContexts,
    pub(super) preview_config: Option<Arc<crate::protocol::mconfig::PcConfig>>,
    gate: tokio::sync::Mutex<()>,
}

#[derive(Clone, Copy, Deserialize, Serialize, Default)]
#[serde(default)]
pub(super) struct SearchFlags {
    pub(super) v3: bool,
    pub(super) expanded: bool,
    pub(super) dense: bool,
}

#[derive(Deserialize, Serialize, Default)]
struct Assignment {
    attempted_at: u64,
    #[serde(flatten)]
    flags: SearchFlags,
}

fn assignment(value: &Value) -> Option<SearchFlags> {
    let rows = value["data"].as_array().filter(|rows| rows.len() <= 4096)?;
    let enabled = |experiment: &str| {
        rows.iter()
            .find(|row| row["expName"] == experiment)
            .is_some_and(|row| row["expGroupName"] == "t1")
    };
    Some(SearchFlags {
        v3: enabled(EXPERIMENTS[0]),
        expanded: enabled(EXPERIMENTS[1]),
        dense: enabled(EXPERIMENTS[2]),
    })
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
    pub(super) async fn assignment_for(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        api: &dyn Api,
    ) -> Result<SearchFlags> {
        if !self.enabled {
            return Ok(SearchFlags::default());
        }
        let _lock = self.gate.lock().await;
        let store = session.persistent_store()?;
        let old: Assignment = session.with_token(token, |owner| {
            let current = store
                .get(owner, NAMESPACE, ASSIGNMENT_KEY)?
                .and_then(|value| serde_json::from_value(value).ok());
            if let Some(current) = current {
                return Ok(current);
            }
            // The old key knew only the endpoint experiment. Preserve that
            // assignment on a failed refresh without delaying the new lookup.
            let previous = store.get(owner, NAMESPACE, "assignment")?;
            Ok(Assignment {
                flags: SearchFlags {
                    v3: previous.as_ref().is_some_and(|value| value["v3"] == true),
                    ..SearchFlags::default()
                },
                ..Assignment::default()
            })
        })?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default();
        if now >= old.attempted_at && now - old.attempted_at < REFRESH_SECONDS {
            return Ok(old.flags);
        }
        let response = api
            .call(
                AB_PATH,
                json!({"expNames":EXPERIMENTS.join(",")}),
                false,
                &[200],
            )
            .await;
        let updated = Assignment {
            attempted_at: now,
            flags: response
                .as_ref()
                .ok()
                .and_then(assignment)
                .unwrap_or(old.flags),
        };
        session.with_token(token, |owner| {
            store.put(
                owner,
                NAMESPACE,
                ASSIGNMENT_KEY,
                &serde_json::to_value(&updated)?,
            )
        })?;
        Ok(updated.flags)
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
