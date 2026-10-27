use super::normalized_path;
use crate::{Error, Result, protocol::api::Api};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

pub const REFRESH_PATH: &str = "/api/middle/account/token/refresh";
const DEFAULT_MESSAGE: &str = "登录状态已失效，请重新登录";
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenPresence {
    pub music_u: bool,
    pub music_a: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidLoginSkip {
    InProgress,
    NoToken,
    RefreshEndpoint,
    AnonymousWhitelist,
    RecentValid,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginNoticeKind {
    Expired,
    Reauthenticate,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginNotice {
    pub kind: LoginNoticeKind,
    pub title: Option<String>,
    pub message: String,
}
impl LoginNotice {
    pub(crate) fn validate(&self) -> Result<()> {
        if bounded_text(&json!(self.message)).is_none()
            || self
                .title
                .as_ref()
                .is_some_and(|title| bounded_text(&json!(title)).is_none())
        {
            return Err(Error::Response("invalid stored login notice"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum InvalidLoginOutcome {
    Skipped(InvalidLoginSkip),
    Valid,
    Failed {
        count: u64,
    },
    /// M/198 ignores defined actions other than numeric 2; action null and
    /// string "2" are defined values and must not become logout instructions.
    IgnoredAction,
    Notice(LoginNotice),
}
#[derive(Default)]
struct State {
    errors: u64,
    last_valid: u64,
}
pub struct InvalidLoginController {
    gate: tokio::sync::Mutex<()>,
    state: Mutex<State>,
    clock: Arc<dyn Fn() -> Result<u64> + Send + Sync>,
}
impl Default for InvalidLoginController {
    fn default() -> Self {
        Self::with_clock(Arc::new(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
                .map_err(|_| Error::State("system clock precedes UNIX epoch"))
        }))
    }
}
impl InvalidLoginController {
    pub fn with_clock(clock: Arc<dyn Fn() -> Result<u64> + Send + Sync>) -> Self {
        Self {
            gate: Default::default(),
            state: Default::default(),
            clock,
        }
    }
    fn fail(&self, business_response: bool) -> Result<InvalidLoginOutcome> {
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        state.errors = state.errors.saturating_add(1);
        // The recovered catch branch increments only. Its >=3 test exists
        // exclusively in the non-200 business-response branch.
        if business_response && state.errors >= 3 {
            state.errors = 0;
            return Ok(InvalidLoginOutcome::Notice(LoginNotice {
                kind: LoginNoticeKind::Expired,
                title: None,
                message: DEFAULT_MESSAGE.into(),
            }));
        }
        Ok(InvalidLoginOutcome::Failed {
            count: state.errors,
        })
    }
    /// `config` is the native customConfig.query result, not arbitrary frontend
    /// input. PC M/290 supplies null for this key. Tests can exercise the generic
    /// vendor whitelist branch without pretending the PC bridge returns it.
    pub async fn handle(
        &self,
        api: &dyn Api,
        tokens: TokenPresence,
        from_api: &str,
        config: Result<Value>,
    ) -> Result<InvalidLoginOutcome> {
        let Ok(_gate) = self.gate.try_lock() else {
            return Ok(InvalidLoginOutcome::Skipped(InvalidLoginSkip::InProgress));
        };
        if !tokens.music_u && !tokens.music_a {
            return Ok(InvalidLoginOutcome::Skipped(InvalidLoginSkip::NoToken));
        }
        if normalized_path(from_api) == "/middle/account/token/refresh" {
            return Ok(InvalidLoginOutcome::Skipped(
                InvalidLoginSkip::RefreshEndpoint,
            ));
        }
        if from_api.len() > 4096 || from_api.chars().any(char::is_control) {
            return Err(Error::InvalidInput("invalid intercepted API path"));
        }
        let config = match config {
            Ok(v) => v,
            Err(_) => return self.fail(false),
        };
        match whitelist_matches(&config, from_api) {
            Ok(true) if tokens.music_a => {
                return Ok(InvalidLoginOutcome::Skipped(
                    InvalidLoginSkip::AnonymousWhitelist,
                ));
            }
            Ok(_) => (),
            Err(_) => return self.fail(false),
        }
        let started = (self.clock)()?;
        {
            let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            if state.last_valid != 0 && started < state.last_valid.saturating_add(5000) {
                return Ok(InvalidLoginOutcome::Skipped(InvalidLoginSkip::RecentValid));
            }
        }
        let response = match api
            .request(REFRESH_PATH, json!({"fromAPI":from_api}), false)
            .await
        {
            Ok(v) => v,
            Err(_) => return self.fail(false),
        };
        if response["code"] != json!(200) {
            return self.fail(true);
        }
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        state.errors = 0;
        let action = response["data"].get("action");
        if action == Some(&json!(1)) {
            state.last_valid = started;
            return Ok(InvalidLoginOutcome::Valid);
        }
        let kind = match action {
            None => LoginNoticeKind::Expired,
            Some(v) if v == &json!(2) => LoginNoticeKind::Reauthenticate,
            _ => return Ok(InvalidLoginOutcome::IgnoredAction),
        };
        Ok(InvalidLoginOutcome::Notice(LoginNotice {
            kind,
            title: bounded_text(&response["data"]["title"]),
            message: bounded_text(&response["data"]["message"])
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| DEFAULT_MESSAGE.into()),
        }))
    }
}
pub(super) fn bounded_text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| {
            s.chars().count() <= 512
                && !s.contains(['\0', '\r'])
                && !["MUSIC_U=", "MUSIC_A=", "refreshToken", "x-refresh-token"]
                    .iter()
                    .any(|key| s.contains(key))
        })
        .map(str::to_owned)
}
fn whitelist_matches(config: &Value, path: &str) -> Result<bool> {
    let Some(list) = config.get("config") else {
        return Ok(false);
    };
    let Some(entries) = list.as_array() else {
        // JS checks truthy length before calling .map. Ordinary objects, null,
        // booleans and numbers have no length; nonempty strings throw at .map.
        if list.as_str().is_some_and(|s| !s.is_empty())
            || list
                .get("length")
                .is_some_and(crate::protocol::wire::serialization::js_truthy)
        {
            return Err(Error::Response("invalid login whitelist"));
        }
        return Ok(false);
    };
    if entries.len() > 4096 {
        return Err(Error::Response("login whitelist too large"));
    }
    let mut paths = Vec::with_capacity(entries.len());
    for entry in entries {
        let value = match entry {
            Value::String(s) => normalized_path(s),
            Value::Null | Value::Bool(false) => String::new(),
            Value::Number(n) if n.as_f64() == Some(0.0) => String::new(),
            _ => return Err(Error::Response("invalid login whitelist item")),
        };
        paths.push(value);
    }
    Ok(paths.contains(&normalized_path(path)))
}
