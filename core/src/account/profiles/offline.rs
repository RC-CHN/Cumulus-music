//! Explicit native authorization for previously verified local resources.
//! This does not authenticate AccountSession or authorize any online request.
use super::*;
use crate::{
    account::SessionStatus,
    membership::{ColdEntitlement, ColdMembership},
};

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
    cursor_id: Uuid,
    credentials: Arc<Credentials>,
    binding: Binding,
    revoked: AtomicBool,
    membership: ColdMembership,
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
            cursor_id: Uuid::new_v4(),
            credentials,
            binding,
            revoked: AtomicBool::new(false),
            membership: ColdMembership::default(),
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

    /// Bounded read-only page. No filesystem hash is performed at listing;
    /// the open path repeats eligibility checks and verifies the bytes.
    pub fn list_plain_audio(
        &self,
        after: Option<&crate::download::offline::ColdOfflineCursor>,
        limit: usize,
    ) -> Result<crate::download::offline::ColdOfflineAudioPage> {
        self.list_downloads(after, limit, None)
    }

    pub fn list_audio(
        &self,
        after: Option<&crate::download::offline::ColdOfflineCursor>,
        limit: usize,
    ) -> Result<crate::download::offline::ColdOfflineAudioPage> {
        self.list_downloads(after, limit, Some(&self.membership))
    }

    fn list_downloads(
        &self,
        after: Option<&crate::download::offline::ColdOfflineCursor>,
        limit: usize,
        membership: Option<&ColdMembership>,
    ) -> Result<crate::download::offline::ColdOfflineAudioPage> {
        self.require()?;
        let page = crate::download::offline::list_audio(
            self.cursor_id,
            &self.registry.store,
            &self.binding.uid,
            after,
            limit,
            membership,
        )?;
        self.require()?;
        Ok(page)
    }

    /// Blocking IO. The caller passes a published task ID, never an owner or path.
    pub fn open_plain_audio(
        &self,
        task_id: &str,
    ) -> Result<crate::download::offline::ColdOfflineAudio<'_>> {
        self.open_download(task_id, None)
    }

    pub fn open_audio(
        &self,
        task_id: &str,
    ) -> Result<crate::download::offline::ColdOfflineAudio<'_>> {
        self.open_download(task_id, Some(&self.membership))
    }

    fn open_download(
        &self,
        task_id: &str,
        membership: Option<&ColdMembership>,
    ) -> Result<crate::download::offline::ColdOfflineAudio<'_>> {
        self.require()?;
        let (file, quality, entitlement) = crate::download::offline::open_audio(
            &self.registry.store,
            &self.binding.uid,
            task_id,
            membership,
        )?;
        self.require()?;
        Ok(crate::download::offline::ColdOfflineAudio::new(
            self,
            file,
            quality,
            entitlement,
        ))
    }

    pub(crate) fn validate_entitlement(&self, entitlement: &ColdEntitlement) -> Result<()> {
        entitlement.validate(&self.registry.store, &self.binding.uid)
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
