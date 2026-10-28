//! Only touches a session with this host process's unique synthetic title.
//! Invoked as a separate bounded process by smtc_smoke.py, not in the host.
use serde_json::{Value, json};
use windows::{
    Media::{Control::*, MediaPlaybackAutoRepeatMode as Repeat},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::Result,
};
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: process-main guard; all COM objects have already dropped.
        unsafe {
            RoUninitialize();
        }
    }
}
pub fn run(prefix: &str, command: &str) -> Result<()> {
    assert!(prefix.starts_with("CumulusSMTCProof-") && prefix.ends_with(": "));
    // SAFETY: this dedicated process has not initialized a GUI or another apartment.
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED)?;
    }
    let _apartment = Apartment;
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;
    let mut selected = None;
    for session in manager.GetSessions()? {
        let title = session
            .TryGetMediaPropertiesAsync()?
            .get()?
            .Title()?
            .to_string();
        if title.starts_with(prefix) {
            assert!(selected.is_none(), "ambiguous synthetic media session");
            selected = Some(session);
        }
    }
    let value = if let Some(session) = selected {
        if command == "snapshot" {
            snapshot(&session)?
        } else {
            let delivered = match command {
                "play" => session.TryPlayAsync()?.get()?,
                "pause" => session.TryPauseAsync()?.get()?,
                "stop" => session.TryStopAsync()?.get()?,
                "next" => session.TrySkipNextAsync()?.get()?,
                "previous" => session.TrySkipPreviousAsync()?.get()?,
                "seek" => session.TryChangePlaybackPositionAsync(40_000_000)?.get()?,
                "rate" => session.TryChangePlaybackRateAsync(1.5)?.get()?,
                "normal-rate" => session.TryChangePlaybackRateAsync(1.0)?.get()?,
                "shuffle" => session.TryChangeShuffleActiveAsync(true)?.get()?,
                "repeat" => session.TryChangeAutoRepeatModeAsync(Repeat::Track)?.get()?,
                _ => panic!("unknown synthetic consumer action"),
            };
            json!({"found":true,"delivered":delivered})
        }
    } else {
        json!({"found":false})
    };
    println!("{value}");
    Ok(())
}
fn snapshot(session: &GlobalSystemMediaTransportControlsSession) -> Result<Value> {
    let media = session.TryGetMediaPropertiesAsync()?.get()?;
    let state = session.GetPlaybackInfo()?;
    let timeline = session.GetTimelineProperties()?;
    let controls = state.Controls()?;
    Ok(json!({
        "found":true,"title":media.Title()?.to_string(),"artist":media.Artist()?.to_string(),
        "album":media.AlbumTitle()?.to_string(),"status":state.PlaybackStatus()?.0,
        "rate":state.PlaybackRate()?.Value()?,"shuffle":state.IsShuffleActive()?.Value()?,
        "repeat":state.AutoRepeatMode()?.Value()?.0,
        "start":timeline.StartTime()?.Duration,"end":timeline.EndTime()?.Duration,
        "min":timeline.MinSeekTime()?.Duration,"max":timeline.MaxSeekTime()?.Duration,
        "position":timeline.Position()?.Duration,
        "canPlay":controls.IsPlayEnabled()?,"canPause":controls.IsPauseEnabled()?,
        "canNext":controls.IsNextEnabled()?,"canPrevious":controls.IsPreviousEnabled()?
    }))
}
