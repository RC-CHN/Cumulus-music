//! M3137/M3087 security QR: a native legacy ticket in the same login candidate.
//! Ticket text and complete authorization URLs never enter the public view.
use super::{CredentialLogin, LoginSnapshot, LoginStatus, state::Operation};
use crate::{
    Error, Result,
    account::{QrLogin, QrStatus},
    protocol::api::Api,
};
use qrcodegen::{QrCode, QrCodeEcc, QrSegment, Version};
use serde::Serialize;
use serde_json::Value;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::watch;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityQrView {
    pub id: String,
    pub status: QrStatus,
    pub nickname: Option<String>,
    pub modules: Option<Vec<Vec<bool>>>,
    pub poll_after_ms: u32,
    pub can_refresh: bool,
    pub can_reconcile: bool,
}

pub(super) struct Attempt {
    view: SecurityQrView,
    qr: Arc<QrLogin>,
    cancel: watch::Sender<bool>,
    identity_required: bool,
}

impl Attempt {
    pub(super) fn from_response(response: &Value, api: Arc<dyn Api>) -> Result<Self> {
        if super::classify(response) != LoginStatus::SecurityQr {
            return Err(Error::Response("not a security QR challenge"));
        }
        let key = match response["toast"].get("unikey") {
            None | Some(Value::Null) => None,
            Some(Value::String(key)) => Some(key.as_str()),
            Some(_) => return Err(Error::Response("invalid security QR ticket")),
        };
        let qr = Arc::new(QrLogin::with_seed_key(api, key)?);
        let snapshot = qr.snapshot()?;
        let nickname = response["toast"]["nickname"]
            .as_str()
            .filter(|value| {
                !value.is_empty()
                    && value.chars().count() <= 128
                    && !value.chars().any(char::is_control)
                    && !value.contains("://")
            })
            .map(str::to_owned);
        let (cancel, _) = watch::channel(false);
        Ok(Self {
            view: SecurityQrView {
                id: uuid::Uuid::new_v4().to_string(),
                status: snapshot.status,
                nickname,
                modules: snapshot.key.as_deref().map(modules).transpose()?,
                poll_after_ms: 1000,
                can_refresh: true,
                can_reconcile: false,
            },
            qr,
            cancel,
            identity_required: false,
        })
    }

    pub(super) fn view(&self) -> &SecurityQrView {
        &self.view
    }

    /// Safe while holding the parent login lock: only the independent QR mutex
    /// and watch channel are touched, never CredentialLogin.state again.
    pub(crate) fn cancel(&self) -> Result<()> {
        self.cancel.send_replace(true);
        self.qr.cancel()
    }

    pub(super) fn interrupted(&mut self) {
        self.view.status = QrStatus::Error;
        self.view.modules = None;
        self.view.can_refresh = !self.identity_required;
        self.view.can_reconcile = self.identity_required;
    }
}

fn modules(key: &str) -> Result<Vec<Vec<bool>>> {
    let mut url = url::Url::parse("https://music.163.com/login")
        .map_err(|_| Error::Protocol("security QR URL"))?;
    url.query_pairs_mut().append_pair("codekey", key);
    // M3087 explicitly selects L; rendering consumes only these bounded modules.
    // The high-level encoder silently boosts ECC when capacity permits. Keep
    // the recovered explicit L setting instead of changing its format bits.
    let qr = QrCode::encode_segments_advanced(
        &QrSegment::make_segments(url.as_str()),
        QrCodeEcc::Low,
        Version::MIN,
        Version::MAX,
        None,
        false,
    )
    .map_err(|_| Error::Response("security QR payload exceeds limit"))?;
    Ok((0..qr.size())
        .map(|y| (0..qr.size()).map(|x| qr.get_module(x, y)).collect())
        .collect())
}

async fn bounded<T>(
    cancel: watch::Sender<bool>,
    work: impl Future<Output = Result<T>>,
) -> Result<T> {
    let mut changed = cancel.subscribe();
    if *changed.borrow() {
        return Err(Error::StaleResponse);
    }
    tokio::select! {
        biased;
        _ = changed.changed() => Err(Error::StaleResponse),
        result = tokio::time::timeout(Duration::from_secs(45), work) =>
            result.unwrap_or_else(|_| Err(Error::Transport("security QR operation timeout".into()))),
    }
}

impl CredentialLogin {
    pub fn security_qr_snapshot(&self) -> Result<Option<SecurityQrView>> {
        Ok(self
            .lock()?
            .security_qr
            .as_ref()
            .map(|item| item.view.clone()))
    }

    fn security_qr_operation(
        &self,
        id: &str,
        ready: impl FnOnce(&Attempt) -> Result<()>,
    ) -> Result<(Operation<'_>, Arc<QrLogin>, watch::Sender<bool>)> {
        self.check_login_base()?;
        let operation =
            Operation::begin_checked(&self.state, LoginStatus::Verifying, None, |state| {
                let item = state
                    .security_qr
                    .as_ref()
                    .filter(|item| item.view.id == id)
                    .ok_or(Error::StaleResponse)?;
                if *item.cancel.borrow() {
                    return Err(Error::StaleResponse);
                }
                ready(item)
            })?;
        let (qr, cancel) = {
            let state = operation.lock()?;
            let item = state.security_qr.as_ref().ok_or(Error::StaleResponse)?;
            (item.qr.clone(), item.cancel.clone())
        };
        Ok((operation, qr, cancel))
    }

    fn security_qr_failed<T>(&self, operation: &Operation<'_>, result: &Result<T>) {
        if result.is_err()
            && let Ok(mut state) = operation.lock()
            && let Some(item) = &mut state.security_qr
        {
            item.interrupted();
        }
    }

    pub async fn security_qr_refresh(&self, id: &str) -> Result<SecurityQrView> {
        let (operation, qr, cancel) = self.security_qr_operation(id, |item| {
            if item.identity_required || item.view.status == QrStatus::Authorized {
                return Err(Error::State("security QR requires identity reconciliation"));
            }
            Ok(())
        })?;
        {
            let mut state = operation.lock()?;
            let item = state.security_qr.as_mut().ok_or(Error::StaleResponse)?;
            item.view.status = QrStatus::Creating;
            item.view.modules = None;
            item.view.can_refresh = false;
            item.view.can_reconcile = false;
            state.challenge = None;
            state.code = None;
        }
        let work = async {
            self.check_login_base()?;
            operation.check()?;
            let key = qr.begin().await?;
            let rendered = modules(&key)?;
            let mut state = operation.lock()?;
            let item = state.security_qr.as_mut().ok_or(Error::StaleResponse)?;
            item.view.status = QrStatus::WaitingScan;
            item.view.modules = Some(rendered);
            item.view.can_refresh = true;
            item.view.can_reconcile = false;
            let view = item.view.clone();
            state.status = LoginStatus::SecurityQr;
            Ok(view)
        };
        let result = bounded(cancel, work).await;
        self.security_qr_failed(&operation, &result);
        operation.finish(result)
    }

    pub async fn security_qr_poll(&self, id: &str, proof: Option<&str>) -> Result<SecurityQrView> {
        if proof.is_some_and(|value| {
            value.is_empty() || value.len() > 16_384 || value.chars().any(char::is_control)
        }) {
            return Err(Error::InvalidInput("invalid completed security QR CAPTCHA"));
        }
        let (operation, qr, cancel) = self.security_qr_operation(id, |item| {
            if item.identity_required {
                return Err(Error::State("security QR requires identity reconciliation"));
            }
            if !matches!(
                item.view.status,
                QrStatus::WaitingScan | QrStatus::WaitingConfirmation | QrStatus::Challenge
            ) || (proof.is_some() && item.view.status != QrStatus::Challenge)
            {
                return Err(Error::State("security QR is not pending"));
            }
            Ok(())
        })?;
        let work = async {
            self.check_login_base()?;
            // A dropped/unreadable poll may have authorized this candidate.
            // Until a complete non-803 response arrives, only account/get may
            // recover it; neither a fresh key nor a poll replay is justified.
            {
                let mut state = operation.lock()?;
                let item = state.security_qr.as_mut().ok_or(Error::StaleResponse)?;
                item.identity_required = true;
                item.view.can_refresh = false;
                item.view.can_reconcile = false;
            }
            let status = qr.poll(proof).await?;
            let snapshot = qr.snapshot()?;
            let view = {
                let mut state = operation.lock()?;
                let item = state.security_qr.as_mut().ok_or(Error::StaleResponse)?;
                item.view.status = status;
                if matches!(status, QrStatus::Expired | QrStatus::Authorized) {
                    item.view.modules = None;
                }
                item.identity_required = status == QrStatus::Authorized;
                item.view.can_refresh = !item.identity_required;
                item.view.can_reconcile = false;
                let view = item.view.clone();
                state.challenge = snapshot.challenge;
                state.code = match status {
                    QrStatus::Expired => Some(800),
                    QrStatus::WaitingScan => Some(801),
                    QrStatus::WaitingConfirmation => Some(802),
                    QrStatus::Authorized => Some(803),
                    QrStatus::Challenge => Some(8821),
                    _ => None,
                };
                state.status = LoginStatus::SecurityQr;
                view
            };
            if status == QrStatus::Authorized {
                // Unlike complete_verification, an account/get 301 must never
                // replay the original password or this QR completion request.
                self.verify_identity(&operation).await?;
            }
            Ok(view)
        };
        let result = bounded(cancel, work).await;
        self.security_qr_failed(&operation, &result);
        operation.finish(result)
    }

    /// Explicit read-only recovery, including a lost 803/account response.
    pub async fn security_qr_reconcile(&self, id: &str) -> Result<LoginSnapshot> {
        let (operation, _, cancel) = self.security_qr_operation(id, |item| {
            if !item.identity_required
                || !matches!(item.view.status, QrStatus::Error | QrStatus::Authorized)
            {
                return Err(Error::State(
                    "security QR has no interrupted identity to read",
                ));
            }
            Ok(())
        })?;
        {
            let mut state = operation.lock()?;
            let item = state.security_qr.as_mut().ok_or(Error::StaleResponse)?;
            item.view.can_refresh = false;
            item.view.can_reconcile = false;
        }
        let result = bounded(cancel, self.verify_identity(&operation)).await;
        if result
            .as_ref()
            .is_err_and(|error| error.is_service_code(301))
        {
            let mut state = operation.lock()?;
            state
                .security_qr
                .as_mut()
                .ok_or(Error::StaleResponse)?
                .identity_required = false;
        }
        self.security_qr_failed(&operation, &result);
        operation.finish(result)?;
        self.snapshot()
    }
}
