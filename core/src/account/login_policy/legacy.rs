//! M/1384 authenticated api301$ subscriber, separate from the JSK controller.
use super::{InvalidLoginSkip, LoginNotice, LoginNoticeKind, REFRESH_PATH, invalid::bounded_text};
use crate::{Error, Result, protocol::api::Api};
use serde::Serialize;
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LegacyLoginOutcome {
    Skipped(InvalidLoginSkip),
    Valid,
    Failed {
        count: u64,
    },
    IgnoredAction,
    Notice {
        notice: LoginNotice,
        clear_queue: bool,
    },
}
#[derive(Default)]
struct State {
    errors: u64,
    cooldown_until: Option<u64>,
}
pub struct LegacyLoginController {
    gate: tokio::sync::Mutex<()>,
    state: Mutex<State>,
    clock: Arc<dyn Fn() -> Result<u64> + Send + Sync>,
}
impl Default for LegacyLoginController {
    fn default() -> Self {
        // Original setTimeout starts after the response. A monotonic clock
        // preserves that behavior across wall-clock corrections.
        let started = tokio::time::Instant::now();
        Self::with_clock(Arc::new(move || {
            Ok(started.elapsed().as_millis().min(u64::MAX as u128) as u64)
        }))
    }
}
impl LegacyLoginController {
    pub fn with_clock(clock: Arc<dyn Fn() -> Result<u64> + Send + Sync>) -> Self {
        Self {
            gate: Default::default(),
            state: Default::default(),
            clock,
        }
    }
    /// This is the authenticated host branch. Absence of MUSIC_U is not proof
    /// that the official host's isAnonymous flag is true.
    pub async fn handle(&self, api: &dyn Api) -> Result<LegacyLoginOutcome> {
        let Ok(_gate) = self.gate.try_lock() else {
            return Ok(LegacyLoginOutcome::Skipped(InvalidLoginSkip::InProgress));
        };
        let now = (self.clock)()?;
        if self
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .cooldown_until
            .is_some_and(|until| now < until)
        {
            return Ok(LegacyLoginOutcome::Skipped(InvalidLoginSkip::InProgress));
        }
        // Unlike the JSK bridge, native legacy transport rejects non-success
        // response codes. Do not convert network errors into business failures.
        // RAII deliberately fixes the original subscriber's stuck pe latch on
        // rejection/cancellation; neither condition increments its fe counter.
        let response = api.request(REFRESH_PATH, json!({}), false).await?;
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        let action = response["data"].get("action");
        if response["code"].as_f64() != Some(200.0) || action.is_some_and(|v| v.is_null()) {
            state.errors = state.errors.saturating_add(1);
            if state.errors == 3 {
                return Ok(LegacyLoginOutcome::Notice {
                    notice: LoginNotice {
                        kind: LoginNoticeKind::Expired,
                        title: None,
                        message: bounded_text(&response["data"]["message"])
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "用户验证失效，请重新登录".into()),
                    },
                    clear_queue: false,
                });
            }
            return Ok(LegacyLoginOutcome::Failed {
                count: state.errors,
            });
        }
        state.errors = 0;
        if action.and_then(|v| v.as_f64()) == Some(1.0) {
            state.cooldown_until = Some((self.clock)()?.saturating_add(5000));
            return Ok(LegacyLoginOutcome::Valid);
        }
        if action.and_then(|v| v.as_f64()) == Some(2.0) {
            return Ok(LegacyLoginOutcome::Notice {
                notice: LoginNotice {
                    kind: LoginNoticeKind::Reauthenticate,
                    title: bounded_text(&response["data"]["title"]),
                    message: bounded_text(&response["data"]["message"])
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "用户验证失效，请重新登录".into()),
                },
                clear_queue: response["data"]["followAction"].as_f64() == Some(1.0),
            });
        }
        Ok(LegacyLoginOutcome::IgnoredAction)
    }
}
