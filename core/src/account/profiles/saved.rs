//! Previously verified account handles. A saved identity is never sufficient
//! to authenticate: every explicit switch reads account/get before adoption.
use super::*;
use crate::protocol::api::identifier;

pub(super) const KIND: &str = "saved-profiles-v1";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedProfile {
    pub session_id: String,
    pub uid: String,
    pub nickname: String,
    pub avatar_url: Option<String>,
    pub auto_login: bool,
}

impl SavedProfile {
    pub(super) fn from_verified(
        session_id: &str,
        profile: &Value,
        auto_login: bool,
    ) -> Result<Self> {
        Ok(Self {
            session_id: session_id.into(),
            uid: identifier(&profile["userId"])?,
            nickname: profile["nickname"].as_str().unwrap_or_default().into(),
            avatar_url: profile["avatarUrl"].as_str().map(str::to_owned),
            auto_login,
        })
    }
}

impl ProfileRegistry {
    pub fn saved(&self) -> Result<Vec<SavedProfile>> {
        self.store
            .values(OWNER, KIND)?
            .into_iter()
            .map(|(uid, value)| {
                let record: SavedProfile = serde_json::from_value(value)?;
                if record.uid != uid || !id(&record.session_id) || identifier(&json!(&uid))? != uid
                {
                    return Err(Error::State("invalid saved account record"));
                }
                Ok(record)
            })
            .collect()
    }

    pub fn saved_candidate(
        self: &Arc<Self>,
        session_id: &str,
        profile: ClientProfile,
    ) -> Result<LoginCandidate> {
        self.saved_candidate_with_backend(
            session_id,
            profile,
            RequestOptions::default(),
            None,
            Arc::new(ReqwestBackend::new(Duration::from_secs(20))?),
        )
    }

    /// Native injection shares the production credential scope and wire path.
    pub fn saved_candidate_with_backend(
        self: &Arc<Self>,
        session_id: &str,
        profile: ClientProfile,
        options: RequestOptions,
        context: Option<Arc<RequestContext>>,
        backend: Arc<dyn HttpBackend>,
    ) -> Result<LoginCandidate> {
        let expected = self.selection()?;
        let record = self
            .saved()?
            .into_iter()
            .find(|r| r.session_id == session_id)
            .ok_or(Error::State("saved account is no longer available"))?;
        self.store
            .transaction(|tx| removal::available(tx, session_id))?;
        let store = Arc::new(StateStore::open(self.profile_path(session_id)?)?);
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
            id: record.session_id,
            expected,
            client,
            credentials,
            expected_uid: Some(record.uid),
            auto_login: AtomicBool::new(record.auto_login),
        })
    }

    /// Used only after the selected legacy profile has been read and verified.
    /// Explicit external profiles are not silently copied into the registry.
    pub fn record_current(
        &self,
        session: &AccountSession,
        client: &Arc<MusicHttpClient>,
    ) -> Result<()> {
        let Some(selection) = self.selection()? else {
            return Ok(());
        };
        if client.credentials().store().directory() != self.profile_path(&selection.session_id)? {
            return Ok(());
        }
        let snapshot = session.snapshot()?;
        session.require_api(&snapshot.token, &(client.clone() as Arc<dyn Api>))?;
        let profile = snapshot
            .profile
            .ok_or(Error::State("no verified account profile"))?;
        let record =
            SavedProfile::from_verified(&selection.session_id, &profile, selection.auto_login)?;
        session.with_token(&snapshot.token, |uid| {
            self.store.transaction(|tx| {
                removal::available(tx, &selection.session_id)?;
                if tx.get(OWNER, super::KIND, "active")? != Some(serde_json::to_value(&selection)?)
                {
                    return Err(Error::StaleResponse);
                }
                tx.put(OWNER, KIND, uid, &serde_json::to_value(record)?)
            })
        })
    }
}
