//! File transport for synthetic native-host observations, never business writes.
use std::{fs, io, path::Path};

pub fn write(path: &Path, value: &serde_json::Value) -> io::Result<()> {
    let pending = path.with_extension("pending");
    fs::write(&pending, serde_json::to_vec(value)?)?;
    replace(&pending, path)
}

fn replace(pending: &Path, path: &Path) -> io::Result<()> {
    #[cfg(not(target_os = "windows"))]
    {
        fs::rename(pending, path)
    }
    #[cfg(target_os = "windows")]
    {
        let started = std::time::Instant::now();
        loop {
            match fs::rename(pending, path) {
                Ok(()) => return Ok(()),
                Err(error)
                    if matches!(error.raw_os_error(), Some(5 | 32 | 33))
                        && started.elapsed() < std::time::Duration::from_secs(1) =>
                {
                    // Python's snapshot reader may temporarily hold an open handle
                    // without FILE_SHARE_DELETE. Retry only the local replacement;
                    // keep the old complete observation visible in the meantime.
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt, time::Duration};

    fn reader(path: &Path) -> fs::File {
        OpenOptions::new()
            .read(true)
            .share_mode(1) // FILE_SHARE_READ, matching the independent reader.
            .open(path)
            .unwrap()
    }

    #[test]
    fn replacement_waits_for_a_short_lived_independent_reader() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.json");
        fs::write(&path, b"{\"generation\":1}").unwrap();
        let held = reader(&path);
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            drop(held);
        });
        write(&path, &serde_json::json!({"generation":2})).unwrap();
        release.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"generation\":2}");
    }

    #[test]
    fn permanent_lock_is_reported_without_losing_the_last_complete_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.json");
        fs::write(&path, b"{\"generation\":1}").unwrap();
        let held = reader(&path);
        let started = std::time::Instant::now();
        assert!(write(&path, &serde_json::json!({"generation":2})).is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(fs::read(&path).unwrap(), b"{\"generation\":1}");
        drop(held);
        replace(&path.with_extension("pending"), &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"generation\":2}");
    }
}
