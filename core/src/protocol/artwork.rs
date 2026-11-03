//! Public image URLs projected to the desktop without upstream query tokens.
use serde_json::Value;

fn public_url(value: &Value) -> Option<url::Url> {
    let mut url = url::Url::parse(value.as_str()?).ok()?;
    let host = url.host_str()?;
    if !["music.126.net", "music.163.com"]
        .iter()
        .any(|suffix| host.ends_with(&format!(".{suffix}")))
        || !["http", "https"].contains(&url.scheme())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    url.set_scheme("https").ok()?;
    url.set_query(None);
    url.set_fragment(None);
    Some(url)
}

pub(crate) fn original(value: &Value) -> Option<String> {
    Some(public_url(value)?.into())
}

pub(crate) fn square(value: &Value) -> Option<String> {
    let mut url = public_url(value)?;
    url.set_query(Some("param=320y320"));
    Some(url.into())
}
