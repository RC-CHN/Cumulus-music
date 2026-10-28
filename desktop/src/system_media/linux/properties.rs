use super::*;
use cumulus_core::playback::PlaybackMode;
use gio::glib::variant::ObjectPath;

pub(super) fn text(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(8192)
        .collect()
}
pub(super) fn track_id(view: &NativeMediaDetails) -> ObjectPath {
    let path = view
        .item
        .as_ref()
        .map(|item| format!("/io/cumulus/music/track/{}", item.id))
        .unwrap_or_else(|| "/org/mpris/MediaPlayer2/TrackList/NoTrack".into());
    ObjectPath::try_from(path).expect("core supplies a hex-only selection identity")
}
pub(super) fn micros(seconds: f64) -> i64 {
    if seconds.is_finite() {
        (seconds.max(0.0) * 1_000_000.0) as i64
    } else {
        0
    }
}
pub(super) fn position(view: &NativeMediaDetails) -> i64 {
    if view.state == NativeMediaStatus::Stopped {
        0
    } else {
        micros(view.position.unwrap_or(view.seek_start))
    }
}
pub(super) fn root() -> Properties {
    Properties::from([
        ("CanQuit".into(), false.to_variant()),
        ("CanRaise".into(), true.to_variant()),
        ("HasTrackList".into(), false.to_variant()),
        ("Identity".into(), "Cumulus".to_variant()),
        (
            "SupportedUriSchemes".into(),
            Vec::<String>::new().to_variant(),
        ),
        (
            "SupportedMimeTypes".into(),
            Vec::<String>::new().to_variant(),
        ),
    ])
}
pub(super) fn player(view: &NativeMediaDetails) -> Properties {
    let mut metadata = Properties::new();
    if let Some(item) = &view.item {
        metadata.insert("mpris:trackid".into(), track_id(view).to_variant());
        metadata.insert("xesam:title".into(), text(&item.title).to_variant());
        metadata.insert(
            "xesam:artist".into(),
            item.artists
                .iter()
                .take(64)
                .map(|a| text(a))
                .collect::<Vec<_>>()
                .to_variant(),
        );
        metadata.insert("xesam:album".into(), text(&item.album).to_variant());
        if let Some(duration) = view.duration {
            metadata.insert("mpris:length".into(), micros(duration).to_variant());
        }
        if let Some(url) = &item.artwork {
            metadata.insert("mpris:artUrl".into(), url.to_variant());
        }
    }
    let status = match view.state {
        NativeMediaStatus::Stopped => "Stopped",
        NativeMediaStatus::Paused => "Paused",
        NativeMediaStatus::Loading if !view.transport.playing => "Paused",
        _ => "Playing",
    };
    let looping = match view.mode {
        Some(PlaybackMode::SingleLoop) => "Track",
        Some(PlaybackMode::OrderLoop | PlaybackMode::Random) => "Playlist",
        _ => "None",
    };
    let selected = view.transport.target.is_some();
    Properties::from([
        ("PlaybackStatus".into(), status.to_variant()),
        ("LoopStatus".into(), looping.to_variant()),
        (
            "Shuffle".into(),
            (view.mode == Some(PlaybackMode::Random)).to_variant(),
        ),
        ("Rate".into(), view.speed.to_variant()),
        ("Volume".into(), (view.volume / 100.0).to_variant()),
        ("Metadata".into(), metadata.to_variant()),
        ("Position".into(), position(view).to_variant()),
        ("MinimumRate".into(), 0.25_f64.to_variant()),
        ("MaximumRate".into(), 4_f64.to_variant()),
        ("CanGoNext".into(), view.transport.can_next.to_variant()),
        (
            "CanGoPrevious".into(),
            view.transport.can_previous.to_variant(),
        ),
        ("CanPlay".into(), selected.to_variant()),
        ("CanPause".into(), selected.to_variant()),
        ("CanSeek".into(), view.can_seek.to_variant()),
        ("CanControl".into(), true.to_variant()),
    ])
}
