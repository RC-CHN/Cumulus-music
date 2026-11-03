//! Native PC M/363/M/3138 registration helpers. Callers own consent and phase guards.
use super::nickname::{NicknameError, validate};
use crate::protocol::api::Api;
use serde_json::{Value, json};

pub(super) fn accepted(response: &Value) -> bool {
    response["code"] == 200 && response["data"] != false && response["data"]["success"] != false
}
pub(super) fn payload(phone: &str, country: &str, captcha: &str, force: bool) -> Value {
    json!({"phone":phone,"countrycode":country,"captcha":captcha,"password":"","force":force.to_string()})
}
pub(super) async fn hint(api: &dyn Api) -> Option<String> {
    api.request("/api/nickname/hint/get", json!({"isAlter":false}), false)
        .await
        .ok()
        .filter(accepted)
        .and_then(|reply| {
            reply["hintNickname"]
                .as_str()
                .and_then(|value| validate(value).ok())
                .map(str::to_owned)
        })
}
pub(super) async fn check(api: &dyn Api, name: &str) -> Option<NicknameError> {
    match api
        .request("/api/nickname/duplicated", json!({"nickname":name}), false)
        .await
    {
        Ok(reply) if accepted(&reply) && reply["duplicated"] == false => None,
        Ok(reply) if accepted(&reply) && reply["duplicated"] == true => Some(NicknameError::Taken),
        _ => Some(NicknameError::CheckFailed),
    }
}
/// Only explicit precondition rejections settle a write intent. Missing codes,
/// a contradictory code200, challenges and server failures remain unresolved.
pub(super) fn rejection(response: &Value) -> Option<NicknameError> {
    match response["code"].as_i64() {
        Some(503) => Some(NicknameError::SmsInvalid),
        Some(504) => Some(NicknameError::PhoneRegistered),
        Some(250) => Some(NicknameError::AntiAbuse),
        Some(509) => Some(NicknameError::RateLimited),
        _ => None,
    }
}
