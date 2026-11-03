//! Native assignment for the official Standard quality trial menu.
use crate::{
    Result,
    account::{AccountSession, SessionToken, login_policy::AB_PATH},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const NAME: &str = "StandardAudio_VIPLogo_PCV2";
const CACHE: &str = "quality-experiment-v1";
const INTERVAL_SECONDS: u64 = 30 * 60;
const LOOKUP_BUDGET: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct QualityExperiment {
    pub(super) enabled: bool,
    gate: tokio::sync::Mutex<()>,
}

#[derive(Default, Deserialize, Serialize)]
struct Assignment {
    attempted_at: u64,
    standard_trial: Option<bool>,
}

fn assigned(value: &Value) -> Option<bool> {
    let rows = value["data"].as_array().filter(|rows| rows.len() <= 4096)?;
    match rows.iter().find(|row| row["expName"] == NAME) {
        Some(row) => row["expGroupName"].as_str().map(|group| group == "t1"),
        // M/147 resets a missing experiment to the built-in empty group.
        None => Some(false),
    }
}

impl QualityExperiment {
    pub(super) async fn standard_trial_for(
        &self,
        session: &AccountSession,
        token: &SessionToken,
    ) -> Result<Option<bool>> {
        if !self.enabled {
            return Ok(None);
        }
        let _gate = self.gate.lock().await;
        let store = session.persistent_store()?;
        let previous: Assignment = session.with_token(token, |uid| {
            Ok(store
                .get(uid, CACHE, NAME)?
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default())
        })?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        if now >= previous.attempted_at && now - previous.attempted_at < INTERVAL_SECONDS {
            return Ok(previous.standard_trial);
        }
        // Optional menu metadata must not intercept login failures or retry a
        // song authorization. The outer session fence still checks account identity.
        let observed = session
            .run_for(token, |api, _| async move {
                let result = tokio::time::timeout(
                    LOOKUP_BUDGET,
                    api.call(AB_PATH, json!({"expNames":NAME}), false, &[200]),
                )
                .await;
                Ok(result.ok().and_then(Result::ok).as_ref().and_then(assigned))
            })
            .await?;
        let updated = Assignment {
            attempted_at: now,
            standard_trial: observed.or(previous.standard_trial),
        };
        session.with_token(token, |uid| {
            store.put(uid, CACHE, NAME, &serde_json::to_value(&updated)?)
        })?;
        Ok(updated.standard_trial)
    }
}
