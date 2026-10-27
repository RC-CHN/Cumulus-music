//! Explicit, read-only import of the archived client's session format.
//! Netscape domain flags are authoritative; no Cookie is widened to a parent.
use crate::{Error, Result, protocol::http::ApiOrigin};
use cookie_store::{Cookie, CookieExpiration};
use serde_json::Value;
use std::{fs, io::Read, path::Path};
use time::OffsetDateTime;
use url::Url;

pub struct LegacySession {
    pub(super) origin: ApiOrigin,
    pub(super) device_id: String,
    pub(super) cookies: Vec<Cookie<'static>>,
    pub(super) refresh_token: Option<String>,
}

fn read_bounded(path: &Path, limit: u64) -> Result<String> {
    // The import is explicitly selected by the host; never traverse a final
    // symlink to a credential file or accept a non-regular file.
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(Error::InvalidInput(
            "invalid or oversized legacy credential file",
        ));
    }
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(Error::InvalidInput("legacy credential file exceeds limit"));
    }
    String::from_utf8(bytes).map_err(|_| Error::InvalidInput("legacy credential file is not UTF-8"))
}

impl LegacySession {
    pub fn read(directory: &Path, origin: ApiOrigin) -> Result<Self> {
        let identity: Value =
            serde_json::from_str(&read_bounded(&directory.join("device.json"), 32768)?)?;
        let device_id = identity["deviceId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 4096 && !s.contains(['\r', '\n', '\0']))
            .ok_or(Error::InvalidInput("invalid legacy device ID"))?
            .to_owned();
        let cookies = parse_netscape(
            &read_bounded(&directory.join("cookies.txt"), 2 * 1024 * 1024)?,
            origin,
        )?;
        let refresh_token = match read_bounded(&directory.join("refresh-token.json"), 32768) {
            Ok(text) => {
                let value: Value = serde_json::from_str(&text)?;
                if value["host"].as_str() != Some(origin.as_str()) {
                    return Err(Error::InvalidInput("legacy refresh origin mismatch"));
                }
                let token = value["refresh_token"]
                    .as_str()
                    .filter(|s| {
                        !s.is_empty() && s.len() <= 16384 && !s.contains(['\r', '\n', '\0'])
                    })
                    .ok_or(Error::InvalidInput("invalid legacy refresh token"))?;
                Some(token.into())
            }
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            origin,
            device_id,
            cookies,
            refresh_token,
        })
    }
}

pub fn parse_netscape(text: &str, origin: ApiOrigin) -> Result<Vec<Cookie<'static>>> {
    if !text
        .lines()
        .take(3)
        .any(|line| line.contains("Netscape HTTP Cookie File") || line.contains("HTTP Cookie File"))
    {
        return Err(Error::InvalidInput("missing Netscape Cookie file header"));
    }
    let mut output = vec![];
    for raw in text.lines() {
        let (line, http_only) = raw
            .strip_prefix("#HttpOnly_")
            .map_or((raw, false), |line| (line, true));
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let columns: Vec<_> = line.split('\t').collect();
        if columns.len() != 7 {
            return Err(Error::InvalidInput("malformed Netscape Cookie row"));
        }
        let [domain, domain_flag, path, secure, expires, name, value] = columns.as_slice() else {
            unreachable!()
        };
        if !["TRUE", "FALSE"].contains(domain_flag)
            || !["TRUE", "FALSE"].contains(secure)
            || name.is_empty()
            || name.contains([';', '=', '\r', '\n', '\0'])
            || value.contains([';', '\r', '\n', '\0'])
            || !path.starts_with('/')
            || path.contains([';', '\r', '\n', '\0'])
        {
            return Err(Error::InvalidInput("invalid Netscape Cookie field"));
        }
        if *domain_flag == "FALSE" && domain.starts_with('.') {
            return Err(Error::InvalidInput(
                "host-only Cookie has a leading domain dot",
            ));
        }
        let host = domain.trim_start_matches('.');
        if host.is_empty()
            || !host
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
        {
            return Err(Error::InvalidInput("invalid Cookie domain"));
        }
        let request_url = Url::parse(&format!("https://{host}/"))
            .map_err(|_| Error::InvalidInput("invalid Cookie origin"))?;
        let mut cookie = format!("{name}={value}; Path={path}");
        if *domain_flag == "TRUE" {
            cookie.push_str(&format!("; Domain={domain}"))
        }
        if *secure == "TRUE" {
            cookie.push_str("; Secure")
        }
        if http_only {
            cookie.push_str("; HttpOnly")
        }
        let mut cookie = Cookie::parse(cookie, &request_url)
            .map_err(|_| Error::InvalidInput("invalid legacy Cookie"))?
            .into_owned();
        if !expires.is_empty() {
            let timestamp = expires
                .parse::<i64>()
                .map_err(|_| Error::InvalidInput("invalid legacy Cookie expiry"))?;
            let expiry = OffsetDateTime::from_unix_timestamp(timestamp)
                .map_err(|_| Error::InvalidInput("legacy Cookie expiry out of range"))?;
            cookie.expires = CookieExpiration::AtUtc(expiry);
        }
        if !cookie.is_expired() && cookie.domain.matches(&origin.url()) {
            output.push(cookie)
        }
    }
    Ok(output)
}
