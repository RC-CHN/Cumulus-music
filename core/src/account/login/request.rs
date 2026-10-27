use crate::{Error, Result, protocol::wire::eapi::md5_hex};
use serde_json::{Value, json};

pub enum PhoneCredential<'a> {
    Password(&'a str),
    Sms(&'a str),
}

pub struct PhoneLogin<'a> {
    pub phone: &'a str,
    pub country_code: &'a str,
    pub credential: PhoneCredential<'a>,
    pub remember: bool,
    pub secure_captcha: Option<&'a str>,
}

#[derive(Clone)]
pub(super) struct LoginRequest {
    pub path: &'static str,
    pub payload: Value,
}

pub(super) fn phone_number(phone: &str, country: &str) -> Result<()> {
    if !(5..=15).contains(&phone.len())
        || !phone.bytes().all(|b| b.is_ascii_digit())
        || !(1..=4).contains(&country.len())
        || !country.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(Error::InvalidInput("invalid phone or country code"));
    }
    Ok(())
}

fn password_digest(password: &str) -> Result<String> {
    if password.is_empty() {
        return Err(Error::InvalidInput("empty password"));
    }
    Ok(md5_hex(password.as_bytes()))
}

pub(super) fn phone(input: PhoneLogin<'_>) -> Result<LoginRequest> {
    phone_number(input.phone, input.country_code)?;
    let mut payload = json!({"type":1,"phone":input.phone,"countrycode":input.country_code,"remember":input.remember,"https":true,"e_r":true});
    match input.credential {
        PhoneCredential::Password(password) => {
            payload["password"] = password_digest(password)?.into();
            if let Some(value) = input.secure_captcha.filter(|s| !s.is_empty()) {
                payload["secureCaptcha"] = value.into();
            }
        }
        PhoneCredential::Sms(code) => {
            if !(4..=8).contains(&code.len()) || !code.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::InvalidInput("invalid SMS code"));
            }
            payload["captcha"] = code.into();
            // M/363 only includes initial secureCaptcha in password mode.
            // A subsequent challenge is resumed by submit_captcha explicitly.
        }
    }
    Ok(LoginRequest {
        path: "/api/w/login/cellphone",
        payload,
    })
}

pub(super) fn email(
    address: &str,
    password: &str,
    remember: bool,
    captcha: Option<&str>,
) -> Result<LoginRequest> {
    let parts: Vec<_> = address.split('@').collect();
    if parts.len() != 2
        || parts.iter().any(|p| p.is_empty())
        || address.chars().any(char::is_whitespace)
    {
        return Err(Error::InvalidInput("invalid email"));
    }
    let mut payload = json!({"username":address,"password":password_digest(password)?,"remember":remember,"https":true,"e_r":true});
    if let Some(value) = captcha.filter(|s| !s.is_empty()) {
        payload["secureCaptcha"] = value.into();
    }
    Ok(LoginRequest {
        path: "/api/w/login",
        payload,
    })
}
