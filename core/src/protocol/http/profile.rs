use crate::{Error, Result};
mod windows;
pub use windows::OFFICIAL_WINDOWS_USER_AGENT;

pub const APP_VERSION: &str = "3.1.42.205594";

/// Native request identity, never a source of authentication or entitlements.
#[derive(Clone)]
pub struct ClientProfile {
    pub user_agent: String,
    pub os_version: String,
    pub client_sign: Option<String>,
}

impl Default for ClientProfile {
    fn default() -> Self {
        let os = match std::env::consts::OS {
            "linux" => "Linux",
            "macos" => "macOS",
            "windows" => "Windows",
            other => other,
        };
        let platform = if cfg!(target_os = "linux") {
            format!("X11; Linux {}", std::env::consts::ARCH)
        } else {
            format!("{os}; {}", std::env::consts::ARCH)
        };
        Self {
            user_agent: format!(
                "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.169 Safari/537.36 NeteaseMusicDesktop/{APP_VERSION}"
            ),
            os_version: os.into(),
            client_sign: None,
        }
    }
}

impl ClientProfile {
    pub fn validate(&self) -> Result<()> {
        for value in std::iter::once(self.user_agent.as_str())
            .chain(std::iter::once(self.os_version.as_str()))
            .chain(self.client_sign.as_deref())
        {
            if value.len() > 16384 || value.contains(['\r', '\n', '\0']) {
                return Err(Error::InvalidInput("invalid client profile field"));
            }
        }
        Ok(())
    }
}
