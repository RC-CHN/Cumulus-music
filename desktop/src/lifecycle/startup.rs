//! Window presentation for app-owned launch sources. This does not register
//! autostart or infer it from an absent argument.
use super::{Host, change, reveal};
use cumulus_core::{client::MusicClient, preferences::window::StartupMode};
use std::{ffi::OsString, time::Duration};
use tauri::Manager;

const TRAY_WAIT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaunchKind {
    Explicit,
    Automatic,
}
impl LaunchKind {
    pub(crate) fn from_args(mut args: impl Iterator<Item = OsString>) -> Self {
        if args.next().is_some_and(|arg| arg == "--autostart") && args.next().is_none() {
            Self::Automatic
        } else {
            Self::Explicit
        }
    }

    pub(crate) fn prepare<R: tauri::Runtime>(self, context: &mut tauri::Context<R>) {
        if self == Self::Automatic
            && let Some(window) = context
                .config_mut()
                .app
                .windows
                .iter_mut()
                .find(|w| w.label == "main")
        {
            // Decide after loading the device preference and registering the
            // tray, without mapping a foreground window in the meantime.
            window.visible = false;
        }
    }

    /// Called once on the GUI thread after the regular lifecycle is installed.
    pub(crate) fn finish<R: tauri::Runtime>(self, app: &tauri::AppHandle<R>) {
        if self != Self::Automatic {
            return;
        }
        if app.state::<MusicClient>().window_startup_mode().ok() != Some(StartupMode::Minisize) {
            reveal(app);
            return;
        }

        let host = app.state::<Host>();
        let Ok(mut state) = host.inner.lock() else {
            reveal(app);
            return;
        };
        if state.available() {
            drop(state);
            change(app, |state| state.view.hidden = true);
            return;
        }
        state.startup_pending = true;
        drop(state);

        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(TRAY_WAIT).await;
            let target = app.clone();
            let _ = app.run_on_main_thread(move || {
                let host = target.state::<Host>();
                let pending = host
                    .inner
                    .lock()
                    .is_ok_and(|mut state| std::mem::take(&mut state.startup_pending));
                if pending {
                    reveal(&target);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_standalone_launch_marker_is_automatic() {
        let mode = |args: &[&str]| LaunchKind::from_args(args.iter().map(OsString::from));
        assert_eq!(mode(&[]), LaunchKind::Explicit);
        assert_eq!(mode(&["--autostart"]), LaunchKind::Automatic);
        assert_eq!(mode(&["--autostart", "song.flac"]), LaunchKind::Explicit);
        assert_eq!(mode(&["song.flac", "--autostart"]), LaunchKind::Explicit);
        assert_eq!(
            mode(&["--cumulus-renderer-diagnostics"]),
            LaunchKind::Explicit
        );
    }
}
