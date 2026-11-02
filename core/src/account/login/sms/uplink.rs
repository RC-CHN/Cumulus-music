use super::*;
use std::time::Duration;
use tokio::time::{Instant, sleep_until, timeout_at};

impl CredentialLogin {
    /// Called only after the user confirms sending the displayed SMS. SDK
    /// retries this status read every three seconds for at most 30 seconds.
    pub async fn sms_check_up(&self, id: &str) -> Result<bool> {
        let operation = self.sms_operation(id, SmsDirection::Uplink, false)?;
        let (payload, mut cancel) = {
            let mut state = operation.lock()?;
            let item = state.sms.as_mut().ok_or(Error::StaleResponse)?;
            let captcha = item
                .view
                .upload_text
                .clone()
                .ok_or(Error::State("missing SMS text"))?;
            let mut payload = json!({"channel":"music_test","captcha":captcha});
            if self.protocol == LoginProtocol::Jsk {
                payload["cellphone"] = item.view.phone_mask.clone().unwrap_or_default().into();
                payload["enUserId"] = json!("undefined");
                payload["scene"] = json!("undefined");
            }
            item.view.phase = SmsPhase::Checking;
            (payload, item.cancel.subscribe())
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        let check = async {
            loop {
                operation.check()?;
                if Instant::now() >= deadline {
                    let mut state = operation.lock()?;
                    let item = state.sms.as_mut().ok_or(Error::StaleResponse)?;
                    item.view.phase = SmsPhase::TimedOut;
                    item.view.code = Some(500);
                    return Ok(false);
                }
                let response = self
                    .send(
                        &operation,
                        LoginRequest {
                            path: "/api/middle/account/uplinkcaptcha/verify",
                            payload: payload.clone(),
                        },
                        false,
                        Some(LoginStatus::TwoFactor),
                    )
                    .await;
                match response {
                    Ok(response)
                        if response["code"] == 200
                            && crate::protocol::wire::serialization::js_truthy(
                                &response["data"],
                            )
                            && response["data"]["success"] != false =>
                    {
                        return Ok(true);
                    }
                    Ok(response)
                        if response["code"] == 200 && response["data"]["success"] == false =>
                    {
                        let mut state = operation.lock()?;
                        let item = state.sms.as_mut().ok_or(Error::StaleResponse)?;
                        item.view.phase = SmsPhase::Failed;
                        item.view.code = Some(200);
                        return Ok(false);
                    }
                    Err(Error::StaleResponse | Error::CredentialChanged) => {
                        return Err(Error::StaleResponse);
                    }
                    _ => {}
                }
                sleep_until((Instant::now() + Duration::from_secs(3)).min(deadline)).await;
            }
        };
        let result = tokio::select! {
            biased;
            _ = cancel.changed() => Err(Error::StaleResponse),
            result = timeout_at(deadline, check) => match result {
                Ok(result) => result,
                Err(_) => {
                    let mut state = operation.lock()?;
                    let item = state.sms.as_mut().ok_or(Error::StaleResponse)?;
                    item.view.phase = SmsPhase::TimedOut;
                    item.view.code = Some(500);
                    Ok(false)
                }
            },
        };
        let result = match result {
            Ok(true) => {
                operation
                    .lock()?
                    .sms
                    .as_mut()
                    .ok_or(Error::StaleResponse)?
                    .view
                    .phase = SmsPhase::RecoveryRequired;
                tokio::select! {
                    biased;
                    _ = cancel.changed() => Err(Error::StaleResponse),
                    result = self.complete_verification(&operation) => result.map(|_| true),
                }
            }
            other => other,
        };
        operation.finish(result)
    }
}
