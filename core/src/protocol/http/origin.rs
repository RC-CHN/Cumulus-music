use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiOrigin {
    #[serde(rename = "https://interface.music.163.com")]
    Interface,
    #[serde(rename = "https://interfacepc.music.163.com")]
    Pc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireRoute {
    Eapi,
    Weapi,
}

/// Native launch/verification policy, chosen before any request is prepared.
/// It is never changed in response to a rejected or uncertain mutation.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum RoutingPolicy {
    #[default]
    VerifiedCompatibility,
    /// Verified PC playlist workflow, with a native prepared context. Other
    /// legacy writes retain their preselected compatibility route.
    OfficialPcPlaylists,
    OfficialEapi,
}

impl ApiOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interface => "https://interface.music.163.com",
            Self::Pc => "https://interfacepc.music.163.com",
        }
    }
    pub fn url(self) -> Url {
        Url::parse(self.as_str()).expect("compiled API origin")
    }
    pub fn request_url(self, route: WireRoute, path: &str) -> Result<Url> {
        validate_path(path)?;
        let prefix = match route {
            WireRoute::Eapi => "/eapi/",
            WireRoute::Weapi => "/weapi/",
        };
        Url::parse(&format!("{}{prefix}{}", self.as_str(), &path[5..]))
            .map_err(|_| Error::InvalidInput("invalid API URL"))
    }
    pub fn require_url(self, url: &Url) -> Result<()> {
        if url.origin() != self.url().origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::InvalidInput("credential origin mismatch"));
        }
        Ok(())
    }
}

pub fn validate_path(path: &str) -> Result<()> {
    if !path.starts_with("/api/")
        || path.len() <= 5
        || path.contains("//")
        || !path
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/_-.".contains(&c))
        || path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
    {
        return Err(Error::InvalidInput("expected a normalized API path"));
    }
    Ok(())
}

/// Decide before sending. A rejected mutation must never choose another route.
pub fn select_route(origin: ApiOrigin, path: &str, payload: &Value) -> Result<WireRoute> {
    select_route_with_policy(origin, path, payload, RoutingPolicy::VerifiedCompatibility)
}

pub fn select_route_with_policy(
    origin: ApiOrigin,
    path: &str,
    payload: &Value,
    policy: RoutingPolicy,
) -> Result<WireRoute> {
    validate_path(path)?;
    let fields = payload
        .as_object()
        .ok_or(Error::InvalidInput("request payload must be an object"))?;
    const WRITES: &[&str] = &[
        "/api/playlist/create",
        "/api/playlist/update/name",
        "/api/playlist/desc/update",
        "/api/v1/playlist/manipulate/tracks",
        "/api/playlist/delete",
        // Official business fields; same-PC-origin compatibility established
        // in recovered product reports 27 and 30, chosen before any write.
        "/api/playlist/order/update",
        "/api/playlist/subscribe",
        "/api/playlist/unsubscribe",
        "/api/user/setting/update",
    ];
    const CHILDREN: &[&str] = &[
        "/api/playlist/update/name",
        "/api/playlist/tags/update",
        "/api/playlist/desc/update",
    ];
    let metadata_batch = path == "/api/batch"
        && fields.len() == CHILDREN.len()
        && CHILDREN.iter().all(|p| fields.contains_key(*p));
    if policy == RoutingPolicy::OfficialPcPlaylists && official_pc_playlist(path, payload) {
        return Ok(WireRoute::Eapi);
    }
    Ok(
        if policy != RoutingPolicy::OfficialEapi
            && origin == ApiOrigin::Pc
            && (WRITES.contains(&path) || metadata_batch)
        {
            WireRoute::Weapi
        } else {
            WireRoute::Eapi
        },
    )
}

pub(super) fn official_pc_playlist(path: &str, payload: &Value) -> bool {
    matches!(
        path,
        "/api/playlist/create"
            | "/api/v1/playlist/manipulate/tracks"
            | "/api/playlist/delete"
            | "/api/playlist/order/update"
    ) || (path == "/api/batch"
        && payload.as_object().is_some_and(|fields| {
            fields.len() == 3
                && [
                    "/api/playlist/update/name",
                    "/api/playlist/tags/update",
                    "/api/playlist/desc/update",
                ]
                .iter()
                .all(|key| fields.contains_key(*key))
        }))
}
