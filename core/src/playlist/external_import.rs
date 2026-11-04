//! The fixed desktop H5 link and text import requests. Both use Interface
//! WEAPI independent of the PC API transport and generic route selection.
use super::{
    ExternalPlaylistInput, external_input::selected_link, preview_external_playlist_inputs,
};
use crate::{
    Error, Result,
    account::credentials::{Credentials, RequestCredentials},
    protocol::{
        http::{
            ApiOrigin, BodyStatus, HttpReply, OFFICIAL_WINDOWS_USER_AGENT, RequestPlan, WireRoute,
        },
        wire::{serialization::encode_component, weapi},
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) const TASK_CREATE: &str = "/api/playlist/import/name/task/create";
const PAGE_ORIGIN: &str = "https://mp.music.163.com";
const TEXT_PREFIX: &str = "rpc://playlist/import?text=";
const MAX_TEXT_BYTES: usize = 16 * 1024;

pub(crate) struct TaskBatch {
    encoded: Vec<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SourcePreview {
    Links { links: Vec<ExternalPlaylistInput> },
    Text { bytes: usize },
}

fn encode_uri(input: &str) -> String {
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || b";,/?:@&=+$#-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(HEX[(b >> 4) as usize]));
            out.push(char::from(HEX[(b & 15) as usize]));
        }
    }
    out
}

pub(crate) fn links(inputs: &[String]) -> Result<(TaskBatch, SourcePreview)> {
    let links = preview_external_playlist_inputs(inputs)?;
    let mut encoded = Vec::new();
    for input in inputs.iter().filter(|input| !input.trim().is_empty()) {
        let (literal, parsed) =
            selected_link(input).ok_or(Error::InvalidInput("invalid playlist import link"))?;
        let host = parsed
            .domain()
            .ok_or(Error::InvalidInput("invalid playlist import host"))?
            .trim_end_matches('.');
        if parsed.port().is_some()
            || host.ends_with(".local")
            || host.ends_with(".localhost")
            || host.ends_with(".internal")
            || host.ends_with(".home.arpa")
            || literal.chars().any(char::is_control)
        {
            return Err(Error::InvalidInput("non-public playlist import link"));
        }
        encoded.push(encode_uri(literal));
    }
    if encoded.is_empty() {
        return Err(Error::InvalidInput(
            "enter at least one playlist import link",
        ));
    }
    Ok((TaskBatch { encoded }, SourcePreview::Links { links }))
}

pub(crate) fn text(raw: &str) -> Result<(TaskBatch, SourcePreview)> {
    let bytes = raw.len();
    if bytes == 0 || bytes > MAX_TEXT_BYTES {
        return Err(Error::InvalidInput(
            "external playlist text must contain 1 to 16384 UTF-8 bytes",
        ));
    }
    // H5:4590–4615 encodes the whole opaque RPC string once. Applying
    // a URL parser or encoding only the text as a query value changes it.
    let encoded = encode_uri(&format!("{TEXT_PREFIX}{raw}"));
    Ok((
        TaskBatch {
            encoded: vec![encoded],
        },
        SourcePreview::Text { bytes },
    ))
}

impl TaskBatch {
    pub(crate) fn payload(&self, name: &str) -> Result<Value> {
        let songs = self
            .encoded
            .iter()
            .map(|encoded| json!({"name":name,"type":"","url":encoded}))
            .collect::<Vec<_>>();
        Ok(json!({
            "playlistName":name,
            "taskIdForLog":"",
            "importStarPlaylist":false,
            "rqRefer":"",
            "songs":serde_json::to_string(&songs)?,
        }))
    }
}

pub(crate) struct PreparedRequest {
    pub plan: RequestPlan,
    revision: String,
}

impl PreparedRequest {
    pub fn revalidate(&self, credentials: &Credentials) -> Result<()> {
        let cookie = scoped_credentials(credentials)?;
        if self.revision != cookie.revision || cookie.unique_music_u().is_none() {
            return Err(Error::CredentialChanged);
        }
        Ok(())
    }
}

fn scoped_credentials(credentials: &Credentials) -> Result<RequestCredentials> {
    if credentials.origin() != ApiOrigin::Interface {
        return Err(Error::InvalidInput(
            "external import requires separate Interface credentials",
        ));
    }
    let target = ApiOrigin::Interface.request_url(WireRoute::Weapi, TASK_CREATE)?;
    credentials.request_credentials(&target)
}

pub(crate) fn request(
    credentials: &Credentials,
    batch: &TaskBatch,
    name: &str,
) -> Result<PreparedRequest> {
    let cookie = scoped_credentials(credentials)?;
    let csrf = credentials.external_import_document_csrf(&cookie.revision)?;
    let plan = encode(
        &cookie,
        csrf.as_deref(),
        batch.payload(name)?,
        weapi::encrypt,
    )?;
    Ok(PreparedRequest {
        plan,
        revision: cookie.revision,
    })
}

fn header_value(value: &str) -> bool {
    value
        .bytes()
        .all(|b| matches!(b, b'!' | b'#'..=b'+' | b'-'..=b':' | b'<'..=b'[' | b']'..=b'~'))
}

fn encode(
    cookie: &RequestCredentials,
    document_csrf: Option<&str>,
    mut payload: Value,
    encrypt: impl FnOnce(&Value) -> Result<weapi::Envelope>,
) -> Result<RequestPlan> {
    let mut url = ApiOrigin::Interface.request_url(WireRoute::Weapi, TASK_CREATE)?;
    if cookie.target != url || cookie.unique_music_u().is_none() {
        return Err(Error::State(
            "external import credentials are unavailable for the fixed URL",
        ));
    }
    let mut header_cookies = Vec::new();
    for (name, value) in &cookie.cookies {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.%".contains(&b))
            || !header_value(value)
        {
            return Err(Error::InvalidInput(
                "external import cookie cannot be sent safely",
            ));
        }
        header_cookies.push(format!("{name}={value}"));
    }
    if let Some(csrf) = document_csrf {
        if !header_value(csrf) {
            return Err(Error::InvalidInput(
                "external import document CSRF is invalid",
            ));
        }
        payload["csrf_token"] = csrf.into();
        url.set_query(Some(&format!("csrf_token={}", encode_component(csrf))));
    }
    let envelope = encrypt(&payload)?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("params", &envelope.params)
        .append_pair("encSecKey", &envelope.encrypted_key)
        .finish()
        .into_bytes();
    let headers = BTreeMap::from([
        (
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into(),
        ),
        ("Origin".into(), PAGE_ORIGIN.into()),
        ("Referer".into(), format!("{PAGE_ORIGIN}/")),
        ("User-Agent".into(), OFFICIAL_WINDOWS_USER_AGENT.into()),
        ("Cookie".into(), header_cookies.join("; ")),
    ]);
    Ok(RequestPlan {
        url,
        headers,
        body,
        route: WireRoute::Weapi,
        decrypt_response: false,
    })
}

pub(crate) fn acknowledgement(reply: &HttpReply) -> Option<String> {
    if reply.status != 200
        || reply.body_status != BodyStatus::Complete
        || reply.body.len() > 256 * 1024
    {
        return None;
    }
    let value: Value = serde_json::from_slice(&reply.body).ok()?;
    if !crate::error::code_matches(&value["code"], 200) {
        return None;
    }
    let data = value
        .get("data")
        .filter(|value| !value.is_null())
        .unwrap_or(&value);
    match data.get("taskId")? {
        Value::String(text) if valid_task_id(text) => Some(text.clone()),
        Value::Number(number) => {
            let text = crate::protocol::api::identifier(&Value::Number(number.clone())).ok()?;
            valid_task_id(&text).then_some(text)
        }
        _ => None,
    }
}

pub(crate) fn valid_task_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
        && id.bytes().any(|b| b.is_ascii_alphanumeric() && b != b'0')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn reply(status: u16, body_status: BodyStatus, body: Value) -> HttpReply {
        HttpReply {
            status,
            body_status,
            body: serde_json::to_vec(&body).unwrap(),
            set_cookies: vec![],
            refresh_token: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn fixed_h5_wire_matches_independent_cbc_rsa_and_js_encode_uri() {
        let vector: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/external_import_weapi.json"
        ))
        .unwrap();
        let inputs: Vec<String> = serde_json::from_value(vector["inputs"].clone()).unwrap();
        let (batch, preview) = links(&inputs).unwrap();
        let name = vector["name"].as_str().unwrap();
        assert!(matches!(preview, SourcePreview::Links { links } if links.len() == 4));
        let csrf = vector["csrf"].as_str().unwrap();
        let cookie = RequestCredentials {
            target: ApiOrigin::Interface
                .request_url(WireRoute::Weapi, TASK_CREATE)
                .unwrap(),
            device_id: "synthetic".into(),
            cookies: vec![
                ("MUSIC_U".into(), "synthetic-cookie".into()),
                ("__csrf".into(), csrf.into()),
            ],
            revision: "synthetic".into(),
        };
        let plan = encode(
            &cookie,
            Some(csrf),
            batch.payload(name).unwrap(),
            |payload| {
                assert_eq!(payload, &vector["payload"]);
                weapi::encrypt_with_key(
                    payload,
                    vector["key"]
                        .as_str()
                        .unwrap()
                        .as_bytes()
                        .try_into()
                        .unwrap(),
                )
            },
        )
        .unwrap();
        assert_eq!(
            plan.url.as_str(),
            "https://interface.music.163.com/weapi/playlist/import/name/task/create?csrf_token=fixture%2Bcsrf"
        );
        assert_eq!(plan.headers["Origin"], "https://mp.music.163.com");
        assert_eq!(plan.headers["Referer"], "https://mp.music.163.com/");
        assert_eq!(
            plan.headers["Cookie"],
            "MUSIC_U=synthetic-cookie; __csrf=fixture+csrf"
        );
        let pairs: BTreeMap<_, _> = url::form_urlencoded::parse(&plan.body)
            .into_owned()
            .collect();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs["params"], vector["params"]);
        assert_eq!(pairs["encSecKey"], vector["encSecKey"]);

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(crate::storage::StateStore::open(dir.path()).unwrap());
        let wrong = Credentials::open(store, ApiOrigin::Pc).unwrap();
        assert!(request(&wrong, &batch, name).is_err());
    }

    #[test]
    fn csrf_comes_from_the_fixed_h5_document_and_not_the_outgoing_cookie_list() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(crate::storage::StateStore::open(dir.path()).unwrap());
        let interface = Credentials::open(store, ApiOrigin::Interface).unwrap();
        let target = ApiOrigin::Interface
            .request_url(WireRoute::Weapi, TASK_CREATE)
            .unwrap();
        let set = |values: &[&str]| {
            let revision = interface.request_credentials(&target).unwrap().revision;
            interface
                .record_response(
                    &target,
                    &values
                        .iter()
                        .map(|value| (*value).into())
                        .collect::<Vec<_>>(),
                    None,
                    &revision,
                )
                .unwrap();
        };
        set(&[
            "MUSIC_U=fixture; Path=/; Secure; HttpOnly",
            "__csrf=interface-only; Path=/; Secure",
            "__csrf=shared-httponly; Domain=music.163.com; Path=/; Secure; HttpOnly",
        ]);
        let (batch, _) = links(&["https://y.qq.com/playlist/fixture".into()]).unwrap();
        let native = request(&interface, &batch, "合成歌单").unwrap();
        assert!(native.plan.headers["Cookie"].contains("interface-only"));
        assert!(native.plan.headers["Cookie"].contains("shared-httponly"));
        assert!(native.plan.url.query().is_none());
        set(&["__csrf=page-visible; Domain=music.163.com; Path=/; Secure"]);
        let native = request(&interface, &batch, "合成歌单").unwrap();
        assert_eq!(native.plan.url.query(), Some("csrf_token=page-visible"));
        assert!(native.plan.headers["Cookie"].contains("interface-only"));
        assert!(native.revalidate(&interface).is_ok());
        set(&["__csrf=changed; Domain=music.163.com; Path=/; Secure"]);
        assert!(native.revalidate(&interface).is_err());
    }

    #[test]
    fn invalid_private_inputs_and_partial_or_missing_receipts_are_not_accepted() {
        for input in [
            "http://user:pass@music.example/42",
            "http://127.0.0.1/x",
            "http://router.home.arpa/x",
            "https://music.example:8080/42",
        ] {
            assert!(links(&[input.into()]).is_err(), "{input}");
        }
        let valid = json!({"code":200,"data":{"taskId":"task-a_1"}});
        assert_eq!(
            acknowledgement(&reply(200, BodyStatus::Complete, valid.clone())).as_deref(),
            Some("task-a_1")
        );
        assert!(acknowledgement(&reply(503, BodyStatus::Complete, valid.clone())).is_none());
        assert!(acknowledgement(&reply(200, BodyStatus::Interrupted, valid)).is_none());
        for value in [
            json!({"code":200,"data":{},"taskId":"unbound"}),
            json!({"code":403,"data":{"taskId":"unbound"}}),
            json!({"code":200,"data":{"taskId":"0"}}),
            json!({"code":200,"data":{"taskId":{"id":"test"}}}),
        ] {
            assert!(acknowledgement(&reply(200, BodyStatus::Complete, value)).is_none());
        }
        assert_eq!(
            acknowledgement(&reply(
                200,
                BodyStatus::Complete,
                json!({"code":200,"taskId":9007199254740993_u64})
            ))
            .as_deref(),
            Some("9007199254740993")
        );
    }

    #[test]
    fn text_is_one_opaque_encode_uri_song_and_preview_does_not_echo_it() {
        let raw = "  第一首 / A&B=#?+ 50%\r\nSecond 🎵\t尾  ";
        let (batch, preview) = text(raw).unwrap();
        assert_eq!(
            serde_json::to_value(preview).unwrap(),
            json!({"kind":"text","bytes":44})
        );
        let payload = batch.payload("导入音乐 2026/11/4 12:34:56").unwrap();
        // Frozen from Node's encodeURI over H5:4605, with the full original text.
        let expected = "rpc://playlist/import?text=%20%20%E7%AC%AC%E4%B8%80%E9%A6%96%20/%20A&B=#?+%2050%25%0D%0ASecond%20%F0%9F%8E%B5%09%E5%B0%BE%20%20";
        let songs: Value = serde_json::from_str(payload["songs"].as_str().unwrap()).unwrap();
        assert_eq!(
            songs,
            json!([{"name":payload["playlistName"],"type":"","url":expected}])
        );
        assert_eq!(payload["importStarPlaylist"], false);
        assert!(text("").is_err());
        assert!(text(" \n").is_ok(), "official only checks nonempty input");
        assert!(text(&"x".repeat(MAX_TEXT_BYTES)).is_ok());
        assert!(text(&"歌".repeat(MAX_TEXT_BYTES / 3 + 1)).is_err());
    }
}
