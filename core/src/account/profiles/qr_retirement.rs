//! Only the ordinary QR constructor issues this ownership capability.
use super::*;
use crate::storage::QrCandidateStore;

pub(super) struct QrCandidateRetirement {
    cancelled: AtomicBool,
}

impl QrCandidateRetirement {
    pub(super) fn track(
        registry: Arc<ProfileRegistry>,
        id: String,
        path: &Path,
    ) -> Result<(StateStore, Arc<Self>)> {
        let nonce = Uuid::new_v4().simple().to_string();
        QrCandidateStore::create_marker(path, &nonce)?;
        let mut store = StateStore::open(path)?;
        let owner = Arc::new(Self {
            cancelled: AtomicBool::new(false),
        });
        let after_close = owner.clone();
        let runtime = tokio::runtime::Handle::try_current().ok();
        store.on_qr_candidate_close(move || {
            if after_close.cancelled.load(Ordering::Acquire)
                && let Some(runtime) = runtime
            {
                runtime.spawn_blocking(move || {
                    // Best effort; any doubt retains the credential files.
                    let _ = registry.retire_cancelled_qr(&id, &nonce);
                });
            }
        });
        Ok((store, owner))
    }
}

impl ProfileRegistry {
    fn retire_cancelled_qr(&self, candidate_id: &str, nonce: &str) -> Result<()> {
        let path = self.profile_path(candidate_id)?;
        // Keep the registry's writer fence while verifying that this exact
        // handle is unregistered and publishing its local tombstone.
        self.store.transaction(|tx| {
            let selected = tx
                .get(OWNER, KIND, "active")?
                .map(serde_json::from_value::<Selection>)
                .transpose()?;
            if selected.as_ref().is_some_and(|row| {
                !id(&row.session_id) || !id(&row.revision) || row.session_id == candidate_id
            }) {
                return Err(Error::StaleResponse);
            }
            for (uid, row) in tx.values(OWNER, saved::KIND)? {
                let record: SavedProfile = serde_json::from_value(row)?;
                if !id(&record.session_id) || record.uid != uid || record.session_id == candidate_id
                {
                    return Err(Error::StaleResponse);
                }
            }
            if registration::references_candidate(tx, candidate_id)? {
                return Err(Error::StaleResponse);
            }
            if fs::canonicalize(&path)? != path {
                return Err(Error::StaleResponse);
            }
            QrCandidateStore::retire(&path, nonce)
        })
    }
}

impl LoginCandidate {
    pub(crate) fn mark_qr_cancelled(&self) {
        if let Some(owner) = &self.qr_retirement {
            owner.cancelled.store(true, Ordering::Release);
        }
    }

    pub(super) fn check_qr_not_cancelled(&self) -> Result<()> {
        if self
            .qr_retirement
            .as_ref()
            .is_some_and(|owner| owner.cancelled.load(Ordering::Acquire))
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_retirement_refuses_independent_store_and_unresolved_registration() {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let sessions = registry.root.join("sessions");
        private_directory(&sessions).unwrap();
        let id = Uuid::new_v4().simple().to_string();
        let directory = sessions.join(&id);
        private_directory(&directory).unwrap();
        let nonce = Uuid::new_v4().simple().to_string();
        QrCandidateStore::create_marker(&directory, &nonce).unwrap();
        let db = directory.join("core.sqlite3");
        let independent = StateStore::open(&directory).unwrap();
        assert!(matches!(
            registry.retire_cancelled_qr(&id, &nonce),
            Err(Error::MutationBusy)
        ));
        assert!(db.is_file());
        drop(independent);

        let session = Arc::new(StateStore::open(&directory).unwrap());
        let credentials = Arc::new(Credentials::open(session, ApiOrigin::Pc).unwrap());
        let transport =
            Arc::new(MusicHttpClient::new(credentials.clone(), ClientProfile::default()).unwrap());
        // This value is synthetic and deliberately protects the exact directory
        // being evaluated, even though ordinary QR never enters phone registration.
        let candidate = LoginCandidate {
            registry: registry.clone(),
            id: id.clone(),
            expected: None,
            client: transport,
            credentials,
            expected_uid: None,
            quick_target: None,
            auto_login: AtomicBool::new(true),
            registration_phone: Mutex::new(None),
            qr_retirement: None,
        };
        candidate
            .bind_registration_phone(Some(("13800138000", "86")))
            .unwrap();
        let _intent = candidate.registration_begin().unwrap();
        drop(candidate);
        assert!(matches!(
            registry.retire_cancelled_qr(&id, &nonce),
            Err(Error::StaleResponse)
        ));
        assert!(db.is_file());

        let candidate = registry.candidate(ClientProfile::default()).unwrap();
        let unrelated = registry.profile_path(&candidate.id).unwrap();
        assert!(unrelated.join("core.sqlite3").is_file());
        assert!(matches!(
            registry.retire_cancelled_qr(&candidate.id, &nonce),
            Err(Error::StaleResponse)
        ));
        assert!(unrelated.join("core.sqlite3").is_file());
    }

    #[test]
    fn qr_retirement_does_not_delete_any_file_when_the_owned_directory_has_unknown_contents() {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let sessions = registry.root.join("sessions");
        private_directory(&sessions).unwrap();
        let id = Uuid::new_v4().simple().to_string();
        let directory = sessions.join(&id);
        private_directory(&directory).unwrap();
        let nonce = Uuid::new_v4().simple().to_string();
        QrCandidateStore::create_marker(&directory, &nonce).unwrap();
        drop(StateStore::open(&directory).unwrap());
        let unknown = directory.join("unrelated");
        fs::write(&unknown, b"synthetic").unwrap();

        assert!(registry.retire_cancelled_qr(&id, &nonce).is_err());
        assert_eq!(fs::read(unknown).unwrap(), b"synthetic");
        assert!(directory.join("core.sqlite3").is_file());
        assert!(StateStore::open(&directory).is_ok());
    }
}
