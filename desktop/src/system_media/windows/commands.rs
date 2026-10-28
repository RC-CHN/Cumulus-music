use super::*;
use cumulus_core::{
    client::native_media::{NativeMediaAction as Action, NativeMediaAdjustment as Adjust},
    playback::PlaybackMode,
};
use windows::Media::{
    MediaPlaybackAutoRepeatMode as Repeat, SystemMediaTransportControlsButton as Button,
};
pub(super) enum Command {
    Button(Button),
    Seek(f64),
    Rate(f64),
    Shuffle(bool),
    Repeat(Repeat),
}
struct Pending(Arc<Host>);
impl Drop for Pending {
    fn drop(&mut self) {
        self.0.refresh_controls.store(true, Ordering::Release);
        self.0.busy.store(false, Ordering::Release);
        let _ = self.0.wake.try_send(());
    }
}
pub(super) fn submit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    host: &Arc<Host>,
    generation: u64,
    command: Command,
) {
    if host.stopped.load(Ordering::Acquire) || host.generation.load(Ordering::Acquire) != generation
    {
        return;
    }
    let Some(published) = host.published.lock().ok().and_then(|v| v.clone()) else {
        return;
    };
    let Ok(view) = app.state::<MusicClient>().native_media_details() else {
        return;
    };
    // Accept quick pause/play on the same visible item using its current fence,
    // but never retarget a command to an item/account not yet shown to the OS.
    if view.item.as_ref().map(|item| &item.id) != published.item.as_ref().map(|item| &item.id) {
        return;
    }
    let Some(target) = view.transport.target else {
        return;
    };
    if host
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let app = app.clone();
    let pending = Pending(host.clone());
    tauri::async_runtime::spawn(async move {
        let _pending = pending;
        if _pending.0.stopped.load(Ordering::Acquire)
            || _pending.0.generation.load(Ordering::Acquire) != generation
        {
            return;
        }
        let client = app.state::<MusicClient>();
        let action = match command {
            Command::Button(Button::Play) => Some(Action::Play),
            Command::Button(Button::Pause) => Some(Action::Pause),
            Command::Button(Button::Stop) => Some(Action::Stop),
            Command::Button(Button::Next) if view.transport.can_next => Some(Action::Next),
            Command::Button(Button::Previous) if view.transport.can_previous => {
                Some(Action::Previous)
            }
            _ => None,
        };
        let result = if let Some(action) = action {
            client.native_media_action(&target, action).await
        } else {
            let adjustment = match command {
                Command::Seek(seconds) if view.can_seek => Adjust::Seek { seconds },
                Command::Rate(value) => Adjust::Speed { value },
                Command::Shuffle(shuffle) if view.mode.is_some() => Adjust::Mode {
                    mode: if shuffle {
                        PlaybackMode::Random
                    } else if view.mode == Some(PlaybackMode::Random) {
                        PlaybackMode::OrderLoop
                    } else {
                        view.mode.unwrap()
                    },
                },
                Command::Repeat(repeat) if view.mode.is_some() => Adjust::Mode {
                    mode: match repeat {
                        Repeat::Track => PlaybackMode::SingleLoop,
                        Repeat::List if view.mode == Some(PlaybackMode::Random) => {
                            PlaybackMode::Random
                        }
                        Repeat::List => PlaybackMode::OrderLoop,
                        Repeat::None => PlaybackMode::Sequential,
                        _ => return,
                    },
                },
                _ => return,
            };
            client.native_media_adjust(&target, adjustment).await
        };
        if result.is_err() {
            // SMTC event delivery has no asynchronous business acknowledgement.
            // Publish actual core state afterward; never optimistically change it.
            eprintln!("Windows media command failed or its selection changed");
        }
    });
}
