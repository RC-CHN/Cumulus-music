use super::MusicClient;
use crate::{
    Error, Result,
    account::login::presentation::{LoginCountry, login_countries},
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginPresentation {
    pub countries: Vec<LoginCountry>,
    pub selected_country: String,
}

impl MusicClient {
    /// Installation-local, nonsecret metadata, available before authentication.
    pub fn login_presentation(&self) -> Result<LoginPresentation> {
        let saved = self
            .registry
            .store()
            .get("_local", "login_preferences", "country")?;
        let selected = saved.as_ref().and_then(serde_json::Value::as_str);
        Ok(LoginPresentation {
            countries: login_countries().to_vec(),
            selected_country: selected
                .filter(|alpha| login_countries().iter().any(|row| row.alpha == *alpha))
                .unwrap_or("CN")
                .into(),
        })
    }

    pub fn select_login_country(&self, alpha: &str) -> Result<()> {
        if !login_countries().iter().any(|row| row.alpha == alpha) {
            return Err(Error::InvalidInput("unknown login country"));
        }
        self.registry.store().put(
            "_local",
            "login_preferences",
            "country",
            &serde_json::Value::String(alpha.into()),
        )
    }
}
