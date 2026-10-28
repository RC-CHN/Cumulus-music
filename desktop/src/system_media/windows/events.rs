use super::*;
use windows::{Foundation::TypedEventHandler, Media::*, core::Result};
pub(super) enum Subscription {
    Button(i64),
    Position(i64),
    Rate(i64),
    Shuffle(i64),
    Repeat(i64),
}
impl Subscription {
    pub(super) fn remove(self, controls: &SystemMediaTransportControls) {
        let _ = match self {
            Self::Button(token) => controls.RemoveButtonPressed(token),
            Self::Position(token) => controls.RemovePlaybackPositionChangeRequested(token),
            Self::Rate(token) => controls.RemovePlaybackRateChangeRequested(token),
            Self::Shuffle(token) => controls.RemoveShuffleEnabledChangeRequested(token),
            Self::Repeat(token) => controls.RemoveAutoRepeatModeChangeRequested(token),
        };
    }
}
pub(super) fn install<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    host: &Arc<Host>,
    generation: u64,
    bridge: &mut Bridge,
) -> Result<()> {
    // Keep revocation tokens as each registration succeeds, so partial failures
    // still revoke every callback before controls/apartment destruction.
    macro_rules! subscribe {
        ($method:ident, $kind:ident, $args:ty, $read:ident, $convert:expr) => {{
            let app = app.clone();
            let host = Arc::downgrade(host);
            let token = bridge.controls.$method(&TypedEventHandler::<
                SystemMediaTransportControls,
                $args,
            >::new(move |_, args| {
                if let Some(host) = host.upgrade()
                    && let Some(args) = args.as_ref()
                {
                    let command = ($convert)(args.$read()?);
                    commands::submit(&app, &host, generation, command);
                }
                Ok(())
            }))?;
            bridge.subscriptions.push(Subscription::$kind(token));
        }};
    }
    subscribe!(
        ButtonPressed,
        Button,
        SystemMediaTransportControlsButtonPressedEventArgs,
        Button,
        commands::Command::Button
    );
    subscribe!(
        PlaybackPositionChangeRequested,
        Position,
        PlaybackPositionChangeRequestedEventArgs,
        RequestedPlaybackPosition,
        |value: windows::Foundation::TimeSpan| commands::Command::Seek(
            value.Duration as f64 / 10_000_000.0
        )
    );
    subscribe!(
        PlaybackRateChangeRequested,
        Rate,
        PlaybackRateChangeRequestedEventArgs,
        RequestedPlaybackRate,
        commands::Command::Rate
    );
    subscribe!(
        ShuffleEnabledChangeRequested,
        Shuffle,
        ShuffleEnabledChangeRequestedEventArgs,
        RequestedShuffleEnabled,
        commands::Command::Shuffle
    );
    subscribe!(
        AutoRepeatModeChangeRequested,
        Repeat,
        AutoRepeatModeChangeRequestedEventArgs,
        RequestedAutoRepeatMode,
        commands::Command::Repeat
    );
    Ok(())
}
