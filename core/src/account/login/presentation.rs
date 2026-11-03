//! Public login metadata recovered from PC modules 541/278. The country table
//! is the official client's snapshot, not a current numbering-plan database.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginCountry {
    pub alpha: String,
    pub name: String,
    pub english_name: String,
    pub calling: String,
    pub allowed_lengths: Vec<usize>,
}

pub fn login_countries() -> &'static [LoginCountry] {
    static COUNTRIES: OnceLock<Vec<LoginCountry>> = OnceLock::new();
    COUNTRIES.get_or_init(|| {
        serde_json::from_str(include_str!("countries.json"))
            .expect("the bundled official country table is valid")
    })
}

pub fn valid_phone_number(phone: &str, country: &str) -> bool {
    // The original loose branch accepts any nonempty digit string. Bound that
    // branch to 32 bytes at the native boundary; retain the common-code rules.
    if phone.is_empty()
        || phone.len() > 32
        || !phone.bytes().all(|b| b.is_ascii_digit())
        || !(1..=4).contains(&country.len())
        || !country.bytes().all(|b| b.is_ascii_digit())
    {
        return false;
    }
    login_countries()
        .iter()
        .find(|row| row.calling == country)
        .is_none_or(|row| {
            row.allowed_lengths.is_empty() || row.allowed_lengths.contains(&phone.len())
        })
}

/// Only public, fixed destinations. No arbitrary URL can cross this command.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginDocument {
    Service,
    Privacy,
    Children,
    EmailRecovery,
    ClientRelease,
    AppDownload,
}
impl LoginDocument {
    pub fn url(self) -> &'static str {
        match self {
            Self::Service => "http://st.music.163.com/official-terms/service",
            Self::Privacy => "http://st.music.163.com/official-terms/privacy",
            Self::Children => "http://st.music.163.com/official-terms/children",
            Self::EmailRecovery => "http://reg.163.com/getpasswd/RetakePassword.jsp",
            Self::ClientRelease => "https://github.com/RC-CHN/Cumulus-music/releases",
            Self::AppDownload => "https://music.163.com/#/download",
        }
    }
}
