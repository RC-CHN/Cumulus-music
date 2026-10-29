use super::*;
use cumulus_core::{
    client::native_media::{NativeMediaKind, NativeMediaStatus},
    playback::PlaybackMode,
};
use windows::{
    Foundation::{TimeSpan, Uri},
    Media::*,
    Storage::Streams::RandomAccessStreamReference,
    core::{HSTRING, Result},
};
fn text(value: &str) -> HSTRING {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(8192)
        .collect::<String>()
        .into()
}
fn span(seconds: f64) -> TimeSpan {
    TimeSpan {
        Duration: if seconds.is_finite() {
            (seconds.max(0.0) * 10_000_000.0) as i64
        } else {
            0
        },
    }
}
fn status(view: &NativeMediaDetails) -> MediaPlaybackStatus {
    match view.state {
        NativeMediaStatus::Stopped => MediaPlaybackStatus::Stopped,
        NativeMediaStatus::Paused => MediaPlaybackStatus::Paused,
        NativeMediaStatus::Loading => MediaPlaybackStatus::Changing,
        NativeMediaStatus::Playing => MediaPlaybackStatus::Playing,
    }
}
pub(super) fn publish(
    bridge: &mut Bridge,
    view: &NativeMediaDetails,
    force_controls: bool,
) -> Result<()> {
    let old = bridge.previous.as_ref();
    let controls = &bridge.controls;
    // Enable before the initial metadata Update, as in the manual SMTC host
    // lifecycle. On publication failure Bridge::drop disables and clears it.
    if old.is_none() {
        controls.SetIsEnabled(true)?;
    }
    if old.is_none_or(|old| old.item != view.item) {
        let display = controls.DisplayUpdater()?;
        display.ClearAll()?;
        if let Some(item) = &view.item {
            display.SetAppMediaId(&item.id.clone().into())?;
            let artists = item
                .artists
                .iter()
                .take(64)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            match item.kind {
                NativeMediaKind::Music => {
                    display.SetType(MediaPlaybackType::Music)?;
                    let music = display.MusicProperties()?;
                    music.SetTitle(&text(&item.title))?;
                    music.SetArtist(&text(&artists))?;
                    music.SetAlbumTitle(&text(&item.album))?;
                }
                NativeMediaKind::Video => {
                    display.SetType(MediaPlaybackType::Video)?;
                    let video = display.VideoProperties()?;
                    video.SetTitle(&text(&item.title))?;
                    video.SetSubtitle(&text(&artists))?;
                }
            }
            // Only the core's public CDN whitelist reaches this boundary; no
            // cookies, signed audio URL, local path or original media file.
            if let Some(url) = &item.artwork
                && let Ok(uri) = Uri::CreateUri(&url.into())
                && let Ok(image) = RandomAccessStreamReference::CreateFromUri(&uri)
            {
                let _ = display.SetThumbnail(&image);
            }
        }
        display.Update()?;
    }
    if force_controls || old.is_none_or(|old| old.transport != view.transport) {
        controls.SetIsPlayEnabled(true)?;
        controls.SetIsPauseEnabled(true)?;
        controls.SetIsStopEnabled(true)?;
        controls.SetIsNextEnabled(view.transport.can_next)?;
        controls.SetIsPreviousEnabled(view.transport.can_previous)?;
    }
    if force_controls || old.is_none_or(|old| status(old) != status(view)) {
        controls.SetPlaybackStatus(status(view))?;
    }
    if force_controls || old.is_none_or(|old| old.speed != view.speed) {
        controls.SetPlaybackRate(view.speed)?;
    }
    if force_controls || old.is_none_or(|old| old.mode != view.mode) {
        controls.SetAutoRepeatMode(match view.mode {
            Some(PlaybackMode::SingleLoop) => MediaPlaybackAutoRepeatMode::Track,
            Some(PlaybackMode::OrderLoop | PlaybackMode::Random) => {
                MediaPlaybackAutoRepeatMode::List
            }
            _ => MediaPlaybackAutoRepeatMode::None,
        })?;
        controls.SetShuffleEnabled(view.mode == Some(PlaybackMode::Random))?;
    }
    // SMTC interpolates the native clock. Refresh at most every five seconds,
    // and immediately for transport, seek, bounds or rate changes.
    if force_controls
        || bridge.timeline_at.elapsed() >= Duration::from_secs(5)
        || old.is_none_or(|old| {
            old.transport != view.transport
                || old.seek_revision != view.seek_revision
                || old.seek_start != view.seek_start
                || old.seek_end != view.seek_end
                || old.duration != view.duration
                || old.speed != view.speed
                || old.state != view.state
        })
    {
        let position = if view.state == NativeMediaStatus::Stopped {
            view.seek_start
        } else {
            view.position.unwrap_or(view.seek_start)
        };
        let end = view
            .duration
            .unwrap_or(position)
            .max(position)
            .max(view.seek_start);
        let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
        timeline.SetStartTime(span(0.0))?;
        timeline.SetEndTime(span(end))?;
        timeline.SetPosition(span(position))?;
        timeline.SetMinSeekTime(span(if view.can_seek {
            view.seek_start
        } else {
            position
        }))?;
        timeline.SetMaxSeekTime(span(if view.can_seek {
            view.seek_end.unwrap_or(end).min(end)
        } else {
            position
        }))?;
        controls.UpdateTimelineProperties(&timeline)?;
        bridge.timeline_at = Instant::now();
    }
    bridge.previous = Some(view.clone());
    Ok(())
}
