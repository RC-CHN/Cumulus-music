//! Native-only authority for the synthetic lyric bounty host.
use cumulus_core::{
    Error, Result as CoreResult,
    client::{ClientError, LyricBountyIntent, MusicClient},
    lyrics::{LyricBountyKind, LyricBountyStatus},
    network_diagnostics::DefaultRoute,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt::Write,
    time::{Duration, Instant},
};
use tauri::Manager;
use tokio::sync::Mutex;

const CONFIRMATION_LIFETIME: Duration = Duration::from_secs(60);

/// Installed by the sealed WebView proof only. The product app never manages
/// this state; there is no environment, setting or renderer opt-in.
#[doc(hidden)]
#[derive(Default)]
pub struct LyricBountyProofHost(Mutex<Option<Prepared>>);

struct Prepared {
    id: String,
    since: Instant,
    intent: LyricBountyIntent,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricBountyAction {
    MissingLyrics,
    Scrolling,
    Translation,
}

impl From<LyricBountyAction> for LyricBountyKind {
    fn from(action: LyricBountyAction) -> Self {
        match action {
            LyricBountyAction::MissingLyrics => Self::MissingLyrics,
            LyricBountyAction::Scrolling => Self::Scrolling,
            LyricBountyAction::Translation => Self::Translation,
        }
    }
}

#[derive(Serialize)]
pub struct LyricBountyOpening {
    id: Option<String>,
    status: Option<LyricBountyStatus>,
}

fn host<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Option<tauri::State<'_, LyricBountyProofHost>> {
    (window.label() == "main")
        .then(|| window.try_state::<LyricBountyProofHost>())
        .flatten()
}

fn require_host<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> CoreResult<tauri::State<'_, LyricBountyProofHost>> {
    host(window).ok_or(Error::State("lyric bounty confirmation unavailable"))
}

fn online<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> bool {
    crate::network_diagnostics::observed_default_route(window) == DefaultRoute::Present
}

#[tauri::command]
pub async fn lyric_bounty_actions<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<Vec<LyricBountyAction>, ClientError> {
    if host(&window).is_none() {
        return Ok(vec![]);
    }
    let mut actions = Vec::new();
    for action in [
        LyricBountyAction::MissingLyrics,
        LyricBountyAction::Scrolling,
        LyricBountyAction::Translation,
    ] {
        // Uses the current native lyric-read receipt and journal only.
        if client
            .prepare_lyric_bounty(&epoch, &play_id, action.into())
            .await
            .is_ok()
        {
            actions.push(action);
        }
    }
    Ok(actions)
}

#[tauri::command]
pub async fn lyric_bounty_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    action: LyricBountyAction,
) -> Result<LyricBountyOpening, ClientError> {
    let host = require_host(&window)?;
    let mut pending = host.0.lock().await;
    pending.take();
    let intent = client
        .prepare_lyric_bounty(&epoch, &play_id, action.into())
        .await?;
    let status = intent.existing_status();
    if matches!(
        status,
        Some(LyricBountyStatus::Unknown | LyricBountyStatus::Accepted)
    ) {
        return Ok(LyricBountyOpening { id: None, status });
    }
    if !online(&window) {
        return Ok(LyricBountyOpening {
            id: None,
            status: Some(LyricBountyStatus::NotSent),
        });
    }
    let mut id = String::with_capacity(32);
    for byte in cumulus_core::protocol::wire::crypto::random_bytes::<16>()? {
        write!(&mut id, "{byte:02x}").expect("formatting a String cannot fail");
    }
    *pending = Some(Prepared {
        id: id.clone(),
        since: Instant::now(),
        intent,
    });
    Ok(LyricBountyOpening {
        id: Some(id),
        status,
    })
}

#[tauri::command]
pub async fn lyric_bounty_confirm<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    id: String,
) -> Result<LyricBountyStatus, ClientError> {
    let host = require_host(&window)?;
    let prepared = {
        let mut pending = host.0.lock().await;
        if pending.as_ref().is_none_or(|prepared| prepared.id != id) {
            return Err(Error::StaleResponse.into());
        }
        pending.take().ok_or(Error::StaleResponse)?
    };
    // A default route is an additional positive native signal; the intent
    // separately requires a successful current signed lyric-read receipt.
    if prepared.since.elapsed() >= CONFIRMATION_LIFETIME || !online(&window) {
        return Ok(LyricBountyStatus::NotSent);
    }
    client
        .confirm_lyric_bounty(prepared.intent)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn lyric_bounty_cancel<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    id: String,
) -> Result<(), ClientError> {
    let host = require_host(&window)?;
    let mut pending = host.0.lock().await;
    if pending.as_ref().is_some_and(|prepared| prepared.id == id) {
        pending.take();
    }
    Ok(())
}
