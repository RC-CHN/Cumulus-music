//! Same-origin account selection and isolated candidate credential stores.
//! SQLite commits the selected profile and identity together before publishing
//! the in-memory account. Old candidates cannot replace a newer selection.
mod removal;
mod saved;
use super::{AccountSession, SessionToken, credentials::Credentials};
use crate::{
    Error, Result,
    protocol::{
        api::Api,
        context::RequestContext,
        http::{
            ApiOrigin, ClientProfile, HttpBackend, MusicHttpClient, RequestOptions, ReqwestBackend,
        },
    },
    storage::StateStore,
};
pub use saved::SavedProfile;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

const OWNER: &str = "transport";
const KIND: &str = "profiles";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub session_id: String,
    pub revision: String,
    #[serde(default = "remember_default")]
    pub auto_login: bool,
}

fn remember_default() -> bool {
    true
}

pub struct ProfileRegistry {
    store: Arc<StateStore>,
    origin: ApiOrigin,
    root: PathBuf,
}

pub struct LoginCandidate {
    registry: Arc<ProfileRegistry>,
    id: String,
    expected: Option<Selection>,
    client: Arc<MusicHttpClient>,
    credentials: Arc<Credentials>,
    expected_uid: Option<String>,
    auto_login: AtomicBool,
}

/// Native-only handle. It deliberately has no Debug/Serialize implementation.
pub struct SelectedProfile {
    pub selection: Selection,
    pub client: Arc<MusicHttpClient>,
    pub credentials: Arc<Credentials>,
}

fn id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn real_directory(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(Error::State("profile directory is missing or redirected"));
    }
    Ok(())
}

fn private_directory(path: &Path) -> Result<()> {
    let builder = crate::storage::private_directory_builder();
    builder.create(path)?;
    Ok(())
}

impl ProfileRegistry {
    pub fn open(root: &Path, origin: ApiOrigin) -> Result<Arc<Self>> {
        let store = Arc::new(StateStore::open(root)?);
        real_directory(root)?;
        let root = fs::canonicalize(root)?;
        store.transaction(|tx| {
            let expected = json!({"schema":1,"origin":origin});
            match tx.get(OWNER, KIND, "configuration")? {
                Some(value) if value != expected => {
                    Err(Error::State("profile registry origin/schema mismatch"))
                }
                Some(_) => Ok(()),
                None => tx.put(OWNER, KIND, "configuration", &expected),
            }
        })?;
        Ok(Arc::new(Self {
            store,
            origin,
            root,
        }))
    }

    pub fn store(&self) -> Arc<StateStore> {
        self.store.clone()
    }
    pub fn origin(&self) -> ApiOrigin {
        self.origin
    }

    pub fn selection(&self) -> Result<Option<Selection>> {
        let selected: Option<Selection> = self
            .store
            .get(OWNER, KIND, "active")?
            .map(serde_json::from_value)
            .transpose()?;
        if let Some(selected) = &selected {
            if !id(&selected.revision) {
                return Err(Error::State("invalid profile revision"));
            }
            self.profile_path(&selected.session_id)?;
        }
        Ok(selected)
    }

    fn profile_path(&self, session_id: &str) -> Result<PathBuf> {
        if !id(session_id) {
            return Err(Error::State("invalid profile identifier"));
        }
        let parent = self.root.join("sessions");
        real_directory(&parent)?;
        let path = parent.join(session_id);
        real_directory(&path)?;
        if !fs::canonicalize(&path)?.starts_with(&self.root) {
            return Err(Error::State("profile escapes its registry"));
        }
        Ok(path)
    }

    pub fn candidate(self: &Arc<Self>, profile: ClientProfile) -> Result<LoginCandidate> {
        self.candidate_with_backend(
            profile,
            RequestOptions::default(),
            None,
            Arc::new(ReqwestBackend::new(Duration::from_secs(20))?),
        )
    }

    pub fn candidate_with_backend(
        self: &Arc<Self>,
        profile: ClientProfile,
        options: RequestOptions,
        context: Option<Arc<RequestContext>>,
        backend: Arc<dyn HttpBackend>,
    ) -> Result<LoginCandidate> {
        let expected = self.selection()?;
        let parent = self.root.join("sessions");
        match private_directory(&parent) {
            Ok(()) => (),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                real_directory(&parent)?
            }
            Err(error) => return Err(error),
        }
        let id = Uuid::new_v4().simple().to_string();
        private_directory(&parent.join(&id))?;
        let path = self.profile_path(&id)?;
        let store = Arc::new(StateStore::open(&path)?);
        let credentials = Arc::new(Credentials::open(store, self.origin)?);
        let client = Arc::new(MusicHttpClient::with_backend(
            credentials.clone(),
            profile,
            options,
            context,
            backend,
        )?);
        Ok(LoginCandidate {
            registry: self.clone(),
            id,
            expected,
            client,
            credentials,
            expected_uid: None,
            auto_login: AtomicBool::new(true),
        })
    }

    pub fn current(&self, profile: ClientProfile) -> Result<Option<SelectedProfile>> {
        let Some(selection) = self.selection()? else {
            return Ok(None);
        };
        let store = Arc::new(StateStore::open(self.profile_path(&selection.session_id)?)?);
        let credentials = Arc::new(Credentials::open(store, self.origin)?);
        let client = Arc::new(MusicHttpClient::new(credentials.clone(), profile)?);
        Ok(Some(SelectedProfile {
            selection,
            client,
            credentials,
        }))
    }
}

impl LoginCandidate {
    /// Native lifecycle binding; never expose the HTTP client through IPC.
    pub fn client(&self) -> Arc<MusicHttpClient> {
        self.client.clone()
    }
    pub(crate) fn remember(&self, enabled: bool) {
        self.auto_login.store(enabled, Ordering::Release);
    }
    pub fn api(&self) -> Arc<dyn Api> {
        self.client.clone()
    }
    pub fn credentials(&self) -> Arc<Credentials> {
        self.credentials.clone()
    }

    pub async fn verify_and_adopt(
        &self,
        session: &AccountSession,
        expected: &SessionToken,
    ) -> Result<Value> {
        self.registry
            .store
            .transaction(|tx| removal::available(tx, &self.id))?;
        let profile = self.client.account().await?;
        self.adopt(session, expected, profile)
    }

    pub(crate) fn adopt(
        &self,
        session: &AccountSession,
        expected: &SessionToken,
        profile: Value,
    ) -> Result<Value> {
        self.registry.profile_path(&self.id)?;
        let uid = crate::protocol::api::identifier(&profile["userId"])?;
        if self
            .expected_uid
            .as_ref()
            .is_some_and(|expected| expected != &uid)
        {
            return Err(Error::Response("saved account identity changed"));
        }
        // Check that another process did not clear/replace this credential set.
        self.credentials.device_id()?;
        let selection = Selection {
            session_id: self.id.clone(),
            revision: Uuid::new_v4().simple().to_string(),
            auto_login: self.auto_login.load(Ordering::Acquire),
        };
        let saved = SavedProfile::from_verified(&self.id, &profile, selection.auto_login)?;
        let prior = self
            .expected
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        session.adopt_registered(
            self.client.clone(),
            profile,
            expected,
            &self.registry.store,
            |tx| {
                removal::available(tx, &self.id)?;
                if let Some(value) = tx.get(OWNER, saved::KIND, &uid)? {
                    let current: SavedProfile = serde_json::from_value(value)?;
                    removal::available(tx, &current.session_id)?;
                }
                if tx.get(OWNER, KIND, "active")? != prior {
                    return Err(Error::StaleResponse);
                }
                if self.expected_uid.is_some() {
                    let current: SavedProfile = tx
                        .get(OWNER, saved::KIND, &uid)?
                        .ok_or(Error::StaleResponse)
                        .and_then(|v| Ok(serde_json::from_value(v)?))?;
                    if current.session_id != self.id {
                        return Err(Error::StaleResponse);
                    }
                }
                // Selection, identity and the safe account-list metadata commit
                // together. A failed write cannot publish a selectable account.
                tx.put(OWNER, saved::KIND, &uid, &serde_json::to_value(&saved)?)?;
                tx.put(OWNER, KIND, "active", &serde_json::to_value(selection)?)
            },
        )
    }
}
