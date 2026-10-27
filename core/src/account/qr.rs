//! Official QR request sequence and cancellation boundary.
use super::session::next_generation;
use crate::{Error, Result, protocol::api::Api};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QrStatus {
    Idle,
    Creating,
    WaitingScan,
    WaitingConfirmation,
    Authorized,
    Expired,
    Challenge,
    Cancelled,
    Error,
}

struct QrState {
    generation: u64,
    key: Option<String>,
    status: QrStatus,
    challenge: Option<Value>,
    polling: bool,
}

struct PollGuard<'a> {
    state: &'a Mutex<QrState>,
    generation: u64,
}
impl Drop for PollGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock()
            && state.generation == self.generation
        {
            state.polling = false;
        }
    }
}

/// QR keys/challenge bodies are private login data, hence no Debug/Serialize.
pub struct QrSnapshot {
    pub generation: u64,
    pub key: Option<String>,
    pub status: QrStatus,
    pub challenge: Option<Value>,
}

pub struct QrLogin {
    api: Arc<dyn Api>,
    state: Mutex<QrState>,
}

impl QrLogin {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self {
            api,
            state: Mutex::new(QrState {
                generation: 0,
                key: None,
                status: QrStatus::Idle,
                challenge: None,
                polling: false,
            }),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, QrState>> {
        self.state.lock().map_err(|_| Error::LockPoisoned)
    }

    pub fn snapshot(&self) -> Result<QrSnapshot> {
        let state = self.lock()?;
        Ok(QrSnapshot {
            generation: state.generation,
            key: state.key.clone(),
            status: state.status,
            challenge: state.challenge.clone(),
        })
    }

    pub async fn begin(&self) -> Result<String> {
        let (generation, old_key) = {
            let mut state = self.lock()?;
            state.generation = next_generation(state.generation)?;
            let old_key = state.key.take();
            state.status = QrStatus::Creating;
            state.challenge = None;
            state.polling = false;
            (state.generation, old_key)
        };
        let mut payload = json!({"type": 3});
        if let Some(key) = old_key {
            payload["lastUniKey"] = key.into();
        }
        let response = self
            .api
            .call("/api/login/qrcode/unikey", payload, false, &[200])
            .await;
        let mut state = self.lock()?;
        if state.generation != generation {
            return Err(Error::StaleResponse);
        }
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                state.status = QrStatus::Error;
                return Err(error);
            }
        };
        let Some(key) = response["unikey"].as_str().filter(|key| !key.is_empty()) else {
            state.status = QrStatus::Error;
            return Err(Error::Response("missing QR key"));
        };
        state.key = Some(key.into());
        state.status = QrStatus::WaitingScan;
        Ok(key.into())
    }

    pub fn cancel(&self) -> Result<()> {
        let mut state = self.lock()?;
        state.generation = next_generation(state.generation)?;
        state.key = None;
        state.challenge = None;
        state.status = QrStatus::Cancelled;
        state.polling = false;
        Ok(())
    }

    pub async fn poll(&self, secure_captcha: Option<&str>) -> Result<QrStatus> {
        let (generation, key) = {
            let mut state = self.lock()?;
            if !matches!(
                state.status,
                QrStatus::WaitingScan | QrStatus::WaitingConfirmation | QrStatus::Challenge
            ) {
                return Err(Error::State("QR is not pending"));
            }
            if state.status == QrStatus::Challenge && secure_captcha.is_none() {
                return Ok(QrStatus::Challenge);
            }
            if state.polling {
                return Err(Error::State("QR poll is already pending"));
            }
            let key = state.key.clone().ok_or(Error::State("missing QR key"))?;
            state.polling = true;
            (state.generation, key)
        };
        // Dropping/cancelling the Future must release the pending operation.
        // A concurrent poll cannot arrive later and overwrite a terminal code.
        let _guard = PollGuard {
            state: &self.state,
            generation,
        };
        let mut payload = json!({"key": key, "type": 3});
        if let Some(captcha) = secure_captcha {
            payload["secureCaptcha"] = captcha.into();
        }
        const PATH: &str = "/api/login/qrcode/client/login";
        let response = self.api.request(PATH, payload, false).await;
        let mut state = self.lock()?;
        if state.generation != generation {
            return Err(Error::StaleResponse);
        }
        let response = response?;
        if !response.is_object() {
            state.status = QrStatus::Error;
            return Err(Error::Response("invalid QR response"));
        }
        state.status = match response["code"].as_i64() {
            Some(800) => QrStatus::Expired,
            Some(801) => QrStatus::WaitingScan,
            Some(802) => QrStatus::WaitingConfirmation,
            Some(803) => QrStatus::Authorized,
            Some(8821) => QrStatus::Challenge,
            _ => QrStatus::Error,
        };
        state.challenge = (state.status == QrStatus::Challenge).then(|| response.clone());
        if state.status == QrStatus::Error {
            return Err(Error::service(PATH, response["code"].clone(), response));
        }
        Ok(state.status)
    }
}
