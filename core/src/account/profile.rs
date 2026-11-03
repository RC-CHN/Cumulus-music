//! Read-only account detail and bindings from the recovered PC host.
pub mod avatar;
pub mod edit;
pub mod regions;
use super::{AccountSession, SessionStatus, SessionToken};
use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfProfileView {
    pub uid: String,
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
    pub signature: Option<String>,
    pub gender: Option<u8>,
    /// Server milliseconds, including negative dates before 1970.
    pub birthday: Option<i64>,
    pub province: Option<u32>,
    pub city: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_label: Option<String>,
    pub level: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingProvider {
    Email,
    Cellphone,
    Weibo,
    Qq,
    Wechat,
    Unknown,
}

/// A returned row confirms a binding exists. Its display details may be absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountBindingView {
    pub provider: BindingProvider,
    pub type_code: u32,
    pub display_name: Option<String>,
    pub masked_address: Option<String>,
    pub country_code: Option<String>,
    pub has_password: Option<bool>,
    pub details_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfProfileDetailView {
    pub profile: SelfProfileView,
    pub bindings: Vec<AccountBindingView>,
}

pub struct AccountProfile {
    session: Arc<AccountSession>,
}

impl AccountProfile {
    pub fn new(session: Arc<AccountSession>) -> Self {
        Self { session }
    }

    pub fn snapshot_for(&self, token: &SessionToken) -> Result<SelfProfileView> {
        let snapshot = self.session.snapshot()?;
        if snapshot.token != *token {
            return Err(Error::StaleResponse);
        }
        if snapshot.status != SessionStatus::Authenticated {
            return Err(Error::State("no authenticated account"));
        }
        let profile = snapshot
            .profile
            .as_ref()
            .ok_or(Error::State("missing authenticated profile"))?;
        self.session
            .with_token(token, |uid| project_profile(uid, profile, None))
    }

    pub async fn detail_for(&self, token: &SessionToken) -> Result<SelfProfileView> {
        let mut profile = self
            .session
            .run_for(token, |api, uid| async move {
                // PC M15:1742–1756 and M100:63–82.
                let response = api
                    .call(
                        &format!("/api/w/v1/user/detail/{uid}"),
                        json!({"all": true, "userId": uid}),
                        false,
                        &[200],
                    )
                    .await?;
                project_profile(&uid, &response["profile"], unsigned(&response["level"]))
            })
            .await?;
        self.session.with_token(token, |uid| {
            if let (Ok(store), Some(province), Some(city)) = (
                self.session.persistent_store(),
                profile.province,
                profile.city,
            ) {
                profile.region_label =
                    regions::RegionTable::cached_label(&store, uid, province, city)?;
            }
            Ok(profile)
        })
    }

    pub async fn bindings_for(&self, token: &SessionToken) -> Result<Vec<AccountBindingView>> {
        self.session
            .run_for(token, |api, uid| async move {
                // PC M15:3642–3644 and M100:179–185. Unlike the PC helper,
                // retain read errors rather than presenting an empty list.
                let response = api
                    .call(
                        &format!("/api/w/v1/user/bindings/{uid}"),
                        json!({"userId": uid}),
                        false,
                        &[200],
                    )
                    .await?;
                response["bindings"]
                    .as_array()
                    .filter(|rows| rows.len() <= 32)
                    .ok_or(Error::Response("missing or oversized account bindings"))?
                    .iter()
                    .map(project_binding)
                    .collect()
            })
            .await
    }

    pub async fn read_for(&self, token: &SessionToken) -> Result<SelfProfileDetailView> {
        let profile = self.detail_for(token).await?;
        let bindings = self.bindings_for(token).await?;
        self.session
            .with_token(token, |_| Ok(SelfProfileDetailView { profile, bindings }))
    }
}

fn project_profile(uid: &str, profile: &Value, level: Option<u32>) -> Result<SelfProfileView> {
    let observed = identifier(&profile["userId"])
        .map_err(|_| Error::Response("missing account profile identity"))?;
    if observed != uid {
        return Err(Error::Response("account profile identity mismatch"));
    }
    Ok(SelfProfileView {
        uid: uid.into(),
        nickname: display_text(&profile["nickname"], 256, false),
        avatar_url: avatar(&profile["avatarUrl"]),
        signature: display_text(&profile["signature"], 1200, true),
        gender: match profile["gender"].as_u64() {
            Some(1) => Some(1),
            Some(2) => Some(2),
            _ => None,
        },
        birthday: profile["birthday"].as_i64().filter(|millis| *millis != 0),
        province: unsigned(&profile["province"]).filter(|code| *code != 0),
        city: unsigned(&profile["city"]).filter(|code| *code != 0),
        region_label: None,
        level,
    })
}

fn project_binding(binding: &Value) -> Result<AccountBindingView> {
    let type_code = unsigned(&binding["type"]).ok_or(Error::Response("invalid binding type"))?;
    let provider = match type_code {
        0 => BindingProvider::Email,
        1 => BindingProvider::Cellphone,
        2 => BindingProvider::Weibo,
        5 => BindingProvider::Qq,
        10 => BindingProvider::Wechat,
        _ => BindingProvider::Unknown,
    };
    let mut view = AccountBindingView {
        provider,
        type_code,
        display_name: None,
        masked_address: None,
        country_code: None,
        has_password: None,
        details_available: false,
    };
    if provider == BindingProvider::Unknown {
        return Ok(view);
    }
    let Some(token) = binding["tokenJsonStr"]
        .as_str()
        .filter(|token| token.len() <= 16 * 1024)
        .and_then(|token| serde_json::from_str::<Value>(token).ok())
        .filter(Value::is_object)
    else {
        return Ok(view);
    };
    // PC pages/bind/views/index.js:260–339. Do not export provider identifiers,
    // tokens, raw email/phone, or the navigation URL on the outer binding.
    match provider {
        BindingProvider::Email => {
            view.masked_address = token["email"].as_str().and_then(mask_email);
        }
        BindingProvider::Cellphone => {
            view.masked_address = digits(&token["cellphone"], 7, 15)
                .map(|phone| format!("***{}", &phone[phone.len() - 4..]));
            view.country_code = digits(&token["countrycode"], 1, 4).map(str::to_owned);
            view.has_password = token["hasPassword"].as_bool();
        }
        BindingProvider::Weibo => {
            view.display_name = display_text(&token["screen_name"], 256, false)
                .or_else(|| display_text(&token["name"], 256, false));
        }
        BindingProvider::Qq | BindingProvider::Wechat => {
            view.display_name = display_text(&token["nickname"], 256, false);
        }
        BindingProvider::Unknown => {}
    }
    view.details_available = view.display_name.is_some()
        || view.masked_address.is_some()
        || view.country_code.is_some()
        || view.has_password.is_some();
    Ok(view)
}

fn unsigned(value: &Value) -> Option<u32> {
    value.as_u64()?.try_into().ok()
}

fn digits(value: &Value, min: usize, max: usize) -> Option<&str> {
    value.as_str().filter(|text| {
        (min..=max).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn display_text(value: &Value, limit: usize, multiline: bool) -> Option<String> {
    value
        .as_str()
        .filter(|text| {
            (multiline || !text.is_empty())
                && text.len() <= limit
                && !text
                    .chars()
                    .any(|character| character.is_control() && !(multiline && character == '\n'))
        })
        .map(str::to_owned)
}

fn mask_email(email: &str) -> Option<String> {
    let (local, domain) = email.split_once('@')?;
    if email.len() > 254
        || !email.is_ascii()
        || local
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || !domain
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return None;
    }
    let local = local.chars().next().filter(char::is_ascii_alphanumeric)?;
    let domain = domain.chars().next().filter(char::is_ascii_alphanumeric)?;
    Some(format!("{local}***@{domain}***"))
}

fn avatar(value: &Value) -> Option<String> {
    let mut url = url::Url::parse(value.as_str().filter(|value| value.len() <= 2048)?).ok()?;
    let host = url.host_str()?;
    if ![".music.126.net", ".music.163.com"]
        .into_iter()
        .any(|suffix| host.ends_with(suffix))
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    url.set_scheme("https").ok()?;
    url.set_query(Some("param=320y320"));
    url.set_fragment(None);
    Some(url.into())
}
