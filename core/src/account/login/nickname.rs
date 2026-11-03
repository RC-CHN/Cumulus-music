//! Nickname policy shared by ordinary signup and phone-reuse registration.
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NicknameError {
    TooShort,
    TooLong,
    InvalidCharacters,
    Taken,
    CheckFailed,
    Rejected,
    SmsInvalid,
    PhoneRegistered,
    AntiAbuse,
    RateLimited,
}

pub(super) fn validate(value: &str) -> std::result::Result<&str, NicknameError> {
    let value = crate::protocol::wire::serialization::trim_js(value);
    let length = value
        .encode_utf16()
        .map(|c| if c <= 255 { 1 } else { 2 })
        .sum::<usize>();
    if length < 4 {
        return Err(NicknameError::TooShort);
    }
    if length > 30 {
        return Err(NicknameError::TooLong);
    }
    if !value.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_')
            || ('\u{4e00}'..='\u{9fa5}').contains(&c)
    }) {
        return Err(NicknameError::InvalidCharacters);
    }
    Ok(value)
}
