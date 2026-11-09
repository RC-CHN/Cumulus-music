//! Both native transports commit through the same ready-player authority.
use cumulus_core::{Error, client::MusicClient, playback::engine::Cancellation};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{AppHandle, Manager, Runtime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Reply {
    Accepted = 0,
    Unavailable = 2,
    Invalid = 3,
    Unknown = 4,
    Busy = 5,
}
impl Reply {
    pub(crate) fn finish(self) -> ! {
        if self != Self::Accepted {
            eprintln!(
                "{}",
                match self {
                    Self::Unavailable =>
                        "Cumulus cannot hand off to the existing app profile; try when its main player is ready.",
                    Self::Invalid =>
                        "Cumulus could not open a readable MP3, FLAC or supported PCM WAV file.",
                    Self::Unknown =>
                        "Cumulus did not confirm the handoff. Check the existing player before opening the files again.",
                    Self::Busy => "Cumulus is already processing an explicit file open.",
                    Self::Accepted => unreachable!(),
                }
            );
        }
        std::process::exit(self as i32);
    }
}

pub(super) fn ready_epoch<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    app.get_webview_window("main")?;
    let account = app.try_state::<MusicClient>()?.account().ok()?;
    app.try_state::<crate::player::PlayerStreams>()?
        .ready_for_native_open(&account.epoch)
        .then_some(account.epoch)
}

pub(super) fn dispatch<R: Runtime>(
    app: &AppHandle<R>,
    paths: Vec<PathBuf>,
    stopped: Arc<AtomicBool>,
    expected_epoch: Option<&str>,
) -> Reply {
    let Some(epoch) = ready_epoch(app) else {
        return Reply::Unavailable;
    };
    if stopped.load(Ordering::Acquire) || expected_epoch.is_some_and(|expected| expected != epoch) {
        return Reply::Unavailable;
    }
    let Some(client) = app.try_state::<MusicClient>() else {
        return Reply::Unavailable;
    };
    let alive = stopped.clone();
    let authority = Cancellation::new(move || !alive.load(Ordering::Acquire));
    match tauri::async_runtime::block_on(
        client.play_native_open_files_warm(&epoch, paths, authority),
    ) {
        Ok(_) => {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                if !stopped.load(Ordering::Acquire) {
                    crate::lifecycle::show(&handle);
                }
            });
            Reply::Accepted
        }
        Err(error) => match error.root() {
            Error::StaleResponse | Error::Cancelled | Error::QueueChanged => Reply::Unavailable,
            Error::InvalidInput(_) => Reply::Invalid,
            _ => Reply::Unknown,
        },
    }
}
