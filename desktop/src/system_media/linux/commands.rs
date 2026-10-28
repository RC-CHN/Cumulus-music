use super::*;
use cumulus_core::{
    client::native_media::{NativeMediaAction as Action, NativeMediaAdjustment as Adjust},
    playback::{PlaybackMode, Step},
};
use gio::glib::variant::ObjectPath;

pub(super) enum Command {
    Play,
    Transport(Action),
    Step(Step),
    Adjust(Adjust),
}
pub(super) struct Failure(pub &'static str, pub &'static str);
pub(super) const INVALID: Failure = Failure(
    "org.freedesktop.DBus.Error.InvalidArgs",
    "Invalid media argument",
);
pub(super) const UNSUPPORTED: Failure = Failure(
    "org.freedesktop.DBus.Error.NotSupported",
    "This media operation is not supported",
);
pub(super) fn method(
    view: &NativeMediaDetails,
    name: &str,
    value: &glib::Variant,
) -> Result<Option<Command>, Failure> {
    Ok(match name {
        "Play" if !view.transport.playing && view.transport.target.is_some() => Some(Command::Play),
        "PlayPause" if view.transport.target.is_some() => Some(if view.transport.playing {
            Command::Transport(Action::Pause)
        } else {
            Command::Play
        }),
        "Pause" if view.transport.playing => Some(Command::Transport(Action::Pause)),
        "Stop" if view.state != NativeMediaStatus::Stopped => {
            Some(Command::Transport(Action::Stop))
        }
        "Next" if view.transport.can_next => Some(Command::Step(Step::Next)),
        "Previous" if view.transport.can_previous => Some(Command::Step(Step::Previous)),
        "Seek" => {
            let (offset,) = value.get::<(i64,)>().ok_or(INVALID)?;
            if !view.can_seek {
                return Ok(None);
            }
            let position = (view.position.unwrap_or(view.seek_start) + offset as f64 / 1_000_000.0)
                .max(view.seek_start);
            if view.seek_end.is_some_and(|end| position > end) {
                view.transport.can_next.then_some(Command::Step(Step::Next))
            } else {
                Some(Command::Adjust(Adjust::Seek { seconds: position }))
            }
        }
        "SetPosition" => {
            let (id, position) = value.get::<(ObjectPath, i64)>().ok_or(INVALID)?;
            let seconds = position as f64 / 1_000_000.0;
            if !view.can_seek
                || id != properties::track_id(view)
                || position < 0
                || seconds < view.seek_start
                || view.seek_end.is_some_and(|end| seconds > end)
            {
                return Ok(None);
            }
            Some(Command::Adjust(Adjust::Seek { seconds }))
        }
        "OpenUri" => return Err(UNSUPPORTED),
        "Play" | "PlayPause" | "Pause" | "Stop" | "Next" | "Previous" => None,
        _ => {
            return Err(Failure(
                "org.freedesktop.DBus.Error.UnknownMethod",
                "Unknown media method",
            ));
        }
    })
}
pub(super) fn set(
    view: &NativeMediaDetails,
    name: &str,
    value: &glib::Variant,
) -> Result<Option<Command>, Failure> {
    let command = match name {
        "Volume" => {
            let volume = value
                .get::<f64>()
                .filter(|n| n.is_finite())
                .ok_or(INVALID)?;
            Command::Adjust(Adjust::Volume {
                value: volume.clamp(0.0, 1.0) * 100.0,
            })
        }
        "Rate" => {
            let rate = value
                .get::<f64>()
                .filter(|n| n.is_finite())
                .ok_or(INVALID)?;
            if rate == 0.0 {
                return Ok(view
                    .transport
                    .playing
                    .then_some(Command::Transport(Action::Pause)));
            }
            if !(0.25..=4.0).contains(&rate) {
                return Err(INVALID);
            }
            Command::Adjust(Adjust::Speed { value: rate })
        }
        "Shuffle" => {
            let shuffle = value.get::<bool>().ok_or(INVALID)?;
            let current = view.mode.ok_or(UNSUPPORTED)?;
            Command::Adjust(Adjust::Mode {
                mode: if shuffle {
                    PlaybackMode::Random
                } else if current == PlaybackMode::Random {
                    PlaybackMode::OrderLoop
                } else {
                    current
                },
            })
        }
        "LoopStatus" => {
            let mode = match value.str().ok_or(INVALID)? {
                "None" => PlaybackMode::Sequential,
                "Track" => PlaybackMode::SingleLoop,
                "Playlist" if view.mode == Some(PlaybackMode::Random) => PlaybackMode::Random,
                "Playlist" => PlaybackMode::OrderLoop,
                _ => return Err(INVALID),
            };
            view.mode.ok_or(UNSUPPORTED)?;
            Command::Adjust(Adjust::Mode { mode })
        }
        name if properties::player(view).contains_key(name) => {
            return Err(Failure(
                "org.freedesktop.DBus.Error.PropertyReadOnly",
                "Read-only media property",
            ));
        }
        _ => {
            return Err(Failure(
                "org.freedesktop.DBus.Error.UnknownProperty",
                "Unknown media property",
            ));
        }
    };
    Ok(Some(command))
}
pub(super) async fn execute(
    client: &MusicClient,
    target: &cumulus_core::client::native_media::NativeMediaTarget,
    command: Command,
) -> cumulus_core::Result<()> {
    match command {
        Command::Play => client.native_media_play(target).await,
        Command::Transport(action) => client.native_media_action(target, action).await,
        Command::Step(step) => client.native_media_step(target, step).await,
        Command::Adjust(adjustment) => client.native_media_adjust(target, adjustment).await,
    }
}
