//! Main-document observations from the actual WebView browser runtime.
//! The observer can only rearm the existing count timer, never grant rights.
use super::Host;
use cumulus_core::{Error, Result, client::ClientError};
use serde::Deserialize;
use tauri::Manager;

pub(super) struct Binding {
    nonce: String,
    sequence: u32,
}

fn trusted_url(url: &tauri::Url, dev: Option<&tauri::Url>) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let bundled = matches!(
        (url.scheme(), url.host_str()),
        ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost"))
    ) && url.port().is_none();
    bundled || dev.is_some_and(|dev| url.origin() == dev.origin())
}

fn dev_origin<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<&tauri::Url> {
    if cfg!(feature = "custom-protocol") {
        None
    } else {
        app.config().build.dev_url.as_ref()
    }
}

fn nonce() -> Result<String> {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|_| Error::Entropy)?;
    Ok(random
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(super) fn attach<R: tauri::Runtime>(window: &tauri::Webview<R>, url: &tauri::Url) {
    if window.label() != "main" || !trusted_url(url, dev_origin(window.app_handle())) {
        return;
    }
    let Some(host) = window.try_state::<Host>() else {
        return;
    };
    let Ok(nonce) = nonce() else {
        return;
    };
    {
        let Ok(mut slot) = host.0.lock() else {
            return;
        };
        if slot.closed || slot.poller.is_none() || slot.browser.is_some() {
            return;
        }
        slot.browser = Some(Binding {
            nonce: nonce.clone(),
            sequence: 0,
        });
    }
    let script = include_str!("browser.js").replace("__CUMULUS_COUNT_NETWORK_NONCE__", &nonce);
    if window.eval(script).is_err() {
        detach(window.app_handle());
    }
}

pub(super) fn detach<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Host>()
        && let Ok(mut slot) = host.0.lock()
        && slot.browser.take().is_some()
        && let Some(poller) = slot.poller.as_ref()
    {
        let _ = poller.try_browser_lifecycle(None);
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    nonce: String,
    sequence: u32,
    initial: bool,
    online: bool,
}

#[tauri::command]
pub fn observe_message_connectivity<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    host: tauri::State<'_, Host>,
    report: Report,
) -> std::result::Result<(), ClientError> {
    if window.label() != "main"
        || report.nonce.len() != 32
        || !window
            .url()
            .is_ok_and(|url| trusted_url(&url, dev_origin(window.app_handle())))
    {
        return Err(Error::StaleResponse.into());
    }
    let mut slot = host.0.lock().map_err(|_| Error::LockPoisoned)?;
    let binding = slot.browser.as_ref().ok_or(Error::StaleResponse)?;
    if binding.nonce != report.nonce
        || binding.sequence.checked_add(1) != Some(report.sequence)
        || report.initial != (report.sequence == 1)
    {
        return Err(Error::StaleResponse.into());
    }
    let poller = slot.poller.as_ref().ok_or(Error::Cancelled)?;
    if report.initial {
        poller.try_browser_lifecycle(Some(report.online))?;
    } else {
        poller.try_observe_browser(report.online)?;
    }
    slot.browser.as_mut().ok_or(Error::StaleResponse)?.sequence = report.sequence;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::trusted_url;
    use tauri::Url;

    #[test]
    fn only_main_app_and_explicit_dev_origins_may_receive_the_page_binding() {
        let urls = [
            ("tauri://localhost/", true),
            ("http://tauri.localhost/", true),
            ("https://tauri.localhost/", true),
            ("https://tauri.localhost:1443/", false),
            ("https://tauri.localhost.example.invalid/", false),
            ("https://user@tauri.localhost/", false),
            ("https://music.163.com/", false),
            ("about:blank", false),
        ];
        for (url, allowed) in urls {
            assert_eq!(
                trusted_url(&Url::parse(url).unwrap(), None),
                allowed,
                "{url}"
            );
        }
        let dev = Url::parse("http://127.0.0.1:1420/").unwrap();
        assert!(trusted_url(&dev, Some(&dev)));
        assert!(!trusted_url(
            &Url::parse("http://127.0.0.1:1421/").unwrap(),
            Some(&dev)
        ));
    }
}
