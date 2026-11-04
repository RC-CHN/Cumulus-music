use super::{MAX_SONG_IDS, Receipt, SealedFingerprint};
use crate::{Error, Result, protocol::wire::eapi::encrypt_json};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

pub const CANONICAL_PATH: &str = "/api/music/audio/match";
pub const DEFAULT_POST_URL: &str = "https://music.163.com/eapi/music/audio/match";
pub const USER_AGENT: &str = "Windows_Curl;";
pub const REFERER: &str = "music.163.com";
pub const OS: &str = "pc";

const MAX_FINGERPRINT_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_SONG_ID_BYTES: usize = 64;
const MAX_JS_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// 62C0C0: host UUID string without braces, case preserved. It is distinct
/// from a local generation token and is reused across one session's jobs.
pub struct SessionUuid(Box<str>);

impl SessionUuid {
    pub fn from_host(text: &str) -> Result<Self> {
        if text.len() != 36
            || !text.bytes().enumerate().all(|(idx, byte)| {
                if matches!(idx, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
            || Uuid::parse_str(text).map_or(true, |id| id.is_nil())
        {
            return Err(Error::InvalidInput("invalid native recognition UUID"));
        }
        Ok(Self(text.into()))
    }
}

/// Explicit inputs to the native JSON header. No Cookie or account material.
/// No Debug or serialization implementation exposes them automatically.
pub struct RequestContext {
    app_version: Box<str>,
    device_id: Box<str>,
    client_sign: Box<str>,
}

impl RequestContext {
    pub fn new(app_version: &str, device_id: &str, client_sign: &str) -> Result<Self> {
        if [app_version, device_id, client_sign]
            .iter()
            .any(|field| field.len() > 512 || !field.bytes().all(|b| (b' '..=b'~').contains(&b)))
            || app_version.is_empty()
        {
            return Err(Error::InvalidInput("invalid recognition fixture context"));
        }
        Ok(Self {
            app_version: app_version.into(),
            device_id: device_id.into(),
            client_sign: client_sign.into(),
        })
    }
}

// FastWriter uses member-name traversal, compact separators, and a final LF.
// Keep this field order explicit; the general Rust request templates differ.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Body<'a> {
    algorithm_code: &'static str,
    decrypt: u8,
    duration: u8,
    from: &'static str,
    header: Header<'a>,
    rawdata: String,
    session_id: &'a str,
    times: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Header<'a> {
    appver: &'a str,
    client_sign: &'a str,
    device_id: &'a str,
    os: &'static str,
    request_id: String,
}

/// Inspectable synthetic envelope, not an HTTP request. There is no network
/// implementation or automatic authorization; this carries no Cookie.
pub struct FixtureRequest {
    receipt: Receipt,
    encrypted_form: String,
}

impl FixtureRequest {
    pub(super) fn encode(
        receipt: Receipt,
        uuid: &SessionUuid,
        fingerprint: &SealedFingerprint,
        context: &RequestContext,
        tick_count: u32,
    ) -> Result<Self> {
        if fingerprint.as_bytes().is_empty() || fingerprint.as_bytes().len() > MAX_FINGERPRINT_BYTES
        {
            return Err(Error::InvalidInput(
                "invalid sealed recognition fingerprint length",
            ));
        }
        let body = Body {
            algorithm_code: "shazam_v2",
            decrypt: 1,
            duration: receipt.duration_seconds(),
            from: "pc_back_discern",
            header: Header {
                appver: &context.app_version,
                client_sign: &context.client_sign,
                device_id: &context.device_id,
                os: OS,
                request_id: tick_count.to_string(),
            },
            rawdata: fingerprint.rawdata(),
            session_id: &uuid.0,
            times: receipt.times(),
        };
        let mut json = serde_json::to_string(&body)?;
        json.push('\n');
        Ok(Self {
            receipt,
            encrypted_form: format!("params={}", encrypt_json(CANONICAL_PATH, &json)?),
        })
    }

    pub fn receipt(&self) -> Receipt {
        self.receipt
    }

    pub fn encrypted_form(&self) -> &str {
        &self.encrypted_form
    }
}

pub(super) struct InterpretedResponse {
    pub native_hit: bool,
    pub song_ids: Vec<String>,
}

impl InterpretedResponse {
    pub(super) fn new(body: &str) -> Self {
        let mut result = Self {
            native_hit: false,
            song_ids: Vec::new(),
        };
        if body.len() > MAX_RESPONSE_BYTES {
            return result;
        }
        let Ok(value) = serde_json::from_str::<Value>(body) else {
            return result;
        };
        let data = &value["data"];
        // Native 30825A / 308341: body code and data.type form the match
        // signal before attempting to inspect result[0]. This is not the
        // bridge's separate code used to determine the resulting page state.
        result.native_hit = value["code"].as_i64() == Some(200) && data["type"].as_i64() == Some(1);
        if let Some(rows) = data["result"].as_array() {
            result.song_ids = rows
                .iter()
                .take(MAX_SONG_IDS)
                .filter_map(|row| song_id(&row["song"]["id"]))
                .collect();
        }
        result
    }
}

fn song_id(value: &Value) -> Option<String> {
    if let Some(number) = value.as_u64() {
        return (1..=MAX_JS_SAFE_INTEGER)
            .contains(&number)
            .then(|| number.to_string());
    }
    let text = value.as_str()?;
    // The official normalizer stringifies IDs; a decimal string is not
    // rounded or stripped of leading zeroes by a numeric conversion.
    (text.len() <= MAX_SONG_ID_BYTES
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && text.bytes().any(|byte| byte != b'0'))
    .then(|| text.to_owned())
}
