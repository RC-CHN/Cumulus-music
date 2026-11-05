//! Shared one-send guard for the recovered moderation endpoints.
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    comments::VinylWriteAuthority,
    error::code_matches,
    protocol::api::{Api, VinylCommentWrite, validate_response},
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const DRAFT_LIFETIME: Duration = Duration::from_secs(120);

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportCommand<R> {
    Inspect,
    Prepare { reason: R },
    Cancel { receipt: String },
    Submit { receipt: String },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Recorded {
    Uncertain,
    Accepted,
    Duplicate,
    OfficialAccount,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Ready,
    Uncertain,
    Accepted,
    Duplicate,
    OfficialAccount,
    NotSent,
    TargetUnavailable,
}

impl From<Recorded> for ReportStatus {
    fn from(value: Recorded) -> Self {
        match value {
            Recorded::Uncertain => Self::Uncertain,
            Recorded::Accepted => Self::Accepted,
            Recorded::Duplicate => Self::Duplicate,
            Recorded::OfficialAccount => Self::OfficialAccount,
        }
    }
}

#[derive(Serialize)]
pub struct ReportView {
    status: ReportStatus,
    receipt: Option<String>,
}

pub(super) fn custom_reason(description: String) -> Result<String> {
    fn js_whitespace(c: char) -> bool {
        matches!(c, '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' |
            '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' |
            '\u{3000}' | '\u{feff}')
    }
    if description.len() > 4096 || description.trim_matches(js_whitespace).is_empty() {
        return Err(Error::InvalidInput("invalid report description"));
    }
    // M55 uses trim only to enable Confirm. Send the original text.
    Ok(description)
}

struct Draft {
    token: SessionToken,
    key: String,
    reason: String,
    receipt: String,
    at: Instant,
}

#[derive(Default)]
pub(super) struct ReportHost(Mutex<Option<Draft>>);

impl ReportHost {
    fn draft(&self, token: &SessionToken) -> Result<std::sync::MutexGuard<'_, Option<Draft>>> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if slot
            .as_ref()
            .is_some_and(|draft| &draft.token != token || draft.at.elapsed() >= DRAFT_LIFETIME)
        {
            *slot = None;
        }
        Ok(slot)
    }

    pub(super) async fn act<R, F, Fut>(
        &self,
        journal: &ReportJournal<'_>,
        command: ReportCommand<R>,
        wire_reason: impl FnOnce(R) -> Result<String>,
        submit: F,
    ) -> Result<ReportView>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<ReportStatus>>,
    {
        let mut receipt = None;
        let status = match command {
            ReportCommand::Inspect => {
                let _lock = journal.lock()?;
                journal.existing()?.map_or(ReportStatus::Ready, Into::into)
            }
            ReportCommand::Prepare { reason } => {
                let reason = wire_reason(reason)?;
                journal.session.with_token(journal.token, |owner| {
                    if let Some(previous) = journal.read(owner)? {
                        return Ok(ReportStatus::from(previous));
                    }
                    let handle = uuid::Uuid::new_v4().simple().to_string();
                    *self.draft(journal.token)? = Some(Draft {
                        token: journal.token.clone(),
                        key: journal.draft_key().to_owned(),
                        reason,
                        receipt: handle.clone(),
                        at: Instant::now(),
                    });
                    receipt = Some(handle);
                    Ok(ReportStatus::Ready)
                })?
            }
            ReportCommand::Cancel { receipt } => {
                journal.session.with_token(journal.token, |owner| {
                    let mut slot = self.draft(journal.token)?;
                    if slot.as_ref().is_some_and(|draft| {
                        draft.key == journal.draft_key() && draft.receipt == receipt
                    }) {
                        *slot = None;
                    }
                    Ok(journal.read(owner)?.map_or(ReportStatus::Ready, Into::into))
                })?
            }
            ReportCommand::Submit { receipt } => {
                let _lock = journal.lock()?;
                if let Some(previous) = journal.existing()? {
                    previous.into()
                } else {
                    let draft = journal.session.with_token(journal.token, |_| {
                        let mut slot = self.draft(journal.token)?;
                        if !slot.as_ref().is_some_and(|draft| {
                            draft.key == journal.draft_key() && draft.receipt == receipt
                        }) {
                            return Err(Error::State("report confirmation expired"));
                        }
                        slot.take()
                            .ok_or(Error::State("report confirmation expired"))
                    })?;
                    submit(draft.reason).await?
                }
            }
        };
        Ok(ReportView { status, receipt })
    }
}

pub(super) struct ReportJournal<'a> {
    pub session: &'a AccountSession,
    pub token: &'a SessionToken,
    pub store: Arc<StateStore>,
    pub kind: &'static str,
    pub lock_prefix: &'static str,
    pub key: String,
    pub draft_scope: Option<String>,
}
impl ReportJournal<'_> {
    fn draft_key(&self) -> &str {
        self.draft_scope.as_deref().unwrap_or(&self.key)
    }
    fn lock(&self) -> Result<OperationLock> {
        self.session.with_token(self.token, |owner| {
            OperationLock::acquire(
                self.store.directory(),
                owner,
                &format!("{}:{}", self.lock_prefix, self.key),
            )
        })
    }
    fn read(&self, owner: &str) -> Result<Option<Recorded>> {
        self.store
            .get(owner, self.kind, &self.key)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }
    fn existing(&self) -> Result<Option<Recorded>> {
        self.session
            .with_token(self.token, |owner| self.read(owner))
    }
    pub(super) async fn send(
        &self,
        api: &Arc<dyn Api>,
        endpoint: &str,
        payload: Value,
        official_account_code: bool,
    ) -> Result<ReportStatus> {
        self.session.require_api(self.token, api)?;
        self.session.with_token(self.token, |owner| {
            self.store
                .put(owner, self.kind, &self.key, &json!(Recorded::Uncertain))
        })?;
        let accepted: &[i64] = if official_account_code {
            &[200, -1, -2]
        } else {
            &[200, -1]
        };
        let result = api.call(endpoint, payload, false, accepted).await;
        self.finish(result, endpoint, official_account_code, None)
    }

    pub(super) async fn send_vinyl(
        &self,
        write: VinylCommentWrite,
        authority: VinylWriteAuthority<'_>,
    ) -> Result<ReportStatus> {
        self.session.require_api(self.token, authority.api)?;
        let owner = self.session.with_token(self.token, |owner| {
            self.store
                .put(owner, self.kind, &self.key, &json!(Recorded::Uncertain))?;
            Ok(owner.to_owned())
        })?;
        let endpoint = write.path();
        let result = authority
            .api
            .request_vinyl_comment_write(write, authority.revision, authority.permission)
            .await
            .and_then(|response| validate_response(endpoint, response, &[200, -1]));
        // The target lock outlives dispatch. A change of account must neither
        // erase a sent result nor leave a provably unsent intent stuck.
        self.finish(result, endpoint, false, Some(&owner))
    }

    fn finish(
        &self,
        result: Result<Value>,
        endpoint: &str,
        official_account_code: bool,
        original_owner: Option<&str>,
    ) -> Result<ReportStatus> {
        let observed = match result {
            Ok(value) if code_matches(&value["code"], 200) => Recorded::Accepted,
            Ok(value) if code_matches(&value["code"], -1) => Recorded::Duplicate,
            Ok(value) if official_account_code && code_matches(&value["code"], -2) => {
                Recorded::OfficialAccount
            }
            Err(error) => match error.root() {
                Error::RequestNotSent(_) => {
                    self.finish_for(original_owner, |owner| {
                        self.store.delete(owner, self.kind, &self.key)
                    })?;
                    return Ok(ReportStatus::NotSent);
                }
                Error::CredentialChanged | Error::LoginDeferred | Error::StaleResponse => {
                    return Err(error);
                }
                _ if error.is_service_code(301) => return Err(error),
                Error::Service {
                    endpoint: reported, ..
                }
                | Error::HttpStatus {
                    endpoint: Some(reported),
                    ..
                } if reported == endpoint => {
                    if error.is_service_code(-1) {
                        Recorded::Duplicate
                    } else if official_account_code && error.is_service_code(-2) {
                        Recorded::OfficialAccount
                    } else {
                        Recorded::Uncertain
                    }
                }
                _ => Recorded::Uncertain,
            },
            _ => Recorded::Uncertain,
        };
        // Persist only this bounded status, without reason, target text or credentials.
        self.finish_for(original_owner, |owner| {
            self.store
                .put(owner, self.kind, &self.key, &json!(observed))
        })?;
        Ok(observed.into())
    }

    fn finish_for(
        &self,
        original_owner: Option<&str>,
        operation: impl FnOnce(&str) -> Result<()>,
    ) -> Result<()> {
        if let Some(owner) = original_owner {
            operation(owner)
        } else {
            self.session.with_token(self.token, operation)
        }
    }
}
