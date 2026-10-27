//! Profile-local credentials committed atomically in the private SQLite store.
//! An origin is fixed at construction; Cookie domain/path flags stay intact.
use crate::{
    Error, Result,
    protocol::{http::ApiOrigin, wire::crypto::random_bytes},
    storage::StateStore,
};
use cookie_store::{Cookie, CookieStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard};
use url::Url;
use uuid::Uuid;

const OWNER: &str = "transport";
const KIND: &str = "credentials";
const KEY: &str = "session";

#[derive(Clone, Serialize, Deserialize)]
struct Record {
    schema: u32,
    origin: ApiOrigin,
    device_id: String,
    // CookieStore's default Serialize drops nonpersistent cookies. Persist
    // this explicit list instead so an authorized session survives restart.
    cookies: Vec<Cookie<'static>>,
    refresh_token: Option<String>,
    #[serde(default)]
    login_rotation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    login_notice: Option<super::login_policy::LoginNotice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    queue_reset: Option<crate::playback::QueueResetPlan>,
    revision: String,
}

struct State {
    record: Record,
    persisted: Value,
}

pub struct Credentials {
    origin: ApiOrigin,
    store: Arc<StateStore>,
    state: Mutex<State>,
}

/// Private native-side values, intentionally neither Debug nor Serialize.
pub struct LoginCredentials {
    pub token: Option<String>,
    pub refresh_token: Option<String>,
    pub revision: String,
    pub rotation: Option<String>,
}

pub struct RequestCredentials {
    pub target: Url,
    pub device_id: String,
    pub cookies: Vec<(String, String)>,
    pub revision: String,
}

impl Credentials {
    pub fn import_legacy(&self, legacy: super::credential_import::LegacySession) -> Result<()> {
        if legacy.origin != self.origin {
            return Err(Error::InvalidInput("legacy session origin mismatch"));
        }
        self.change(|record| {
            if !record.cookies.is_empty() || record.refresh_token.is_some() {
                return Err(Error::State("import requires an empty credential profile"));
            }
            record.device_id = legacy.device_id;
            record.cookies = legacy.cookies;
            record.refresh_token = legacy.refresh_token;
            record.login_rotation = None;
            Ok(())
        })
    }
    pub fn open(store: Arc<StateStore>, origin: ApiOrigin) -> Result<Self> {
        let (record, persisted) = store.transaction(|tx| {
            if let Some(raw) = tx.get(OWNER, KIND, KEY)? {
                let record: Record = serde_json::from_value(raw.clone())?;
                validate_record(&record, origin)?;
                return Ok((record, raw));
            }
            let record = Record {
                schema: 1,
                origin,
                device_id: hex::encode(random_bytes::<24>()?),
                cookies: vec![],
                refresh_token: None,
                login_rotation: None,
                login_notice: None,
                queue_reset: None,
                revision: Uuid::new_v4().simple().to_string(),
            };
            let raw = serde_json::to_value(&record)?;
            tx.put(OWNER, KIND, KEY, &raw)?;
            Ok((record, raw))
        })?;
        Ok(Self {
            origin,
            store,
            state: Mutex::new(State { record, persisted }),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| Error::LockPoisoned)
    }
    pub fn origin(&self) -> ApiOrigin {
        self.origin
    }
    pub fn device_id(&self) -> Result<String> {
        let state = self.lock()?;
        self.require_current(&state)?;
        Ok(state.record.device_id.clone())
    }

    pub fn cookies_for(&self, url: &Url) -> Result<Vec<(String, String)>> {
        Ok(self.request_credentials(url)?.cookies)
    }

    pub fn request_credentials(&self, url: &Url) -> Result<RequestCredentials> {
        self.origin.require_url(url)?;
        let state = self.lock()?;
        self.require_current(&state)?;
        let jar = jar(&state.record)?;
        let cookies = request_cookies(&jar, url);
        Ok(RequestCredentials {
            target: url.clone(),
            device_id: state.record.device_id.clone(),
            cookies,
            revision: state.record.revision.clone(),
        })
    }

    pub fn login_credentials(&self) -> Result<LoginCredentials> {
        let url = self.origin.request_url(
            crate::protocol::http::WireRoute::Eapi,
            "/api/login/token/refresh/v1",
        )?;
        let state = self.lock()?;
        self.require_current(&state)?;
        let jar = jar(&state.record)?;
        let token = request_cookies(&jar, &url)
            .into_iter()
            .find(|(name, _)| name == "MUSIC_U")
            .map(|(_, value)| value);
        Ok(LoginCredentials {
            token,
            refresh_token: state.record.refresh_token.clone(),
            revision: state.record.revision.clone(),
            rotation: state.record.login_rotation.clone(),
        })
    }

    fn change(&self, operation: impl FnOnce(&mut Record) -> Result<()>) -> Result<()> {
        let mut state = self.lock()?;
        let mut candidate = state.record.clone();
        operation(&mut candidate)?;
        candidate.revision = Uuid::new_v4().simple().to_string();
        validate_record(&candidate, self.origin)?;
        let raw = serde_json::to_value(&candidate)?;
        self.store.transaction(|tx| {
            if tx.get(OWNER, KIND, KEY)?.as_ref() != Some(&state.persisted) {
                return Err(Error::CredentialChanged);
            }
            tx.put(OWNER, KIND, KEY, &raw)
        })?;
        state.record = candidate;
        state.persisted = raw;
        Ok(())
    }

    fn require_current(&self, state: &State) -> Result<()> {
        if self.store.get(OWNER, KIND, KEY)?.as_ref() != Some(&state.persisted) {
            return Err(Error::CredentialChanged);
        }
        Ok(())
    }

    /// Store only a response from this profile's origin. The caller decides
    /// whether a refresh header belongs to an acknowledged login/refresh call.
    pub fn record_response(
        &self,
        url: &Url,
        set_cookies: &[String],
        refresh_token: Option<&str>,
        expected_revision: &str,
    ) -> Result<()> {
        self.origin.require_url(url)?;
        if let Some(token) = refresh_token {
            validate_token(token)?;
        }
        if set_cookies.is_empty() && refresh_token.is_none() {
            let state = self.lock()?;
            self.require_current(&state)?;
            if state.record.revision != expected_revision {
                return Err(Error::CredentialChanged);
            }
            return Ok(());
        }
        self.change(|record| {
            if record.revision != expected_revision {
                return Err(Error::CredentialChanged);
            }
            let mut store = jar(record)?;
            for value in set_cookies {
                // RFC-invalid cookies are ignored, as a browser would. They
                // must not make an acknowledged write appear unacknowledged.
                let _ = store.parse(value, url);
            }
            record.cookies = store.iter_unexpired().cloned().collect();
            if let Some(token) = refresh_token {
                record.refresh_token = Some(token.into());
                let refresh_url = self.origin.request_url(
                    crate::protocol::http::WireRoute::Eapi,
                    "/api/login/token/refresh/v1",
                )?;
                let effective_token = request_cookies(&store, &refresh_url)
                    .into_iter()
                    .find(|(name, _)| name == "MUSIC_U")
                    .map(|(_, value)| value);
                // M/1368 requires both MUSIC_U and x-refresh-token in this
                // acknowledged response, not merely any changed Cookie.
                if set_cookies.iter().any(|value| {
                    Cookie::parse(value.as_str(), url).is_ok_and(|cookie| {
                        cookie.name() == "MUSIC_U"
                            && !cookie.value().is_empty()
                            && !cookie.is_expired()
                            && cookie.matches(&refresh_url)
                            && effective_token.as_deref() == Some(cookie.value())
                    })
                }) {
                    record.login_rotation = Some(Uuid::new_v4().simple().to_string());
                }
            }
            Ok(())
        })
    }

    pub fn set_login_ticket(&self, ticket: &str) -> Result<()> {
        validate_token(ticket)?;
        if ticket.contains(';') {
            return Err(Error::InvalidInput("invalid login challenge ticket"));
        }
        self.change(|record| {
            let mut store = jar(record)?;
            store
                .parse(
                    &format!("MUSIC_TU={ticket}; Path=/; Secure; HttpOnly"),
                    &self.origin.url(),
                )
                .map_err(|_| Error::InvalidInput("invalid login challenge ticket"))?;
            record.cookies = store.iter_unexpired().cloned().collect();
            Ok(())
        })
    }

    pub fn clear(&self) -> Result<()> {
        self.change(|record| {
            if record.queue_reset.is_some() {
                return Err(Error::State(
                    "login queue cleanup must finish before clearing profile",
                ));
            }
            record.cookies.clear();
            record.refresh_token = None;
            record.login_rotation = None;
            record.login_notice = None;
            Ok(())
        })
    }

    pub(crate) fn invalid_login_notice(&self) -> Result<Option<super::login_policy::LoginNotice>> {
        let state = self.lock()?;
        self.require_current(&state)?;
        Ok(state.record.login_notice.clone())
    }

    /// The invalidation and credential removal commit together. A restart must
    /// not automatically restore a server-rejected profile. New login flows
    /// use a fresh isolated candidate; saved user resources remain untouched.
    pub(crate) fn invalidate_login(
        &self,
        notice: &super::login_policy::LoginNotice,
        queue_reset: Option<crate::playback::QueueResetPlan>,
    ) -> Result<()> {
        notice.validate()?;
        if let Some(plan) = &queue_reset {
            plan.validate()?;
        }
        self.change(|record| {
            record.cookies.clear();
            record.refresh_token = None;
            record.login_rotation = None;
            record.login_notice = Some(notice.clone());
            record.queue_reset = queue_reset;
            Ok(())
        })
    }

    pub(crate) fn pending_queue_reset(&self) -> Result<Option<crate::playback::QueueResetPlan>> {
        let state = self.lock()?;
        self.require_current(&state)?;
        Ok(state.record.queue_reset.clone())
    }
    pub(crate) fn finish_queue_reset(&self, id: &str) -> Result<()> {
        self.change(|record| {
            if record
                .queue_reset
                .as_ref()
                .is_none_or(|plan| plan.id() != id)
            {
                return Err(Error::StaleResponse);
            }
            record.queue_reset = None;
            Ok(())
        })
    }

    pub(crate) fn store(&self) -> Arc<StateStore> {
        self.store.clone()
    }
}

fn request_cookies(jar: &CookieStore, url: &Url) -> Vec<(String, String)> {
    let mut cookies = jar.matches(url);
    // The underlying store is a hash map. A scoped authentication cookie
    // must precede a same-name root cookie, including in native headers.
    cookies.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.as_ref().len()));
    cookies
        .into_iter()
        .map(|cookie| (cookie.name().into(), cookie.value().into()))
        .collect()
}

fn jar(record: &Record) -> Result<CookieStore> {
    CookieStore::from_cookies(
        record
            .cookies
            .iter()
            .cloned()
            .map(Ok::<_, cookie_store::CookieError>),
        false,
    )
    .map_err(|_| Error::Response("invalid stored cookies"))
}

fn validate_record(record: &Record, origin: ApiOrigin) -> Result<()> {
    if let Some(plan) = &record.queue_reset {
        plan.validate()?;
        if record.login_notice.is_none() {
            return Err(Error::State(
                "queue cleanup requires a persisted login invalidation",
            ));
        }
    }
    if let Some(notice) = &record.login_notice {
        notice.validate()?;
    }
    if record.schema != 1 || record.origin != origin {
        return Err(Error::State(
            "unsupported credential schema or origin mismatch",
        ));
    }
    if record.device_id.is_empty()
        || record.device_id.len() > 4096
        || record.device_id.contains(['\r', '\n', '\0'])
    {
        return Err(Error::State("invalid device identity"));
    }
    if let Some(token) = &record.refresh_token {
        validate_token(token)?;
    }
    Ok(())
}

fn validate_token(token: &str) -> Result<()> {
    if token.is_empty() || token.len() > 16384 || token.contains(['\r', '\n', '\0']) {
        return Err(Error::InvalidInput("invalid refresh/login credential"));
    }
    Ok(())
}
