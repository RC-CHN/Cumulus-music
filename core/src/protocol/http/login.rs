//! Explicit login boundaries; a business response cannot rotate login credentials.
use crate::{Error, Result};
use std::sync::Mutex;

/// Native attempt-local SDK VerifyManager state. Not reconstructed from a
/// persistent Cookie jar: SDK only accepts this header on NEED_VERIFY events.
#[derive(Default)]
pub(crate) struct JskLoginContext {
    ticket: Mutex<Option<String>>,
}
impl JskLoginContext {
    pub(super) fn ticket(&self) -> Result<Option<String>> {
        Ok(self.ticket.lock().map_err(|_| Error::LockPoisoned)?.clone())
    }
    pub(super) fn observe_challenge(&self, headers: &[String]) -> Result<()> {
        for header in headers {
            let Some((name, value)) = header
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .split_once('=')
            else {
                continue;
            };
            if name != "MUSIC_TU" {
                continue;
            }
            if value.len() > 16_384 || value.contains(['\r', '\n', '\0']) {
                return Err(Error::Response("invalid JSK challenge header"));
            }
            if !value.is_empty() {
                *self.ticket.lock().map_err(|_| Error::LockPoisoned)? = Some(value.into());
            }
            break;
        }
        Ok(())
    }
}
pub(super) fn completes_login(path: &str, cookies: &[String]) -> bool {
    let primary = matches!(
        path,
        "/api/w/login"
            | "/api/w/login/cellphone"
            | "/api/w/register/cellphone"
            | "/api/login"
            | "/api/login/cellphone"
            | "/api/login/qrcode/client/login"
            | "/api/login/token/refresh/v1"
    );
    if primary {
        return true;
    }
    // A verification acknowledgement alone is not an authorization receipt.
    // Keep a genuine returned pair for the candidate's identity reconciliation.
    let authorized = cookies.iter().any(|header| {
        header
            .split(';')
            .next()
            .and_then(|pair| pair.trim().split_once('='))
            .is_some_and(|(name, value)| name == "MUSIC_U" && !value.is_empty())
    });
    if !authorized {
        return false;
    }
    verification_completion(path)
}

fn verification_completion(path: &str) -> bool {
    matches!(
        path,
        "/api/login/2fa/mail/captcha/check"
            | "/api/login/origin-device/apply/check"
            | "/api/login/origin-device/scan-apply/check"
            | "/api/user/2fa/securityEmail/verify"
            | "/api/middle/account/captcha/verify"
            | "/api/middle/account/uplinkcaptcha/verify"
            | "/api/user/2fa/order/check"
            | "/api/user/2fa/bindcellphone"
            | "/api/login/cellphone/twice/question/verify"
            | "/api/login/cellphone/twice/used/confirm"
    )
}

pub(super) fn authentication_write(path: &str) -> bool {
    verification_completion(path)
        || matches!(
            path,
            "/api/w/login" | "/api/w/login/cellphone" | "/api/w/register/cellphone"
        )
}

pub(super) fn can_emit_verification(path: &str) -> bool {
    verification_completion(path)
        || matches!(
            path,
            "/api/login" | "/api/login/cellphone" | "/api/login/qrcode/client/login"
        )
}

pub(super) fn jsk_login_path(path: &str) -> bool {
    matches!(
        path,
        "/api/login"
            | "/api/login/origin-device/apply/start"
            | "/api/login/origin-device/apply/check-status"
            | "/api/login/origin-device/apply/check"
            | "/api/login/origin-device/apply/cancel"
            | "/api/login/origin-device/scan-apply/start"
            | "/api/login/origin-device/scan-apply/check"
            | "/api/login/origin-device/scan-apply/cancel"
            | "/api/login/cellphone"
            | "/api/login/qrcode/unikey"
            | "/api/login/qrcode/client/login"
            | "/api/user/2fa/list"
            | "/api/user/2fa/begin"
            | "/api/login/2fa/mail/captcha/send"
            | "/api/login/2fa/mail/captcha/check"
            | "/api/user/2fa/securityEmail/send"
            | "/api/user/2fa/securityEmail/verify"
            | "/api/middle/account/captcha/sent"
            | "/api/middle/account/captcha/verify"
            | "/api/middle/account/uplinkcaptcha/get"
            | "/api/middle/account/uplinkcaptcha/verify"
            | "/api/user/2fa/order/list"
            | "/api/user/2fa/order/check"
            | "/api/user/2fa/bindcellphone"
            | "/api/login/cellphone/twice/question/get"
            | "/api/login/cellphone/twice/question/verify"
            | "/api/login/cellphone/twice/used/user/get"
            | "/api/login/cellphone/twice/used/confirm"
            | "/api/login/penalty/status"
    )
}
