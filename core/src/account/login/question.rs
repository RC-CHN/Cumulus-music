//! Candidate-owned account question. Reads may retry; unknown answers only reconcile.
use super::{CredentialLogin, LoginProtocol, LoginStatus, request::LoginRequest, state::Operation};
use crate::{Error, Result};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionPhase {
    Loading,
    Ready,
    Verifying,
    Failed,
    RecoveryRequired,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionVerificationView {
    pub id: String,
    pub phase: QuestionPhase,
    pub question_type: Option<u8>,
    pub code: Option<i64>,
    pub answer_rejected: bool,
}
pub(super) struct Attempt {
    pub view: QuestionVerificationView,
    pub offered: Vec<String>,
    pub cancel: tokio::sync::watch::Sender<bool>,
}
impl CredentialLogin {
    /// Read-only recovery still belongs to this cancellable question attempt.
    pub async fn question_reconcile(&self) -> Result<super::LoginSnapshot> {
        let cancel = self
            .lock()?
            .question
            .as_ref()
            .ok_or(Error::StaleResponse)?
            .cancel
            .clone();
        let mut cancelled = cancel.subscribe();
        tokio::select! {
            biased;
            _ = cancelled.changed() => Err(Error::StaleResponse),
            result = tokio::time::timeout(std::time::Duration::from_secs(45),self.reconcile()) => result.unwrap_or_else(|_|Err(Error::Transport("question reconciliation timeout".into()))),
        }
    }
    pub fn question_snapshot(&self) -> Result<Option<QuestionVerificationView>> {
        Ok(self.lock()?.question.as_ref().map(|q| q.view.clone()))
    }
    pub async fn question_read(&self) -> Result<Value> {
        let operation =
            Operation::begin_checked(&self.state, LoginStatus::LoadingVerification, None, |s| {
                if s.pending.is_none()
                    || s.verification != Some(LoginStatus::SecurityQuestion)
                    || s.question
                        .as_ref()
                        .is_some_and(|q| q.view.phase != QuestionPhase::Failed)
                {
                    return Err(Error::State("no readable account question pending"));
                }
                Ok(())
            })?;
        let (cancel, _) = tokio::sync::watch::channel(false);
        operation.lock()?.question = Some(Attempt {
            view: QuestionVerificationView {
                id: uuid::Uuid::new_v4().to_string(),
                phase: QuestionPhase::Loading,
                question_type: None,
                code: None,
                answer_rejected: false,
            },
            offered: vec![],
            cancel: cancel.clone(),
        });
        let mut cancelled = cancel.subscribe();
        let work = async {
            let mut payload = json!({});
            // PC JSK passes an omitted scene through M/48 as "undefined";
            // the legacy helper sends no scene at all (M/2992 + M/15).
            if self.protocol == LoginProtocol::Jsk {
                payload["scene"] = "undefined".into();
            }
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: "/api/login/cellphone/twice/question/get",
                        payload,
                    },
                    false,
                    Some(LoginStatus::SecurityQuestion),
                )
                .await?;
            if new_challenge(&response) {
                operation.lock()?.question = None;
                return Ok(response);
            }
            if !accepted(&response) {
                let mut state = operation.lock()?;
                let q = state.question.as_mut().ok_or(Error::StaleResponse)?;
                q.view.code = response["code"].as_i64();
                q.view.phase = QuestionPhase::Failed;
                return Ok(response);
            }
            if response["data"].is_null()
                || response["data"].as_object().is_some_and(|v| v.is_empty())
            {
                operation
                    .lock()?
                    .question
                    .as_mut()
                    .ok_or(Error::StaleResponse)?
                    .view
                    .phase = QuestionPhase::RecoveryRequired;
                self.complete_verification(&operation).await?;
                return Ok(response);
            }
            let data = &response["data"];
            valid_text(&data["title"])?;
            let options = data["options"]
                .as_array()
                .filter(|v| v.len() <= 64)
                .ok_or(Error::Response("invalid account question options"))?;
            let mut offered = Vec::with_capacity(options.len() + 1);
            for option in options {
                let title = valid_text(&option["title"])?;
                if title == "none" || offered.iter().any(|v| v == title) {
                    return Err(Error::Response("ambiguous account question options"));
                }
                offered.push(title.to_owned());
            }
            offered.push("none".into());
            let mut state = operation.lock()?;
            let q = state.question.as_mut().ok_or(Error::StaleResponse)?;
            q.offered = offered;
            q.view.phase = QuestionPhase::Ready;
            q.view.code = Some(200);
            q.view.question_type = data["questionType"]
                .as_u64()
                .and_then(|v| u8::try_from(v).ok());
            Ok(response)
        };
        let result = tokio::select! {
            biased;
            _ = cancelled.changed() => Err(Error::StaleResponse),
            result = tokio::time::timeout(std::time::Duration::from_secs(45),work) => result.unwrap_or_else(|_|Err(Error::Transport("question read timeout".into()))),
        };
        if result.is_err()
            && let Ok(mut state) = operation.lock()
            && let Some(q) = &mut state.question
            && q.view.phase == QuestionPhase::Loading
        {
            q.view.phase = QuestionPhase::Failed;
        }
        operation.finish(result)
    }
    pub async fn question_answer(&self, id: &str, answer: &str) -> Result<()> {
        let operation = Operation::begin_checked(&self.state, LoginStatus::Verifying, None, |s| {
            let q = s
                .question
                .as_ref()
                .filter(|q| q.view.id == id)
                .ok_or(Error::StaleResponse)?;
            if s.verification != Some(LoginStatus::SecurityQuestion)
                || q.view.phase != QuestionPhase::Ready
                || !q.offered.iter().any(|v| v == answer)
            {
                return Err(Error::InvalidInput(
                    "answer was not offered for this question",
                ));
            }
            Ok(())
        })?;
        let cancel = {
            let mut state = operation.lock()?;
            let q = state.question.as_mut().ok_or(Error::StaleResponse)?;
            q.view.phase = QuestionPhase::Verifying;
            q.view.answer_rejected = false;
            q.view.code = None;
            q.cancel.clone()
        };
        let mut cancelled = cancel.subscribe();
        let work = async {
            let mut payload = json!({"ans":answer});
            if self.protocol == LoginProtocol::Jsk {
                payload["scene"] = "undefined".into();
            }
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: "/api/login/cellphone/twice/question/verify",
                        payload,
                    },
                    false,
                    Some(LoginStatus::SecurityQuestion),
                )
                .await?;
            if new_challenge(&response) {
                operation.lock()?.question = None;
                return Ok(());
            }
            let success = accepted(&response);
            {
                let mut state = operation.lock()?;
                let q = state.question.as_mut().ok_or(Error::StaleResponse)?;
                q.view.code = response["code"].as_i64();
                q.view.answer_rejected = !success;
                q.view.phase = if success {
                    QuestionPhase::RecoveryRequired
                } else {
                    QuestionPhase::Ready
                };
            }
            if success {
                self.complete_verification(&operation).await?;
            }
            Ok(())
        };
        let result = tokio::select! {
            biased;
            _ = cancelled.changed() => Err(Error::StaleResponse),
            result = tokio::time::timeout(std::time::Duration::from_secs(45),work) => result.unwrap_or_else(|_|Err(Error::Transport("question verification timeout".into()))),
        };
        if let Err(error) = &result
            && let Ok(mut state) = operation.lock()
            && let Some(q) = &mut state.question
        {
            q.view.phase = if matches!(error.root(), Error::RequestNotSent(_))
                && q.view.phase == QuestionPhase::Verifying
            {
                QuestionPhase::Ready
            } else {
                QuestionPhase::RecoveryRequired
            };
        }
        operation.finish(result)
    }
}
fn accepted(v: &Value) -> bool {
    v["code"] == 200 && v["data"] != false && v["data"]["success"] != false
}
fn new_challenge(v: &Value) -> bool {
    matches!(
        v["code"].as_i64(),
        Some(8830 | 8860 | 8861 | 8831 | 8821 | 702 | 8840)
    )
}
fn valid_text(v: &Value) -> Result<&str> {
    v.as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= 512 && !s.chars().any(char::is_control))
        .ok_or(Error::Response("invalid account question text"))
}
