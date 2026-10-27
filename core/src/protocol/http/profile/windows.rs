use super::ClientProfile;
use crate::{Error, Result};

/// cloudmusic.dll x64 3.1.42.205594, RVA 0x1dbb590. The x64 binary itself
/// contains WOW64; do not "correct" this recovered literal to Win64/x64.
pub const OFFICIAL_WINDOWS_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Safari/537.36 Chrome/91.0.4472.164 NeteaseMusicDesktop/3.1.42.205594";

/// Explicit declared compatibility profile. Description is a selected Windows
/// product/build, never inferred as a true OS measurement from Linux hardware.
impl ClientProfile {
    pub fn windows_compatibility(description: &str) -> Result<Self> {
        if description.is_empty() || description.len() > 512 {
            return Err(Error::InvalidInput(
                "invalid Windows compatibility description",
            ));
        }
        // Native queryOsVer RVA 0x32d030 erases parentheses, then replaces ASCII
        // spaces/commas. It does not collapse dashes or trim Unicode whitespace.
        let os_version: String = description
            .chars()
            .filter_map(|c| match c {
                '(' | ')' => None,
                ' ' | ',' => Some('-'),
                c => Some(c),
            })
            .collect();
        let profile = Self {
            user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
            os_version,
            client_sign: None,
        };
        profile.validate()?;
        Ok(profile)
    }
}
