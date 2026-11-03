//! Explicit native authorization for previously verified local resources.
//! This does not authenticate AccountSession or authorize any online request.
use super::*;
use crate::account::SessionStatus;

const BINDING: &str = "cold-offline-binding-v1";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    selection: Selection,
    uid: String,
    credential_revision: String,
}

/// A process-local, revocable grant. It is intentionally neither a SessionToken
/// nor serializable, and its account identifier is private to native core.
pub struct ColdOfflineGrant {
    registry: Arc<ProfileRegistry>,
    online: Arc<AccountSession>,
    generation: u64,
    credentials: Arc<Credentials>,
    binding: Binding,
    revoked: AtomicBool,
}

pub(super) fn store_verified(
    tx: &mut crate::storage::StateTransaction<'_>,
    selection: &Selection,
    uid: &str,
    credential_revision: Option<&str>,
) -> Result<()> {
    if let Some(revision) = credential_revision {
        tx.put(
            OWNER,
            KIND,
            BINDING,
            &serde_json::to_value(Binding {
                selection: selection.clone(),
                uid: uid.into(),
                credential_revision: revision.into(),
            })?,
        )
    } else {
        tx.delete(OWNER, KIND, BINDING)
    }
}

pub(crate) fn clear_binding_for(store: &StateStore, credentials: &Credentials) -> Result<()> {
    let root = fs::canonicalize(store.directory())?;
    store.transaction(|tx| {
        let Some(raw) = tx.get(OWNER, KIND, BINDING)? else {
            return Ok(());
        };
        let binding: Binding = serde_json::from_value(raw)?;
        if id(&binding.selection.session_id)
            && credentials.store().directory()
                == root.join("sessions").join(&binding.selection.session_id)
        {
            tx.delete(OWNER, KIND, BINDING)?;
        }
        Ok(())
    })
}

impl ProfileRegistry {
    /// Native host opt-in for a cold, still unauthenticated process. Callers
    /// cannot choose a UID, credential path or saved slot for this grant.
    pub fn grant_cold_offline(
        self: &Arc<Self>,
        online: Arc<AccountSession>,
    ) -> Result<ColdOfflineGrant> {
        let snapshot = online.snapshot()?;
        if snapshot.status != SessionStatus::Unknown || snapshot.token.uid.is_some() {
            return Err(Error::State(
                "cold offline mode requires an unverified online session",
            ));
        }
        if !Arc::ptr_eq(&self.store, &online.persistent_store()?) {
            return Err(Error::StaleResponse);
        }
        let api: Arc<dyn std::any::Any + Send + Sync> = online.native_api(&snapshot.token)?;
        let native = api
            .downcast::<MusicHttpClient>()
            .map_err(|_| Error::State("cold offline mode requires the active native profile"))?;
        let credentials = native.credentials().clone();
        let binding = self
            .store
            .get(OWNER, KIND, BINDING)?
            .ok_or(Error::State("no verified cold offline profile"))?;
        let binding: Binding = serde_json::from_value(binding)?;
        let grant = ColdOfflineGrant {
            registry: self.clone(),
            online,
            generation: snapshot.token.generation,
            credentials,
            binding,
            revoked: AtomicBool::new(false),
        };
        grant.require()?;
        Ok(grant)
    }
}

impl ColdOfflineGrant {
    /// Hosts explicitly close their local mode before switch/logout and stop
    /// the associated decoder. Any authority change also invalidates the grant.
    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
    }

    pub fn validate(&self) -> Result<()> {
        self.require()
    }

    /// Blocking IO. The caller passes a task ID, never an owner or path; UI
    /// projection and decoder lifecycle are intentionally not connected yet.
    pub fn open_plain_audio(
        &self,
        task_id: &str,
    ) -> Result<crate::download::offline::ColdOfflineAudio<'_>> {
        self.require()?;
        let (file, quality) = crate::download::offline::open_plain_audio(
            &self.registry.store,
            &self.binding.uid,
            task_id,
        )?;
        self.require()?;
        Ok(crate::download::offline::ColdOfflineAudio::new(
            self, file, quality,
        ))
    }

    fn require(&self) -> Result<()> {
        let online = self.online.snapshot()?;
        if self.revoked.load(Ordering::Acquire)
            || online.status != SessionStatus::Unknown
            || online.token.generation != self.generation
            || online.token.uid.is_some()
        {
            return Err(Error::StaleResponse);
        }
        let expected = &self.binding.selection;
        if !expected.auto_login
            || self.registry.selection()?.as_ref() != Some(expected)
            || self.credentials.store().directory()
                != self.registry.profile_path(&expected.session_id)?
            || self.credentials.offline_credential_revision()?.as_deref()
                != Some(&self.binding.credential_revision)
        {
            return Err(Error::StaleResponse);
        }
        self.registry.store.transaction(|tx| {
            removal::available(tx, &expected.session_id)?;
            if tx.get(OWNER, KIND, "active")? != Some(serde_json::to_value(expected)?)
                || tx.get(OWNER, KIND, BINDING)? != Some(serde_json::to_value(&self.binding)?)
            {
                return Err(Error::StaleResponse);
            }
            let saved: SavedProfile = serde_json::from_value(
                tx.get(OWNER, saved::KIND, &self.binding.uid)?
                    .ok_or(Error::StaleResponse)?,
            )?;
            if saved.uid != self.binding.uid
                || saved.session_id != expected.session_id
                || !saved.auto_login
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        })?;
        let online = self.online.snapshot()?;
        if self.revoked.load(Ordering::Acquire)
            || online.status != SessionStatus::Unknown
            || online.token.generation != self.generation
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}
