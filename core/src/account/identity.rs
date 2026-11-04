//! Minimum same-account metadata from the existing authenticated identity read.
use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
};
use serde_json::Value;

#[derive(Clone)]
pub(crate) struct VerifiedAccount {
    pub profile: Value,
    pub registration_type: Option<u32>,
}

impl VerifiedAccount {
    pub(crate) async fn read(api: &dyn Api) -> Result<Self> {
        Self::from_reply(&api.account_reply().await?)
    }

    pub(crate) fn profile_only(profile: Value) -> Self {
        Self {
            profile,
            registration_type: None,
        }
    }

    pub(crate) fn from_reply(reply: &Value) -> Result<Self> {
        let profile = &reply["profile"];
        let uid = identifier(&profile["userId"])
            .map_err(|_| Error::Response("missing verified account identity"))?;
        // M198 uses account.type, not the login channel or a binding's type.
        // A missing/inconsistent account identity may not classify this UID.
        let account = &reply["account"];
        let registration_type = if identifier(&account["id"]).as_ref().ok() == Some(&uid) {
            match account["type"].as_u64() {
                Some(value @ (0 | 1 | 2 | 5 | 10)) => Some(value as u32),
                _ => None,
            }
        } else {
            None
        };
        Ok(Self {
            profile: profile.clone(),
            registration_type,
        })
    }
}
