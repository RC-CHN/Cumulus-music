//! M/21 validates the RN zone against the current native registry. M/3095
//! and M/3089 supply a tab display name, never an upstream tab key or URL.
use crate::protocol::{pc_experiments::PcExperimentGroups, wire::serialization::decode_component};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default)]
pub(crate) struct HotZoneContext<'a> {
    pub(crate) groups: Option<&'a PcExperimentGroups>,
    /// A current, verified registry, supplied only by a host whose destination
    /// is ready. Presence in an upstream search response is not registration.
    pub(crate) ready_pages: Option<&'a (dyn Fn(&str) -> bool + Sync)>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum SearchHotZoneSource {
    #[serde(rename = "searchHotExplain")]
    SearchHotExplain,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHotZoneTarget {
    pub page_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_key_name: Option<String>,
    pub from: SearchHotZoneSource,
}

pub(super) enum ZoneAction {
    Other,
    Blocked,
    Allowed(SearchHotZoneTarget),
}

pub(super) fn action(value: &str, context: HotZoneContext<'_>) -> ZoneAction {
    let Some(tail) = value
        .strip_prefix("orpheus://rnpage")
        .filter(|tail| tail.is_empty() || tail.starts_with(['/', '?', '&', '#']))
    else {
        return ZoneAction::Other;
    };
    if value.len() > 2048 || value.contains('#') || value.chars().any(char::is_control) {
        return ZoneAction::Blocked;
    }
    let Some(target) = target(tail, context) else {
        return ZoneAction::Blocked;
    };
    ZoneAction::Allowed(target)
}

fn target(tail: &str, context: HotZoneContext<'_>) -> Option<SearchHotZoneTarget> {
    let (id, query) = tail.split_at(tail.find(['?', '&'])?);
    if !id.is_empty() && !id.strip_prefix('/').is_some_and(valid_token) {
        return None;
    }
    let query = &query[1..];
    // M/21 removes the first '?' or '&'; URLSearchParams also consumes an
    // optional leading '?'. Its entries are copied into a last-value map.
    let query = query.strip_prefix('?').unwrap_or(query);
    let (mut component, mut page_code, mut tab_name) = (None, None, None);
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "component" => component = Some(value),
            "page" => page_code = Some(value),
            "tabKey" => tab_name = Some(value),
            _ => {}
        }
    }
    let page_code = page_code?.into_owned();
    if component.as_deref() != Some("cube-renderer-rn")
        || !valid_token(&page_code)
        || page_code.len() > 128
        || context
            .groups
            .and_then(|groups| groups.group("PH-PC-zhuanqu"))
            != Some("t1")
        || !context
            .ready_pages
            .is_some_and(|contains| contains(&page_code))
    {
        return None;
    }
    let tab_key_name = match tab_name.filter(|name| !name.is_empty()) {
        None => None,
        Some(name) => {
            // Click handler and destination each use forgiving URI decode.
            // The intervening route query serialization/parse is lossless.
            let name = decode_component(&name).unwrap_or_else(|_| name.into_owned());
            let name = decode_component(&name).unwrap_or(name);
            if name.len() > 256 || name.contains("://") || name.chars().any(char::is_control) {
                return None;
            }
            Some(name)
        }
    };
    Some(SearchHotZoneTarget {
        page_code,
        tab_key_name,
        from: SearchHotZoneSource::SearchHotExplain,
    })
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
