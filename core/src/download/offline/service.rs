use super::{OfflineCheck, OfflineRecord, PublishedDownload};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    media::{
        integrity::inspect_file,
        ncm::{HeaderPolicy, NcmReader},
    },
    membership::{
        MembershipManager, MembershipSnapshot, OfflineAccess, OfflineSource,
        clock::{MembershipClock, SystemMembershipClock},
        offline_access,
    },
    storage::{OperationLock, StateStore, StateTransaction},
};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
const KIND: &str = "offline_music_v1";

pub struct OfflineLibrary {
    pub(super) session: Arc<AccountSession>,
    store: Arc<StateStore>,
    pub(super) membership: MembershipManager,
}
impl OfflineLibrary {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Self::with_clock(session, Arc::new(SystemMembershipClock::default()))
    }
    pub fn with_clock(
        session: Arc<AccountSession>,
        clock: Arc<dyn MembershipClock>,
    ) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            membership: MembershipManager::with_clock(session.clone(), clock)?,
            session,
        })
    }
    pub async fn refresh_membership(&self, token: &SessionToken) -> Result<MembershipSnapshot> {
        self.membership.refresh_for(token.clone()).await
    }
    fn lock(&self, token: &SessionToken, id: &str) -> Result<OperationLock> {
        let uid = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        OperationLock::acquire(
            self.store.directory(),
            "offline-record",
            &serde_json::to_string(&(uid, id))?,
        )
    }
    fn read(&self, token: &SessionToken, id: &str) -> Result<Option<OfflineRecord>> {
        self.session.with_token(token, |uid| {
            self.store
                .get(uid, KIND, id)?
                .map(serde_json::from_value)
                .transpose()
                .map_err(Into::into)
        })
    }
    /// Blocking file verification; no long IO is performed under account or DB locks.
    pub fn register(
        &self,
        token: &SessionToken,
        task: &PublishedDownload,
    ) -> Result<OfflineRecord> {
        self.register_with(token, task, |_, record| Ok(record.clone()))
    }
    /// The publisher can commit its task journal in the same transaction as
    /// the offline record. Verification stays outside the account/DB locks.
    pub(crate) fn register_with<T>(
        &self,
        token: &SessionToken,
        task: &PublishedDownload,
        commit: impl FnOnce(&mut StateTransaction<'_>, &OfflineRecord) -> Result<T>,
    ) -> Result<T> {
        task.validate()?;
        let path = task.target.canonicalize()?;
        let id = path_id(&path)?;
        let _lock = self.lock(token, &id)?;
        let record = task.record(path, id)?;
        verify(&record)?;
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                if let Some(existing) = tx.get(uid, KIND, &record.id)? {
                    let old: OfflineRecord = serde_json::from_value(existing)?;
                    if old.path == record.path
                        && old.task_id == record.task_id
                        && old.song_id == record.song_id
                        && old.quality == record.quality
                        && old.privilege == record.privilege
                        && old.encrypted == record.encrypted
                        && old.integrity == record.integrity
                    {
                        return commit(tx, &old);
                    }
                    return Err(Error::State(
                        "offline path already belongs to another publication",
                    ));
                }
                tx.put(uid, KIND, &record.id, &serde_json::to_value(&record)?)?;
                commit(tx, &record)
            })
        })
    }
    /// Includes entries whose files have since disappeared, for UI/recovery.
    pub fn records(&self, token: &SessionToken) -> Result<Vec<OfflineRecord>> {
        self.session.with_token(token, |uid| {
            self.store
                .values(uid, KIND)?
                .into_values()
                .map(|v| serde_json::from_value(v).map_err(Into::into))
                .collect()
        })
    }
    pub fn record(
        &self,
        token: &SessionToken,
        path: impl AsRef<Path>,
    ) -> Result<Option<OfflineRecord>> {
        self.read(token, &path_id(&path.as_ref().canonicalize()?)?)
    }
    /// Removes only the exact ledger revision. Never deletes the media file.
    pub fn forget(&self, token: &SessionToken, record: &OfflineRecord) -> Result<()> {
        let _lock = self.lock(token, &record.id)?;
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                if tx.get(uid, KIND, &record.id)? != Some(serde_json::to_value(record)?) {
                    return Err(Error::State("offline record changed"));
                }
                tx.delete(uid, KIND, &record.id)
            })
        })
    }
    /// Explicit `renew` contacts the membership endpoint only for encrypted
    /// music. Plain media still requires its account record and file integrity.
    pub async fn check(
        &self,
        token: &SessionToken,
        path: impl AsRef<Path>,
        renew: bool,
    ) -> Result<OfflineCheck> {
        let path = path.as_ref().canonicalize()?;
        let id = path_id(&path)?;
        let _lock = self.lock(token, &id)?;
        let record = self
            .read(token, &id)?
            .ok_or(Error::State("no offline record for this account"))?;
        if record.id != id || record.path != path {
            return Err(Error::State("offline record identity mismatch"));
        }
        let access = if record.encrypted {
            let snapshot = if renew {
                self.membership.refresh_for(token.clone()).await?
            } else {
                self.membership.snapshot()?
            };
            self.session.with_token(token, |_| Ok(()))?;
            offline_access(
                Some(&record.privilege),
                record.quality,
                &snapshot.state,
                OfflineSource::Downloaded { complete: true },
                true,
            )?
        } else {
            OfflineAccess {
                can_play: true,
                reason: None,
            }
        };
        if !access.can_play {
            self.unchanged(token, &record)?;
            return Ok(OfflineCheck {
                access,
                record,
                integrity_verified: false,
            });
        }
        let input = record.clone();
        tokio::task::spawn_blocking(move || verify(&input))
            .await
            .map_err(|_| Error::State("offline integrity worker stopped"))??;
        // A long hash must not return an entitlement that expired while IO
        // was running. Re-read the monotonic view without another request.
        let access = if record.encrypted {
            offline_access(
                Some(&record.privilege),
                record.quality,
                &self.membership.snapshot()?.state,
                OfflineSource::Downloaded { complete: true },
                true,
            )?
        } else {
            access
        };
        self.unchanged(token, &record)?;
        Ok(OfflineCheck {
            access,
            record,
            integrity_verified: true,
        })
    }
    fn unchanged(&self, token: &SessionToken, record: &OfflineRecord) -> Result<()> {
        if self.read(token, &record.id)? != Some(record.clone()) {
            return Err(Error::State("offline record changed during check"));
        }
        Ok(())
    }
}
fn path_id(path: &Path) -> Result<String> {
    Ok(hex::encode(Sha256::digest(
        path.to_str()
            .ok_or(Error::InvalidInput("offline path requires lossless UTF-8"))?
            .as_bytes(),
    )))
}
pub(super) fn verify(record: &OfflineRecord) -> Result<()> {
    record.integrity.validate()?;
    if inspect_file(&record.path)? != record.integrity {
        return Err(Error::Media(
            "offline file differs from published integrity",
        ));
    }
    if record.encrypted
        && NcmReader::open(&record.path, HeaderPolicy::Strict)?
            .info()
            .audio_format
            .is_none()
    {
        return Err(Error::Media("unrecognized encrypted offline audio"));
    }
    Ok(())
}
