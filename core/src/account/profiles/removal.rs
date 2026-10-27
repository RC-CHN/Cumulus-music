//! Local saved-session removal. A durable intent blocks adoption before the
//! credential store changes; retry finishes cleanup after either database fails.
use super::*;
use crate::storage::{OperationLock, StateTransaction};

const REMOVALS: &str = "saved-profile-removals-v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Removal {
    uid: String,
    session_id: String,
    complete: bool,
}

pub(super) fn available(tx: &StateTransaction<'_>, session_id: &str) -> Result<()> {
    if tx.get(OWNER, REMOVALS, session_id)?.is_some() {
        return Err(Error::State("saved account removal has started"));
    }
    Ok(())
}

impl ProfileRegistry {
    pub fn removal_pending(&self, session_id: &str) -> Result<bool> {
        self.store
            .get(OWNER, REMOVALS, session_id)?
            .map(|v| Ok(!serde_json::from_value::<Removal>(v)?.complete))
            .unwrap_or(Ok(false))
    }

    /// Removes exactly this saved session, not remote accounts or account data.
    /// Completed intents remain to prevent late candidates resurrecting it.
    pub fn remove_saved(
        &self,
        session: &AccountSession,
        expected: &SessionToken,
        session_id: &str,
    ) -> Result<()> {
        if !id(session_id) {
            return Err(Error::InvalidInput("invalid saved account identifier"));
        }
        let _guard = OperationLock::acquire(&self.root, REMOVALS, session_id)?;
        let api: Arc<dyn std::any::Any + Send + Sync> = session.native_api(expected)?;
        let bound = api.downcast::<MusicHttpClient>().ok();
        // No network awaits. Holding the generation guard across local work
        // prevents this removal racing an account adoption in this session.
        session.with_local_owner(expected, |_| {
            let mut intent: Removal = self.store.transaction(|tx| {
                if let Some(value) = tx.get(OWNER, REMOVALS, session_id)? {
                    let intent: Removal = serde_json::from_value(value)?;
                    if intent.session_id != session_id {
                        return Err(Error::State("invalid saved account removal"));
                    }
                    if !intent.complete && expected.uid.as_deref() == Some(&intent.uid) {
                        return Err(Error::State("cannot remove current account"));
                    }
                    return Ok(intent);
                }
                let record = tx
                    .values(OWNER, saved::KIND)?
                    .into_values()
                    .map(serde_json::from_value::<SavedProfile>)
                    .collect::<std::result::Result<Vec<_>, _>>()?
                    .into_iter()
                    .find(|r| r.session_id == session_id)
                    .ok_or(Error::State("saved account is no longer available"))?;
                if expected.uid.as_deref() == Some(&record.uid) {
                    return Err(Error::State("cannot remove current account"));
                }
                let intent = Removal {
                    uid: record.uid,
                    session_id: session_id.into(),
                    complete: false,
                };
                if let Some(value) = tx.get(OWNER, KIND, "active")? {
                    let selected: Selection = serde_json::from_value(value)?;
                    if selected.session_id == session_id {
                        tx.delete(OWNER, KIND, "active")?;
                    }
                }
                tx.put(OWNER, REMOVALS, session_id, &serde_json::to_value(&intent)?)?;
                Ok(intent)
            })?;
            if intent.complete {
                return Ok(());
            }
            match self.profile_path(session_id) {
                Ok(path) => {
                    if let Some(client) = bound
                        .as_ref()
                        .filter(|c| c.credentials().store().directory() == path)
                    {
                        // A signed-out/expired bound profile must update its
                        // existing object, otherwise account_snapshot is stale.
                        client.credentials().clear()?;
                    } else {
                        // Deleting the record also handles malformed credentials;
                        // outstanding HTTP responses fail their persisted CAS.
                        StateStore::open(path)?.delete(OWNER, "credentials", "session")?;
                    }
                }
                Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
            self.store.transaction(|tx| {
                if let Some(value) = tx.get(OWNER, saved::KIND, &intent.uid)? {
                    let record: SavedProfile = serde_json::from_value(value)?;
                    if record.session_id == session_id {
                        tx.delete(OWNER, saved::KIND, &intent.uid)?;
                    }
                }
                intent.complete = true;
                tx.put(OWNER, REMOVALS, session_id, &serde_json::to_value(&intent)?)
            })
        })
    }
}
