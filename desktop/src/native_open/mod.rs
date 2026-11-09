//! One cold process argument group. Nothing in the WebView can supply a path.
#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod warm;
#[cfg(target_os = "windows")]
pub(crate) mod windows;
#[cfg(target_os = "linux")]
pub(crate) use linux as platform;
#[cfg(target_os = "windows")]
pub(crate) use windows as platform;

use cumulus_core::{Error, client::ClientError};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Default)]
pub(crate) struct Pending {
    paths: Mutex<Option<Vec<PathBuf>>>,
    #[cfg(windows)]
    opening_epoch: std::sync::OnceLock<u64>,
}

impl Pending {
    pub(crate) fn from_args(mut args: impl Iterator<Item = OsString>) -> Self {
        let Some(first) = args.next() else {
            return Self::default();
        };
        let mut paths = if cfg!(target_os = "linux") && first == "--open-local-files" {
            // Desktop Entry %F disappears on a normal launcher click. Keep
            // that path distinct from an explicit invalid --play invocation.
            let Some(path) = args.next() else {
                return Self::default();
            };
            vec![PathBuf::from(path)]
        } else if first == "--play" {
            Vec::new()
        } else if first.as_encoded_bytes().starts_with(b"--play=") {
            let Some(path) = first.to_str().and_then(|text| text.strip_prefix("--play=")) else {
                // A lossless queue path is required, but the unsupported
                // explicit activation must still suppress old-queue autoplay.
                return Self::explicit(Vec::new());
            };
            vec![PathBuf::from(path)]
        } else {
            return Self::default();
        };
        paths.extend(args.take(9).map(PathBuf::from));
        // Consume a recognized invalid activation as well. Never interpret
        // another flag, a URI, or a relative path as a native file request.
        if paths.len() > 8 || paths.iter().any(|path| !path.is_absolute()) {
            paths.clear();
        }
        Self::explicit(paths)
    }

    fn explicit(paths: Vec<PathBuf>) -> Self {
        Self {
            paths: Mutex::new(Some(paths)),
            #[cfg(windows)]
            opening_epoch: std::sync::OnceLock::new(),
        }
    }

    pub(crate) fn requested(&self) -> bool {
        self.paths.lock().is_ok_and(|pending| pending.is_some())
    }

    pub(crate) fn take(&self) -> Option<Vec<PathBuf>> {
        self.paths.lock().ok()?.take()
    }

    #[cfg(windows)]
    pub(crate) fn take_for_epoch(&self, epoch: u64) -> Option<Vec<PathBuf>> {
        let mut pending = self.paths.lock().ok()?;
        if pending.is_none() || self.opening_epoch.set(epoch).is_err() {
            return None;
        }
        pending.take()
    }

    #[cfg(windows)]
    pub(crate) fn blocks_startup(&self, epoch: &str) -> bool {
        self.requested()
            || self
                .opening_epoch
                .get()
                .is_some_and(|current| epoch.parse::<u64>().ok() == Some(*current))
    }
}

/// Platform owners request the exclusive lease; a profile without a relay
/// may retain the shared fallback. The file itself holds no application data.
pub(crate) struct ProcessGuard {
    lock: File,
    #[cfg(windows)]
    windows_profile: Option<windows::Profile>,
}
impl ProcessGuard {
    pub(crate) fn acquire(root: &Path, exclusive: bool) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let path = root.join(".native-open-process.lock");
        if fs::symlink_metadata(&path).is_ok_and(|metadata| !metadata.is_file()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid application process lock",
            ));
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        Self::from_file(lock, exclusive)
    }

    fn from_file(lock: File, exclusive: bool) -> io::Result<Self> {
        if exclusive {
            lock.try_lock()
        } else {
            lock.try_lock_shared()
        }
        .map_err(io::Error::from)?;
        Ok(Self {
            lock,
            #[cfg(windows)]
            windows_profile: None,
        })
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        // A forked child can still hold the same file description until exec,
        // even with close-on-exec set. Release the lock before closing our fd.
        let _ = self.lock.unlock();
    }
}

pub(crate) fn public_error(error: Error) -> ClientError {
    if matches!(error.root(), Error::StaleResponse | Error::Cancelled) {
        return error.into();
    }
    ClientError {
        code: "native_file_open",
        message: "未能打开本地音频。请确认文件可读，并重新打开 MP3、FLAC 或受支持的 PCM WAV 文件。",
        retryable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_open_excludes_incompatible_handles_using_the_same_root() {
        let root = tempfile::tempdir().unwrap();
        let first = ProcessGuard::acquire(root.path(), false).unwrap();
        assert!(ProcessGuard::acquire(root.path(), false).is_ok());
        assert_eq!(
            ProcessGuard::acquire(root.path(), true)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(first);
        let active = ProcessGuard::acquire(root.path(), true).unwrap();
        assert_eq!(
            ProcessGuard::acquire(root.path(), false)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(active);
        assert!(ProcessGuard::acquire(root.path(), false).is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_open_releases_the_lock_while_an_inherited_descriptor_is_open() {
        let root = tempfile::tempdir().unwrap();
        let guard = ProcessGuard::acquire(root.path(), false).unwrap();
        // A duplicate pins the same Linux file description as an inherited fd
        // in a child before exec. Keeping it open makes the race deterministic.
        let inherited = guard.lock.try_clone().unwrap();
        drop(guard);
        let active = ProcessGuard::acquire(root.path(), true).unwrap();
        drop(inherited);
        assert_eq!(
            ProcessGuard::acquire(root.path(), false)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(active);
        assert!(ProcessGuard::acquire(root.path(), false).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn undecodable_explicit_path_is_not_an_ordinary_launch() {
        use std::os::unix::ffi::OsStringExt;

        let path = OsString::from_vec(b"--play=/local/\xff.flac".to_vec());
        let pending = Pending::from_args([path].into_iter());
        assert!(pending.requested());
        assert_eq!(pending.take(), Some(vec![]));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_entry_file_field_disappears_on_an_ordinary_launch() {
        let ordinary = Pending::from_args(["--open-local-files".into()].into_iter());
        assert!(!ordinary.requested());
        assert_eq!(ordinary.take(), None);
        let local = PathBuf::from("/tmp/合成 % \"音乐\".flac");
        let selection = Pending::from_args(
            ["--open-local-files".into(), local.clone().into_os_string()].into_iter(),
        );
        assert_eq!(selection.take(), Some(vec![local]));
        let uri = Pending::from_args(
            [
                "--open-local-files".into(),
                "https://example.invalid/music.mp3".into(),
            ]
            .into_iter(),
        );
        assert_eq!(uri.take(), Some(vec![]));
    }

    #[test]
    fn only_explicit_native_launch_paths_are_consumed_once() {
        let absolute = std::env::current_dir().unwrap().join("合成 音频.flac");
        let arg = format!("--play={}", absolute.to_str().unwrap());
        let pending = Pending::from_args([arg.into()].into_iter());
        assert_eq!(pending.take(), Some(vec![absolute]));
        assert_eq!(pending.take(), None);
        assert_eq!(
            Pending::from_args(["--autostart".into()].into_iter()).take(),
            None
        );
        for value in [
            "--play=relative.flac",
            "--play=https://example.invalid/remote.flac",
            "--play",
        ] {
            assert_eq!(
                Pending::from_args([value.into()].into_iter()).take(),
                Some(vec![])
            );
        }
    }
}
