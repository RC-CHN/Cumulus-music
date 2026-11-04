//! Read-only input recognition for the official external playlist link form.
use crate::{Error, Result};
use serde::Serialize;
use url::Url;

pub const MAX_EXTERNAL_PLAYLIST_LINKS: usize = 10;
const MAX_INPUT_BYTES: usize = 16 * 1024;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalPlaylistSource {
    Qq,
    Kugou,
    Kuwo,
    Xiaomi,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExternalPlaylistInput {
    Empty,
    Unrecognized,
    Link {
        host: String,
        source: Option<ExternalPlaylistSource>,
    },
}

fn link_tokens(input: &str) -> impl Iterator<Item = &str> {
    input.match_indices("://").filter_map(|(separator, _)| {
        let prefix = &input[..separator];
        let start = prefix
            .rfind(|c: char| !c.is_ascii_alphabetic())
            .map_or(0, |offset| {
                offset + prefix[offset..].chars().next().unwrap().len_utf8()
            });
        if start == separator {
            return None;
        }
        let token = &input[start..];
        Some(
            token
                .split_once(|c: char| c.is_whitespace() || c == '，')
                .map_or(token, |(token, _)| token),
        )
    })
}

fn valid_link(token: &str) -> Option<Url> {
    let url = Url::parse(token).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.domain()?.trim_end_matches('.').contains('.')
        && url.username().is_empty()
        && url.password().is_none()
        && !token.contains('\\'))
    .then_some(url)
}

fn family(host: &str, suffix: &str) -> bool {
    host == suffix
        || host
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn classify(input: &str) -> ExternalPlaylistInput {
    if input.trim().is_empty() {
        return ExternalPlaylistInput::Empty;
    }
    let first = link_tokens(input).next();
    // The official recognizer checks Xiaohongshu short links before the
    // general URI candidate. The preview validates the actual URL host.
    let xiaohongshu = link_tokens(input).find_map(|token| {
        let url = valid_link(token)?;
        (url.domain()? == "xhslink.com").then_some(url)
    });
    let Some(url) = xiaohongshu.or_else(|| first.and_then(valid_link)) else {
        return ExternalPlaylistInput::Unrecognized;
    };
    let host = url
        .domain()
        .expect("validated DNS host")
        .trim_end_matches('.');
    // These special URL paths are accepted by the official recognizer without
    // the generic source label.
    let special = (host == "m.kugou.com" && url.path().starts_with("/songlist/"))
        || (host == "h5app.kuwo.cn" && url.path() == "/m/bodian/collection.html");
    let source = if special {
        None
    } else if family(host, "qq.com") {
        Some(ExternalPlaylistSource::Qq)
    } else if family(host, "kugou.com") {
        Some(ExternalPlaylistSource::Kugou)
    } else if family(host, "kuwo.cn") {
        Some(ExternalPlaylistSource::Kuwo)
    } else if family(host, "xiami.com") {
        Some(ExternalPlaylistSource::Xiaomi)
    } else {
        None
    };
    ExternalPlaylistInput::Link {
        host: host.to_owned(),
        source,
    }
}

/// Classify manually pasted entries locally. Only the host and source label
/// cross the desktop boundary; no raw links, navigation, network or writes.
pub fn preview_external_playlist_inputs(inputs: &[String]) -> Result<Vec<ExternalPlaylistInput>> {
    if inputs.len() > MAX_EXTERNAL_PLAYLIST_LINKS {
        return Err(Error::InvalidInput(
            "at most ten playlist links can be previewed",
        ));
    }
    if inputs.iter().any(|input| input.len() > MAX_INPUT_BYTES) {
        return Err(Error::InvalidInput("playlist link input exceeds limit"));
    }
    Ok(inputs.iter().map(|input| classify(input)).collect())
}
