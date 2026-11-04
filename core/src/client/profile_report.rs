//! M622 and M55: a report from an authenticated guest user's PC profile.
//! The moderation endpoint has no recovered status lookup or inverse.
use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    account::SessionToken,
    error::code_matches,
    protocol::{api::identifier, wire::serialization::js_truthy},
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;

const ENDPOINT: &str = "/api/report/reportuser";
const JOURNAL: &str = "profile-report-receipt-v1";
const MAX_REASON_BYTES: usize = 4096;
const DRAFT_LIFETIME: Duration = Duration::from_secs(120);

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileReportReason {
    Advertising,
    PersonalAttack,
    HarmfulContent,
    Political,
    Other { description: String },
}

fn js_whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' |
        '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' |
        '\u{3000}' | '\u{feff}')
}

impl ProfileReportReason {
    fn into_wire(self) -> Result<String> {
        Ok(match self {
            Self::Advertising => "广告或营销信息".into(),
            Self::PersonalAttack => "恶意人身攻击".into(),
            Self::HarmfulContent => "色情、低俗或有害内容".into(),
            Self::Political => "政治敏感言论".into(),
            Self::Other { description }
                if description.len() <= MAX_REASON_BYTES
                    && !description.trim_matches(js_whitespace).is_empty() =>
            {
                // M55 uses trim only to enable Confirm. Send the original text.
                description
            }
            Self::Other { .. } => return Err(Error::InvalidInput("invalid report description")),
        })
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileReportCommand {
    Inspect,
    Prepare { reason: ProfileReportReason },
    Cancel { receipt: String },
    Submit { receipt: String },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalStatus {
    Uncertain,
    Accepted,
    Duplicate,
    OfficialAccount,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReportStatus {
    Ready,
    Uncertain,
    Accepted,
    Duplicate,
    OfficialAccount,
    NotSent,
    TargetUnavailable,
}

impl From<JournalStatus> for ReportStatus {
    fn from(value: JournalStatus) -> Self {
        match value {
            JournalStatus::Uncertain => Self::Uncertain,
            JournalStatus::Accepted => Self::Accepted,
            JournalStatus::Duplicate => Self::Duplicate,
            JournalStatus::OfficialAccount => Self::OfficialAccount,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReportView {
    status: ReportStatus,
    receipt: Option<String>,
}

struct Draft {
    token: SessionToken,
    uid: String,
    reason: String,
    receipt: String,
    at: Instant,
}

impl Draft {
    fn matches(&self, token: &SessionToken, uid: &str, receipt: &str) -> bool {
        &self.token == token
            && self.uid == uid
            && self.receipt == receipt
            && self.at.elapsed() < DRAFT_LIFETIME
    }
}

#[derive(Default)]
pub(super) struct ProfileReportHost(Mutex<Option<Draft>>);

impl ProfileReportHost {
    fn for_token<T>(
        &self,
        token: &SessionToken,
        f: impl FnOnce(&mut Option<Draft>) -> Result<T>,
    ) -> Result<T> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if slot
            .as_ref()
            .is_some_and(|draft| &draft.token != token || draft.at.elapsed() >= DRAFT_LIFETIME)
        {
            *slot = None;
        }
        f(&mut slot)
    }
}

fn recorded(store: &StateStore, owner: &str, uid: &str) -> Result<Option<JournalStatus>> {
    store
        .get(owner, JOURNAL, uid)?
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}

fn acknowledgement(result: &Result<Value>) -> JournalStatus {
    match result {
        Ok(value) if code_matches(&value["code"], 200) => JournalStatus::Accepted,
        Ok(value) if code_matches(&value["code"], -1) => JournalStatus::Duplicate,
        Ok(value) if code_matches(&value["code"], -2) => JournalStatus::OfficialAccount,
        Err(error) => {
            // M55 also reads explicit duplicate/official-account codes from
            // an error body. A non-success transport never establishes code 200.
            let same_endpoint = match error.root() {
                Error::Service { endpoint, .. }
                | Error::HttpStatus {
                    endpoint: Some(endpoint),
                    ..
                } => endpoint == ENDPOINT,
                _ => false,
            };
            if same_endpoint && error.is_service_code(-1) {
                JournalStatus::Duplicate
            } else if same_endpoint && error.is_service_code(-2) {
                JournalStatus::OfficialAccount
            } else {
                JournalStatus::Uncertain
            }
        }
        _ => JournalStatus::Uncertain,
    }
}

impl MusicClient {
    pub async fn profile_report(
        &self,
        epoch: &str,
        target: &str,
        command: ProfileReportCommand,
    ) -> Result<AccountData<ProfileReportView>> {
        let token = self.playlist_token(epoch)?;
        let uid = identifier(&json!(target))?;
        let owner = token
            .uid
            .as_deref()
            .filter(|owner| {
                uid.len() <= 32 && !uid.starts_with('0') && owner.trim_start_matches('0') != uid
            })
            .ok_or(Error::InvalidInput(
                "report requires an authenticated guest profile",
            ))?;
        let store = self.session.persistent_store()?;
        let mut receipt = None;
        let status = match command {
            ProfileReportCommand::Inspect => {
                let _lock = OperationLock::acquire(
                    store.directory(),
                    owner,
                    &format!("profile-report:{uid}"),
                )?;
                self.session
                    .with_token(&token, |owner| recorded(&store, owner, &uid))?
                    .map_or(ReportStatus::Ready, ReportStatus::from)
            }
            ProfileReportCommand::Prepare { reason } => {
                let wire = reason.into_wire()?;
                self.session.with_token(&token, |owner| {
                    if let Some(previous) = recorded(&store, owner, &uid)? {
                        return Ok(ReportStatus::from(previous));
                    }
                    self.profile_report.for_token(&token, |slot| {
                        let handle = Uuid::new_v4().simple().to_string();
                        *slot = Some(Draft {
                            token: token.clone(),
                            uid: uid.clone(),
                            reason: wire,
                            receipt: handle.clone(),
                            at: Instant::now(),
                        });
                        receipt = Some(handle);
                        Ok(ReportStatus::Ready)
                    })
                })?
            }
            ProfileReportCommand::Cancel { receipt } => {
                self.session.with_token(&token, |owner| {
                    self.profile_report.for_token(&token, |slot| {
                        if slot
                            .as_ref()
                            .is_some_and(|draft| draft.matches(&token, &uid, &receipt))
                        {
                            *slot = None;
                        }
                        Ok(())
                    })?;
                    Ok(recorded(&store, owner, &uid)?
                        .map_or(ReportStatus::Ready, ReportStatus::from))
                })?
            }
            ProfileReportCommand::Submit { receipt } => {
                let _lock = OperationLock::acquire(
                    store.directory(),
                    owner,
                    &format!("profile-report:{uid}"),
                )?;
                let _relationship = OperationLock::acquire(
                    store.directory(),
                    owner,
                    &format!("profile-follow:{uid}"),
                )?;
                if let Some(previous) = self
                    .session
                    .with_token(&token, |owner| recorded(&store, owner, &uid))?
                {
                    ReportStatus::from(previous)
                } else {
                    let draft = self.session.with_token(&token, |_| {
                        self.profile_report.for_token(&token, |slot| {
                            if !slot
                                .as_ref()
                                .is_some_and(|draft| draft.matches(&token, &uid, &receipt))
                            {
                                return Err(Error::State("report confirmation expired"));
                            }
                            slot.take()
                                .ok_or(Error::State("report confirmation expired"))
                        })
                    })?;
                    self.submit_profile_report(&token, &store, &uid, draft.reason)
                        .await?
                }
            }
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: ProfileReportView { status, receipt },
            })
        })
    }

    async fn submit_profile_report(
        &self,
        token: &SessionToken,
        store: &StateStore,
        uid: &str,
        reason: String,
    ) -> Result<ReportStatus> {
        self.session
            .run_for(token, |api, _| async move {
                let detail = api
                    .call(
                        &format!("/api/w/v1/user/detail/{uid}"),
                        json!({"all":true,"userId":uid}),
                        false,
                        &[200, 403, 404],
                    )
                    .await?;
                if !code_matches(&detail["code"], 200) {
                    return Ok(ReportStatus::TargetUnavailable);
                }
                let profile = &detail["profile"];
                if identifier(&profile["userId"]).ok().as_deref() != Some(uid) {
                    return Err(Error::Response("report target identity mismatch"));
                }
                let account_status = &profile["accountStatus"];
                if matches!(
                    account_status
                        .as_i64()
                        .or_else(|| account_status.as_str()?.parse().ok()),
                    Some(30 | 40)
                ) || js_truthy(&profile["blacklist"])
                    || js_truthy(&profile["inBlacklist"])
                {
                    return Ok(ReportStatus::TargetUnavailable);
                }
                self.session.require_api(token, &api)?;
                self.session.with_token(token, |owner| {
                    store.put(owner, JOURNAL, uid, &json!(JournalStatus::Uncertain))
                })?;
                let result = api
                    .call(
                        ENDPOINT,
                        json!({"userid":uid,"reason":reason}),
                        false,
                        &[200, -1, -2],
                    )
                    .await;
                if matches!(
                    result.as_ref().err().map(Error::root),
                    Some(Error::RequestNotSent(_))
                ) {
                    self.session
                        .with_token(token, |owner| store.delete(owner, JOURNAL, uid))?;
                    return Ok(ReportStatus::NotSent);
                }
                if let Err(error) = &result
                    && (error.is_service_code(301)
                        || matches!(
                            error.root(),
                            Error::CredentialChanged | Error::LoginDeferred | Error::StaleResponse
                        ))
                {
                    // Keep the uncertain receipt while letting the existing
                    // session lifecycle handle any authoritative login error.
                    return result.map(|_| ReportStatus::Uncertain);
                }
                let status = acknowledgement(&result);
                // The persistent record is only a status; no report reason, body,
                // error text or reusable transport credentials leave this scope.
                self.session.with_token(token, |owner| {
                    store.put(owner, JOURNAL, uid, &json!(status))
                })?;
                Ok(status.into())
            })
            .await
    }
}
