//! M/402 frontend header and native MetaInfo deliberately use different wire
//! shapes. Scoped authentication is never widened to obtain a request header.
use super::{
    ApiOrigin, RoutingPolicy, WireRoute,
    profile::{APP_VERSION, ClientProfile},
    select_route_with_policy,
};
use crate::{
    Error, Result,
    account::credentials::RequestCredentials,
    protocol::wire::{
        crypto::random_bytes,
        eapi,
        serialization::{compact_json, encode_component, official_data},
        weapi,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

#[derive(Clone, Copy, Default)]
pub enum EapiStyle {
    #[default]
    Frontend,
    Native,
    FrontendWithAuthHeader,
}

#[derive(Clone, Copy, Default)]
pub struct RequestOptions {
    pub cache: bool,
    pub encrypt_response: bool,
    pub style: EapiStyle,
    pub routing: RoutingPolicy,
}

/// Private outbound data. No Debug/Serialize because Cookie and body are secret.
pub struct RequestPlan {
    pub url: Url,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub route: WireRoute,
    pub decrypt_response: bool,
}

pub struct RequestInput<'a> {
    pub origin: ApiOrigin,
    pub path: &'a str,
    pub payload: Value,
    pub credentials: &'a RequestCredentials,
    pub profile: &'a ClientProfile,
    pub extra_headers: &'a BTreeMap<String, String>,
    pub options: RequestOptions,
}

pub fn build_request(input: RequestInput<'_>) -> Result<RequestPlan> {
    input.profile.validate()?;
    for (key, value) in input.extra_headers {
        if ![
            "MConfig-Info",
            "MConfig-Bucket",
            "X-antiCheatToken",
            "x-login-chain-id",
        ]
        .contains(&key.as_str())
            || value.contains(['\r', '\n', '\0'])
        {
            return Err(Error::InvalidInput(
                "unsupported supplemental request header",
            ));
        }
    }
    let route = select_route_with_policy(
        input.origin,
        input.path,
        &input.payload,
        input.options.routing,
    )?;
    let mut url = input.origin.request_url(route, input.path)?;
    if url != input.credentials.target {
        return Err(Error::InvalidInput(
            "cookies were not scoped to this request URL",
        ));
    }
    let cookies = &input.credentials.cookies;
    let names: HashSet<_> = cookies.iter().map(|(name, _)| name.as_str()).collect();
    let cookie = |name: &str| {
        cookies
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
            .unwrap_or("")
    };
    let mut headers = BTreeMap::from([
        (
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into(),
        ),
        ("Referer".into(), "https://music.163.com/".into()),
        ("User-Agent".into(), input.profile.user_agent.clone()),
    ]);
    let mut cookie_parts: Vec<_> = cookies
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    if route == WireRoute::Weapi {
        let mut payload = input.payload;
        payload["csrf_token"] = cookie("__csrf").into();
        let envelope = weapi::encrypt(&payload)?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("params", &envelope.params)
            .append_pair("encSecKey", &envelope.encrypted_key)
            .finish()
            .into_bytes();
        if !cookie_parts.is_empty() {
            headers.insert("Cookie".into(), cookie_parts.join("; "));
        }
        headers.extend(input.extra_headers.clone());
        return Ok(RequestPlan {
            url,
            headers,
            body,
            route,
            decrypt_response: false,
        });
    }
    let style = if input.path == "/api/upload/cloud/info/v2" {
        EapiStyle::Native
    } else {
        input.options.style
    };
    let frontend_wire = !matches!(style, EapiStyle::Native);
    // Per-request M/363 login and M/612 artist flags take precedence over
    // the transport default. Requesting encryption must also enable decoding.
    let encrypt_response = match input.payload.get("e_r") {
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(Error::InvalidInput("e_r must be a boolean")),
        None => input.options.encrypt_response,
    };
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::State("system clock precedes UNIX epoch"))?
        .as_millis();
    let request_id = if frontend_wire {
        json!(0)
    } else {
        json!(format!("{millis}_{}", hex::encode(random_bytes::<2>()?)))
    };
    let mut full_header = json!({"os":"pc","appver":APP_VERSION,"osver":input.profile.os_version,"deviceId":input.credentials.device_id,"channel":"netease","requestId":request_id,"__csrf":cookie("__csrf")});
    if let Some(sign) = &input.profile.client_sign {
        full_header["clientSign"] = sign.as_str().into()
    }
    for name in ["MUSIC_U", "MUSIC_A"] {
        if !cookie(name).is_empty() {
            full_header[name] = cookie(name).into()
        }
    }
    for (key, value) in input.extra_headers {
        full_header[key] = value.clone().into();
        headers.insert(key.clone(), value.clone());
    }
    let mut encrypted_header = full_header.clone();
    if matches!(style, EapiStyle::Frontend) {
        let fields = [
            "clientSign",
            "os",
            "appver",
            "deviceId",
            "requestId",
            "osver",
        ];
        encrypted_header
            .as_object_mut()
            .ok_or(Error::State("missing device header"))?
            .retain(|key, _| {
                fields.contains(&key.as_str()) || input.extra_headers.contains_key(key)
            });
    }
    for (key, value) in full_header
        .as_object()
        .ok_or(Error::State("missing device context"))?
    {
        if names.contains(key.as_str()) || input.extra_headers.contains_key(key) {
            continue;
        }
        if matches!(style, EapiStyle::Frontend)
            && ![
                "os",
                "deviceId",
                "osver",
                "appver",
                "clientSign",
                "channel",
                "mode",
            ]
            .contains(&key.as_str())
        {
            continue;
        }
        let value = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        cookie_parts.push(format!(
            "{}={}",
            encode_component(key),
            encode_component(&value)
        ));
    }
    let mut data = if frontend_wire {
        official_data(&input.payload)?
    } else {
        input.payload
    };
    data["e_r"] = json!(encrypt_response);
    if input.options.cache {
        let (_, key) = eapi::cache_envelope(&data)?;
        url.set_query(Some(&format!("cache_key={}", encode_component(&key))));
        data["cache_key"] = key.into();
    }
    if frontend_wire {
        if !encrypt_response {
            data.as_object_mut()
                .ok_or(Error::State("missing payload"))?
                .remove("e_r");
        }
        data["header"] = compact_json(&encrypted_header)?.into();
    } else {
        data["header"] = encrypted_header;
    }
    let encrypted = eapi::encrypt(input.path, &data)?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("params", &encrypted)
        .finish()
        .into_bytes();
    if !cookie_parts.is_empty() {
        headers.insert("Cookie".into(), cookie_parts.join("; "));
    }
    Ok(RequestPlan {
        url,
        headers,
        body,
        route,
        decrypt_response: encrypt_response,
    })
}
