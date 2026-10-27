//! Account-scoped effect reads and resumable parameter-material acquisition.
//! Catalogues describe products; only a fresh positive auth response permits
//! this service to fetch or return a material. Trial occupancy is a separate
//! activation lifecycle and is not silently started here.
mod files;
mod materials;
mod records;

use super::{
    api::EffectsApi,
    catalog::{CatalogueKind, Sound},
};
use crate::{
    Result,
    account::{AccountSession, SessionToken},
    download::transfer::{MediaBackend, ReqwestMediaBackend, TransferManager},
    storage::{OperationLock, StateStore},
};
pub use records::{MaterialHandle, MaterialOutcome, MaterialPhase, VerifiedMaterial};
use std::{sync::Arc, time::Duration};
const CATALOGUES: &str = "effects-catalogues-v1";
const MATERIALS: &str = "effects-materials-v1";
const LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct EffectsLibrary {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    backend: Arc<dyn MediaBackend>,
}
impl EffectsLibrary {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_backend(
            session,
            Arc::new(ReqwestMediaBackend::new(Duration::from_secs(30))?),
        )
    }
    pub fn with_backend(
        session: Arc<AccountSession>,
        backend: Arc<dyn MediaBackend>,
    ) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
            backend,
        })
    }
    /// Explicit refresh. Failed/incomplete responses leave the last complete
    /// snapshot untouched; callers can deliberately request that cache later.
    pub async fn catalogue_for(
        &self,
        token: &SessionToken,
        kind: CatalogueKind,
        refresh: bool,
    ) -> Result<Vec<Sound>> {
        let key = kind.expose_type().to_string();
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner, &format!("catalogue:{key}"))?;
        if !refresh
            && let Some(value) = self
                .session
                .with_token(token, |owner| self.store.get(owner, CATALOGUES, &key))?
        {
            // Store official rows, then revalidate identity and material fields
            // at their use sites; no unchecked deserialized Sound is trusted.
            return super::api::sounds(value);
        }
        let sounds = self
            .session
            .run_for(token, |api, _| async move {
                EffectsApi::new(api).catalogue(kind).await
            })
            .await?;
        self.session.with_token(token, |owner| {
            self.store.put(
                owner,
                CATALOGUES,
                &key,
                &serde_json::json!(sounds.iter().map(Sound::native_record).collect::<Vec<_>>()),
            )
        })?;
        Ok(sounds)
    }
    fn owner(&self, token: &SessionToken) -> Result<String> {
        self.session.with_token(token, |owner| Ok(owner.to_owned()))
    }
    fn lock(&self, owner: &str, key: &str) -> Result<OperationLock> {
        OperationLock::acquire(
            self.store.directory(),
            "effects-service",
            &serde_json::to_string(&(owner, key))?,
        )
    }
    fn transfers(&self, owner: &str) -> Result<TransferManager> {
        TransferManager::with_backend(
            self.store.clone(),
            owner.to_owned(),
            self.backend.clone(),
            LIMIT,
        )
    }
}
