//! Account-bound link-import preparation and durable one-shot dispatch.
use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    account::SessionToken,
    playlist::{ExternalPlaylistInput, external_import as wire},
    protocol::http::{HttpBackend, ReqwestBackend},
    storage::StateStore,
};
use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

const JOURNAL: &str = "external-playlist-import-v1";
const LATEST: &str = "latest";

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExternalImportAction {
    Inspect,
    Prepare { inputs: Vec<String> },
    Cancel { receipt: String },
    Submit { receipt: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalImportView {
    authorized: bool,
    outcome: ExternalImportOutcome,
    prepared: Option<ExternalImportPreview>,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum ExternalImportOutcome {
    Ready,
    Unknown { operation: String },
    Accepted { operation: String },
}

#[derive(Serialize)]
struct ExternalImportPreview {
    receipt: String,
    name: String,
    links: Vec<ExternalPlaylistInput>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    operation: String,
    phase: Phase,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Phase {
    Unknown,
    Accepted { task_id: String },
}

impl Record {
    fn validate(self) -> Result<Self> {
        if !valid_receipt(&self.operation)
            || matches!(&self.phase, Phase::Accepted { task_id } if !wire::valid_task_id(task_id))
        {
            return Err(Error::State("invalid external import journal"));
        }
        Ok(self)
    }

    fn outcome(self) -> ExternalImportOutcome {
        match self.phase {
            Phase::Unknown => ExternalImportOutcome::Unknown {
                operation: self.operation,
            },
            Phase::Accepted { .. } => ExternalImportOutcome::Accepted {
                operation: self.operation,
            },
        }
    }
}

fn valid_receipt(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

fn parse(raw: Option<serde_json::Value>) -> Result<Option<Record>> {
    raw.map(|raw| serde_json::from_value::<Record>(raw)?.validate())
        .transpose()
}

fn begin(store: &StateStore, uid: &str, operation: &str) -> Result<Option<Record>> {
    if !valid_receipt(operation) {
        return Err(Error::InvalidInput("invalid external import operation"));
    }
    store.transaction(|tx| {
        let previous = parse(tx.get(uid, JOURNAL, LATEST)?)?;
        if matches!(
            &previous,
            Some(Record {
                phase: Phase::Unknown,
                ..
            })
        ) {
            return Err(Error::State("previous external import remains unconfirmed"));
        }
        tx.put(
            uid,
            JOURNAL,
            LATEST,
            &serde_json::to_value(Record {
                operation: operation.into(),
                phase: Phase::Unknown,
            })?,
        )?;
        Ok(previous)
    })
}

fn abandon_unsent(
    store: &StateStore,
    uid: &str,
    operation: &str,
    previous: Option<Record>,
) -> Result<()> {
    store.transaction(|tx| match parse(tx.get(uid, JOURNAL, LATEST)?)? {
        Some(Record {
            operation: current,
            phase: Phase::Unknown,
        }) if current == operation => {
            if let Some(previous) = previous {
                tx.put(uid, JOURNAL, LATEST, &serde_json::to_value(previous)?)
            } else {
                tx.delete(uid, JOURNAL, LATEST)
            }
        }
        _ => Err(Error::State("external import journal no longer matches")),
    })
}

fn settle(store: &StateStore, uid: &str, operation: &str, task_id: String) -> Result<()> {
    let accepted = Record {
        operation: operation.into(),
        phase: Phase::Accepted { task_id },
    }
    .validate()?;
    store.transaction(|tx| match parse(tx.get(uid, JOURNAL, LATEST)?)? {
        Some(Record {
            operation: current,
            phase: Phase::Unknown,
        }) if current == operation => {
            tx.put(uid, JOURNAL, LATEST, &serde_json::to_value(accepted)?)
        }
        _ => Err(Error::State("external import journal no longer matches")),
    })
}

struct Draft {
    token: SessionToken,
    name: String,
    receipt: String,
    batch: wire::LinkBatch,
}

#[derive(Default)]
pub(super) struct ExternalImportHost {
    draft: Mutex<Option<Draft>>,
    backend: Option<Arc<dyn HttpBackend>>,
}

impl ExternalImportHost {
    fn matching(&self, token: &SessionToken) -> Result<std::sync::MutexGuard<'_, Option<Draft>>> {
        let mut slot = self.draft.lock().map_err(|_| Error::LockPoisoned)?;
        if slot.as_ref().is_some_and(|draft| draft.token != *token) {
            *slot = None;
        }
        Ok(slot)
    }
}

fn default_name() -> String {
    let now = Local::now();
    format!(
        "导入音乐 {}/{}/{} {:02}:{:02}:{:02}",
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

impl MusicClient {
    /// Native fixture injection. No IPC can replace the fixed destination,
    /// headers, scope, body encoder, or account verification.
    pub fn with_external_import_backend(mut self, backend: Arc<dyn HttpBackend>) -> Self {
        self.external_import.backend = Some(backend);
        self
    }

    pub async fn external_import_command(
        &self,
        epoch: &str,
        action: ExternalImportAction,
    ) -> Result<AccountData<ExternalImportView>> {
        let token = self.playlist_token(epoch)?;
        let store = self.registry.store();
        let outcome = |uid: &str| -> Result<ExternalImportOutcome> {
            Ok(parse(store.get(uid, JOURNAL, LATEST)?)?
                .map_or(ExternalImportOutcome::Ready, Record::outcome))
        };
        let mut prepared = None;
        match action {
            ExternalImportAction::Inspect => {
                self.session.with_token(&token, |_| {
                    drop(self.external_import.matching(&token)?);
                    Ok(())
                })?;
            }
            ExternalImportAction::Prepare { inputs } => {
                if self.external_import_credentials(&token)?.is_none() {
                    return Err(Error::State("authorize the external import session first"));
                }
                let mut batch = wire::batch(&inputs)?;
                let name = default_name();
                let receipt = Uuid::new_v4().simple().to_string();
                self.session.with_token(&token, |uid| {
                    if matches!(outcome(uid)?, ExternalImportOutcome::Unknown { .. }) {
                        return Err(Error::State("previous external import remains unconfirmed"));
                    }
                    prepared = Some(ExternalImportPreview {
                        receipt: receipt.clone(),
                        name: name.clone(),
                        links: std::mem::take(&mut batch.preview),
                    });
                    *self.external_import.matching(&token)? = Some(Draft {
                        token: token.clone(),
                        name,
                        receipt,
                        batch,
                    });
                    Ok(())
                })?;
            }
            ExternalImportAction::Cancel { receipt } => {
                self.session.with_token(&token, |_| {
                    let mut slot = self.external_import.matching(&token)?;
                    if slot.as_ref().is_some_and(|draft| draft.receipt == receipt) {
                        *slot = None;
                    }
                    Ok(())
                })?;
            }
            ExternalImportAction::Submit { receipt } => {
                let credentials = self
                    .external_import_credentials(&token)?
                    .ok_or(Error::State("authorize the external import session first"))?;
                let backend: Arc<dyn HttpBackend> = match &self.external_import.backend {
                    Some(backend) => backend.clone(),
                    None => Arc::new(ReqwestBackend::new(Duration::from_secs(20))?),
                };
                let operation = Uuid::new_v4().simple().to_string();
                let (plan, uid, previous) = self.session.with_token(&token, |uid| {
                    let mut slot = self.external_import.matching(&token)?;
                    let draft = slot
                        .as_ref()
                        .filter(|draft| draft.receipt == receipt)
                        .ok_or(Error::State("external import confirmation expired"))?;
                    let plan = wire::request(&credentials, &draft.batch, &draft.name)?;
                    let previous = begin(&store, uid, &operation)?;
                    *slot = None;
                    Ok((plan, uid.to_owned(), previous))
                })?;
                let preflight = (|| {
                    let current = self
                        .external_import_credentials(&token)?
                        .filter(|current| Arc::ptr_eq(current, &credentials))
                        .ok_or(Error::CredentialChanged)?;
                    plan.revalidate(&current)?;
                    self.session.check_generation(&token)
                })();
                if let Err(error) = preflight {
                    // No backend call has occurred: restore only the exact
                    // superseded local receipt. A crash still leaves Unknown.
                    abandon_unsent(&store, &uid, &operation, previous)?;
                    return Err(error);
                }
                // An explicitly dispatched operation belongs to the verified
                // original owner, even if the UI has since changed accounts.
                // The final view is still fenced to the initiating generation.
                if let Ok(reply) = backend.post(plan.plan).await
                    && let Some(task_id) = wire::acknowledgement(&reply)
                {
                    settle(&store, &uid, &operation, task_id)?;
                }
            }
        }
        let authorized = self.external_import_credentials(&token)?.is_some();
        self.session.with_token(&token, |uid| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: ExternalImportView {
                    authorized,
                    outcome: outcome(uid)?,
                    prepared,
                },
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_intent_survives_reopen_and_is_never_displaced_by_another_attempt() {
        let root = tempfile::tempdir().unwrap();
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let other = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        {
            let store = StateStore::open(root.path()).unwrap();
            begin(&store, "42", id).unwrap();
            assert!(begin(&store, "42", other).is_err());
            begin(&store, "43", other).unwrap();
        }
        let store = StateStore::open(root.path()).unwrap();
        assert!(begin(&store, "42", other).is_err());
        assert!(settle(&store, "42", other, "task-99".into()).is_err());
        settle(&store, "42", id, "task-42".into()).unwrap();
        let accepted = parse(store.get("42", JOURNAL, LATEST).unwrap())
            .unwrap()
            .unwrap();
        let visible = serde_json::to_string(&accepted.outcome()).unwrap();
        assert!(visible.contains("accepted"));
        assert!(!visible.contains("task-42"));
        let previous = begin(&store, "42", other).unwrap();
        assert!(abandon_unsent(&store, "42", id, None).is_err());
        abandon_unsent(&store, "42", other, previous).unwrap();
        let restored = parse(store.get("42", JOURNAL, LATEST).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(restored.operation, id);
        assert!(matches!(restored.phase, Phase::Accepted { .. }));
        assert!(matches!(
            parse(store.get("43", JOURNAL, LATEST).unwrap())
                .unwrap()
                .unwrap()
                .phase,
            Phase::Unknown
        ));
    }
}
