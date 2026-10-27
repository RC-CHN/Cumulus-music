//! Explicit Windows declarations for the recovered PC protocol and collectors.
//! Hardware, credentials and renderer measurements never come from this profile.
use crate::{
    Error, Result,
    protocol::http::{ClientProfile, OFFICIAL_WINDOWS_USER_AGENT},
};
use serde::Serialize;

/// Selected compatibility target, not an observation of the running OS.
pub const WINDOWS_COMPATIBILITY_DESCRIPTION: &str =
    "Microsoft Windows 10 Professional (build 19045),64bit";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsBrowserIdentity {
    user_agent: &'static str,
    app_version: &'static str,
    platform: &'static str,
    language: String,
    languages: Vec<String>,
}
impl WindowsBrowserIdentity {
    /// POSIX host locale names are converted to a small, validated language-tag
    /// subset. C/POSIX and unsupported syntax use the explicit application locale.
    /// This is not a general BCP-47 parser or a locale guessed from a time zone.
    pub fn from_locales(locales: &[String], application_locale: &str) -> Result<Self> {
        if locales.len() > 32 {
            return Err(Error::InvalidInput("too many browser languages"));
        }
        let fallback = language_tag(application_locale)
            .ok_or(Error::InvalidInput("invalid application locale"))?;
        let mut languages = Vec::new();
        for locale in locales {
            if let Some(tag) = language_tag(locale)
                && !languages.contains(&tag)
            {
                languages.push(tag);
            }
        }
        if languages.is_empty() {
            languages.push(fallback);
        }
        Ok(Self {
            user_agent: OFFICIAL_WINDOWS_USER_AGENT,
            app_version: OFFICIAL_WINDOWS_USER_AGENT
                .strip_prefix("Mozilla/")
                .ok_or(Error::State("unexpected recovered user agent"))?,
            // Chromium's Windows platform token is Win32, including x64 hosts.
            platform: "Win32",
            language: languages[0].clone(),
            languages,
        })
    }
    pub fn native_profile(&self) -> Result<ClientProfile> {
        ClientProfile::windows_compatibility(WINDOWS_COMPATIBILITY_DESCRIPTION)
    }
}

fn language_tag(value: &str) -> Option<String> {
    if value.len() > 128 || !value.is_ascii() || value.contains(['\r', '\n', '\0', '@']) {
        return None;
    }
    let base = value.split_once('.').map_or(value, |(base, _)| base);
    if base.eq_ignore_ascii_case("c") || base.eq_ignore_ascii_case("posix") {
        return None;
    }
    let normalized = base.replace('_', "-");
    let mut parts = normalized.split('-');
    let first = parts.next()?;
    if !(2..=8).contains(&first.len()) || !first.bytes().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut output = first.to_ascii_lowercase();
    for (index, part) in parts.enumerate() {
        if !(2..=8).contains(&part.len()) || !part.bytes().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        let part = if index == 0 && part.len() == 4 && part.bytes().all(|c| c.is_ascii_alphabetic())
        {
            let mut script = part.to_ascii_lowercase();
            script[..1].make_ascii_uppercase();
            script
        } else if part.len() == 2 && part.bytes().all(|c| c.is_ascii_alphabetic()) {
            part.to_ascii_uppercase()
        } else {
            part.to_ascii_lowercase()
        };
        output.push('-');
        output.push_str(&part);
    }
    Some(output)
}
