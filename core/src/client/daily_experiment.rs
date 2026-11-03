//! Native assignment for M/244's PH-PC-Blacklist recommendation split.
use super::MusicClient;
use crate::{
    Result,
    account::{SessionToken, login_policy::AB_PATH},
    catalog::DailyRecommendationEndpoint,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const NAME: &str = "PH-PC-Blacklist";
const CACHE: &str = "daily-experiment-v1";
const INTERVAL: Duration = Duration::from_secs(30 * 60);
const LOOKUP_BUDGET: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct DailyExperiment {
    enabled: bool,
    gate: tokio::sync::Mutex<()>,
}

#[derive(Default, Deserialize, Serialize)]
struct Assignment {
    attempted_at: u64,
    v3: bool,
}

fn assignment(value: &Value) -> Option<bool> {
    let rows = value["data"]
        .as_array()
        .filter(|rows| rows.len() <= 4096 && rows.iter().all(Value::is_object))?;
    let mut matched = rows.iter().filter(|row| row["expName"] == NAME);
    let selected = matched.next();
    if matched.next().is_some() {
        return None;
    }
    match selected {
        Some(row) => row["expGroupName"].as_str().map(|group| group == "t1"),
        // M/147 replaces absent assignments with the built-in empty group.
        None => Some(false),
    }
}

impl MusicClient {
    /// Production desktop explicitly enables official assignment; independent
    /// hosts can retain their established direct-v3 integration.
    pub fn with_official_daily_experiment(mut self) -> Self {
        self.daily_experiment.enabled = true;
        self
    }

    pub(super) async fn daily_recommendation_endpoint(
        &self,
        token: &SessionToken,
    ) -> Result<DailyRecommendationEndpoint> {
        if !self.daily_experiment.enabled {
            return Ok(DailyRecommendationEndpoint::V3);
        }
        let _guard = self.daily_experiment.gate.lock().await;
        let store = self.session.persistent_store()?;
        let previous: Assignment = self.session.with_token(token, |owner| {
            Ok(store
                .get(owner, CACHE, NAME)?
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default())
        })?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let use_v3 =
            if now >= previous.attempted_at && now - previous.attempted_at < INTERVAL.as_secs() {
                previous.v3
            } else {
                // Experiment metadata is optional and must not enter recursive
                // login recovery. The chosen business read uses run_for normally.
                let observed = self
                    .session
                    .run_for(token, |api, _| async move {
                        let result = tokio::time::timeout(
                            LOOKUP_BUDGET,
                            api.call(AB_PATH, json!({"expNames":NAME}), false, &[200]),
                        )
                        .await;
                        Ok(result
                            .ok()
                            .and_then(Result::ok)
                            .as_ref()
                            .and_then(assignment))
                    })
                    .await?;
                let updated = Assignment {
                    attempted_at: now,
                    v3: observed.unwrap_or(previous.v3),
                };
                self.session.with_token(token, |owner| {
                    store.put(owner, CACHE, NAME, &serde_json::to_value(&updated)?)
                })?;
                updated.v3
            };
        Ok(if use_v3 {
            DailyRecommendationEndpoint::V3
        } else {
            DailyRecommendationEndpoint::V1
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Error,
        account::{AccountSession, profiles::ProfileRegistry},
        protocol::{
            api::{Api, ApiFuture},
            http::ApiOrigin,
        },
    };
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    struct Script {
        responses: Mutex<VecDeque<Result<Value>>>,
    }

    impl Api for Script {
        fn request<'a>(
            &'a self,
            path: &'a str,
            payload: Value,
            cache: bool,
        ) -> ApiFuture<'a, Value> {
            Box::pin(async move {
                assert_eq!(path, AB_PATH);
                assert_eq!(payload, json!({"expNames":NAME}));
                assert!(!cache);
                self.responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("unexpected experiment read")
            })
        }
    }

    #[tokio::test]
    async fn opt_in_assignment_is_account_scoped_and_failed_refresh_keeps_last_success() {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let api = Arc::new(Script {
            responses: Mutex::new(VecDeque::from([
                Ok(json!({"code":200,"data":[{"expName":NAME,"expGroupName":"t1"}]})),
                Err(Error::Transport("synthetic failure".into())),
                Ok(json!({"code":200,"data":[]})),
                Ok(json!({"code":200,"data":[{"expName":NAME,"expGroupName":"c"}]})),
            ])),
        });
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        let adopt = |id| {
            session
                .adopt(api.clone(), json!({"userId":id}), &session.token().unwrap())
                .unwrap();
            session.token().unwrap()
        };
        let first = adopt(1);
        let client = MusicClient::with_session(session.clone(), registry);
        assert_eq!(
            client.daily_recommendation_endpoint(&first).await.unwrap(),
            DailyRecommendationEndpoint::V3
        );
        assert_eq!(api.responses.lock().unwrap().len(), 4);
        let client = client.with_official_daily_experiment();
        for _ in 0..2 {
            assert_eq!(
                client.daily_recommendation_endpoint(&first).await.unwrap(),
                DailyRecommendationEndpoint::V3
            );
        }
        assert_eq!(api.responses.lock().unwrap().len(), 3);
        store
            .put("1", CACHE, NAME, &json!({"attempted_at":0,"v3":true}))
            .unwrap();
        assert_eq!(
            client.daily_recommendation_endpoint(&first).await.unwrap(),
            DailyRecommendationEndpoint::V3
        );
        let second = adopt(2);
        assert_eq!(
            client.daily_recommendation_endpoint(&second).await.unwrap(),
            DailyRecommendationEndpoint::V1
        );
        assert!(matches!(
            client.daily_recommendation_endpoint(&first).await,
            Err(Error::StaleResponse)
        ));
        let third = adopt(3);
        assert_eq!(
            client.daily_recommendation_endpoint(&third).await.unwrap(),
            DailyRecommendationEndpoint::V1
        );
        assert!(api.responses.lock().unwrap().is_empty());
    }

    #[test]
    fn malformed_or_duplicate_assignments_do_not_overwrite_previous_result() {
        assert_eq!(assignment(&json!({"data":[{"expName":NAME}]})), None);
        assert_eq!(assignment(&json!({"data":[null]})), None);
        assert_eq!(
            assignment(
                &json!({"data":[{"expName":NAME,"expGroupName":"c"},{"expName":NAME,"expGroupName":"t1"}]})
            ),
            None
        );
        assert_eq!(
            assignment(&json!({"data":[{"expName":"other","expGroupName":"t1"}]})),
            Some(false)
        );
    }
}
