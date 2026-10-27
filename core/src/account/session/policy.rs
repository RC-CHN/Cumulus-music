//! Account-scoped official login experiments and intercepted error policy.
use super::*;
use crate::account::login_policy::{
    AB_PATH, InvalidLoginController, InvalidLoginOutcome, LegacyLoginController,
    LegacyLoginOutcome, LoginExperiments, REFRESH_PATH, TokenPresence,
};
use crate::protocol::http::{MusicHttpClient, WireRoute};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) enum Interception {
    Deferred,
    Invalid {
        notice: LoginNotice,
        clear_queue: bool,
    },
}

/// M/290 nativeRequest returns failed network callbacks as a response body.
/// This is distinct from the generic vendor adapter throwing an exception.
struct PcValidationApi(Arc<MusicHttpClient>);
impl Api for PcValidationApi {
    fn request<'a>(
        &'a self,
        path: &'a str,
        payload: Value,
        _cache: bool,
    ) -> crate::protocol::api::ApiFuture<'a, Value> {
        Box::pin(async move {
            match self.0.request_jsk(path, payload).await {
                Ok(value) => Ok(value),
                Err(Error::HttpStatus {
                    body: Some(body), ..
                }) => Ok(*body),
                Err(Error::Transport(_) | Error::HttpStatus { .. }) => Ok(json!({"code":-1})),
                Err(error) => Err(error),
            }
        })
    }
}

#[derive(Default)]
pub(super) struct PolicyHost {
    enabled: AtomicBool,
    scope: Mutex<Option<(SessionToken, Arc<Scope>)>>,
}
#[derive(Default)]
struct Cache {
    loaded: bool,
    attempted: Option<u64>,
    flags: LoginExperiments,
}
#[derive(Default)]
struct Scope {
    gate: tokio::sync::Mutex<()>,
    cache: Mutex<Cache>,
    invalid: InvalidLoginController,
    legacy: LegacyLoginController,
}
impl PolicyHost {
    fn scope(&self, token: &SessionToken) -> Result<Arc<Scope>> {
        let mut slot = self.scope.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some((_, scope)) = slot.as_ref().filter(|(current, _)| current == token) {
            return Ok(scope.clone());
        }
        let scope = Arc::new(Scope::default());
        *slot = Some((token.clone(), scope.clone()));
        Ok(scope)
    }
    pub(super) fn configured(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
}
impl Scope {
    async fn experiments(
        &self,
        api: &dyn Api,
        token: &SessionToken,
        store: Option<&StateStore>,
    ) -> Result<bool> {
        let _gate = self.gate.lock().await;
        let owner = token.uid.as_deref().unwrap_or("unresolved");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::State("system clock precedes UNIX epoch"))?
            .as_secs();
        {
            let mut cache = self.cache.lock().map_err(|_| Error::LockPoisoned)?;
            if !cache.loaded {
                if let Some(store) = store
                    && let Some(value) = store.get(owner, "login-policy-v1", "pc")?
                {
                    // M/147 restores abtest-cached-exps and defaults on malformed
                    // JSON. This cache contains only bounded boolean flags.
                    cache.flags = serde_json::from_value(value).unwrap_or_default();
                }
                cache.loaded = true;
            }
            if cache
                .attempted
                .is_some_and(|last| now < last.saturating_add(1800))
            {
                return Ok(cache.flags.enabled());
            }
        }
        // Direct Api call deliberately bypasses AccountSession::reject: a failed
        // experiment lookup must never recursively check its own login status.
        let response = api
            .request(AB_PATH, LoginExperiments::payload(), false)
            .await
            .and_then(|v| LoginExperiments::from_response(&v));
        let mut cache = self.cache.lock().map_err(|_| Error::LockPoisoned)?;
        if let Ok(flags) = response {
            if let Some(store) = store {
                store.put(
                    owner,
                    "login-policy-v1",
                    "pc",
                    &serde_json::to_value(&flags)?,
                )?;
            }
            cache.flags = flags;
        }
        // M/147 retains its old/default groups on failure and checks again on
        // the next 30-minute refresh. Never synthesize a t1 assignment.
        cache.attempted = Some(now);
        Ok(cache.flags.enabled())
    }
}
impl AccountSession {
    pub(crate) fn enable_official_login_policy(&self) {
        self.policy.enabled.store(true, Ordering::Release);
    }
    pub(crate) fn login_policy_enabled(&self, token: &SessionToken) -> Result<Option<bool>> {
        self.check_generation(token)?;
        if !self.policy.configured() {
            return Ok(None);
        }
        let scope = self.policy.scope(token)?;
        let cache = scope.cache.lock().map_err(|_| Error::LockPoisoned)?;
        Ok(cache.attempted.map(|_| cache.flags.enabled()))
    }
    pub(crate) async fn refresh_login_policy(&self, token: &SessionToken) -> Result<bool> {
        let api = self.native_api(token)?;
        if !self.policy.configured() {
            return Ok(true);
        }
        let scope = self.policy.scope(token)?;
        let native: Arc<dyn std::any::Any + Send + Sync> = api.clone();
        let store = native
            .downcast::<MusicHttpClient>()
            .ok()
            .map(|client| client.credentials().store());
        let enabled = scope
            .experiments(api.as_ref(), token, store.as_deref())
            .await?;
        self.require_api(token, &api)?;
        Ok(enabled)
    }
    pub(super) async fn intercept_login_error(
        &self,
        token: &SessionToken,
        error: &Error,
    ) -> Result<Option<Interception>> {
        if !self.policy.configured() {
            return Ok(None);
        }
        let path = match error.root() {
            Error::Service { endpoint, code, .. }
                if super::legacy_api::intercepted_code(code) && endpoint.starts_with("/api/") =>
            {
                endpoint
            }
            Error::HttpStatus {
                endpoint: Some(endpoint),
                body: Some(body),
                ..
            } if super::legacy_api::intercepted_code(&body["code"]) => endpoint,
            _ => return Ok(None),
        };
        let api = self.native_api(token)?;
        let native: Arc<dyn std::any::Any + Send + Sync> = api.clone();
        let Ok(client) = native.downcast::<MusicHttpClient>() else {
            return Ok(None);
        };
        let scope = self.policy.scope(token)?;
        let store = client.credentials().store();
        let jsk = scope.experiments(api.as_ref(), token, Some(&store)).await?;
        self.require_api(token, &api)?;
        let credentials = client.credentials();
        let target = credentials
            .origin()
            .request_url(WireRoute::Eapi, REFRESH_PATH)?;
        let cookies = credentials.cookies_for(&target)?;
        let tokens = TokenPresence {
            music_u: cookies
                .iter()
                .any(|(key, value)| key == "MUSIC_U" && !value.is_empty()),
            music_a: cookies
                .iter()
                .any(|(key, value)| key == "MUSIC_A" && !value.is_empty()),
        };
        if !jsk {
            // Our host has no official anonymous-account state machine yet.
            // Do not infer isAnonymous from a missing Cookie or invent its UID.
            if token.uid.is_none() && !tokens.music_u {
                return Ok(None);
            }
            let result = scope
                .legacy
                .handle(&super::legacy_api::LegacyValidationApi(client))
                .await;
            self.require_api(token, &api)?;
            return Ok(Some(match result {
                Ok(LegacyLoginOutcome::Notice {
                    notice,
                    clear_queue,
                }) => Interception::Invalid {
                    notice,
                    clear_queue,
                },
                Ok(_) => Interception::Deferred,
                Err(error) => match error.root() {
                    Error::Service { .. }
                    | Error::Transport(_)
                    | Error::HttpStatus { .. }
                    | Error::Response(_) => Interception::Deferred,
                    _ => return Err(error),
                },
            }));
        }
        // M/290 customConfig supports only nm#play#emotionEffect; the login
        // whitelist key yields null. Do not invent a PC whitelist from MConfig.
        let result = scope
            .invalid
            .handle(&PcValidationApi(client), tokens, path, Ok(Value::Null))
            .await?;
        self.require_api(token, &api)?;
        Ok(Some(match result {
            InvalidLoginOutcome::Notice(notice) => Interception::Invalid {
                notice,
                clear_queue: false,
            },
            _ => Interception::Deferred,
        }))
    }
}
