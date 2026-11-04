//! XDG Autostart for the user. System entries are read only and never masked.
use super::{Status, invalid};
use std::{
    ffi::CString,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
};

const NAME: &str = "io.cumulus.music.desktop";

struct DirectoryLock(fs::File);

impl DirectoryLock {
    fn acquire(directory: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(directory)?;
        file.try_lock().map_err(io::Error::from)?;
        Ok(Self(file))
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        // A parallel fork can briefly hold the same open description until exec.
        // Release the lock explicitly instead of waiting for its last fd to close.
        let _ = self.0.unlock();
    }
}

pub(super) struct Registration {
    directory: PathBuf,
    other_directories: Vec<PathBuf>,
    bytes: Vec<u8>,
}
impl Registration {
    pub(super) fn new(exe: PathBuf) -> io::Result<Self> {
        let home = dirs::config_dir()
            .filter(|path| path.is_absolute())
            .ok_or_else(|| invalid("no absolute XDG configuration directory"))?;
        let mut system: Vec<_> = std::env::var_os("XDG_CONFIG_DIRS")
            .into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .filter(|path| path.is_absolute())
            .collect();
        if system.is_empty() {
            system.push("/etc/xdg".into());
        }
        Self::in_directories(&exe, home, system)
    }

    fn in_directories(exe: &Path, home: PathBuf, system: Vec<PathBuf>) -> io::Result<Self> {
        let executable = exe
            .to_str()
            .filter(|text| !text.contains(['=', '%']) && !text.chars().any(char::is_control))
            .ok_or_else(|| invalid("path cannot be represented in a desktop entry"))?;
        // The Desktop Entry value unescape happens before Exec argument quoting.
        let mut quoted = String::new();
        for ch in executable.chars() {
            if ['"', '`', '$', '\\'].contains(&ch) {
                quoted.push('\\');
            }
            quoted.push(ch);
        }
        let exec = quoted.replace('\\', "\\\\");
        let try_exec = executable.replace('\\', "\\\\");
        let bytes = format!(
            "[Desktop Entry]\nType=Application\nName=Cumulus Music\nExec=\"{exec}\" --autostart\n\
             TryExec={try_exec}\nTerminal=false\nX-Cumulus-Autostart-Version=1\n"
        )
        .into_bytes();
        let directory = home.join("autostart");
        let other_directories = system
            .into_iter()
            .map(|path| path.join("autostart"))
            .filter(|path| path != &directory)
            .collect();
        Ok(Self {
            directory,
            other_directories,
            bytes,
        })
    }

    pub(super) fn status(&self) -> io::Result<Status> {
        let local = self.directory.join(NAME);
        match fs::symlink_metadata(&self.directory) {
            Ok(info) if !info.is_dir() => return Ok(Status::External),
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
        if let Some(owned) = self.ownership(&local)? {
            return Ok(if owned {
                Status::Enabled
            } else {
                Status::External
            });
        }
        for directory in &self.other_directories {
            match fs::symlink_metadata(directory.join(NAME)) {
                Ok(_) => return Ok(Status::External),
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => return Err(e),
            }
        }
        Ok(Status::Disabled)
    }

    fn ownership(&self, path: &Path) -> io::Result<Option<bool>> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Ok(Some(false)),
            Err(e) => return Err(e),
        };
        let info = file.metadata()?;
        if !info.is_file() || info.len() != self.bytes.len() as u64 {
            return Ok(Some(false));
        }
        let mut actual = Vec::new();
        file.take(self.bytes.len() as u64 + 1)
            .read_to_end(&mut actual)?;
        Ok(Some(actual == self.bytes))
    }

    pub(super) fn change(&self, expected: Status, desired: Status) -> io::Result<()> {
        // Never create the user configuration directory on read or disable.
        if desired == Status::Enabled {
            if self.status()? != expected {
                return Err(invalid("autostart entry changed"));
            }
            fs::create_dir_all(&self.directory)?;
        }
        let lock = DirectoryLock::acquire(&self.directory)?;
        if self.status()? != expected {
            return Err(invalid("autostart entry changed"));
        }
        let path = self.directory.join(NAME);
        if desired == Status::Enabled {
            let mut file = tempfile::NamedTempFile::new_in(&self.directory)?;
            file.write_all(&self.bytes)?;
            file.as_file().sync_all()?;
            file.persist_noclobber(&path)?;
        } else {
            self.remove_owned(&path)?;
        }
        lock.0.sync_all()
    }

    fn remove_owned(&self, path: &Path) -> io::Result<()> {
        // Atomically detach the name, then check the actually captured object.
        // A third party can race our observation, so never unlink its file.
        let quarantine = tempfile::Builder::new()
            .prefix(".cumulus-autostart-recovery-")
            .tempdir_in(&self.directory)?;
        let captured = quarantine.path().join(NAME);
        fs::rename(path, &captured)?;
        if self.ownership(&captured).ok() == Some(Some(true)) {
            return quarantine.close();
        }
        if restore_without_replace(&captured, path).is_err() {
            // An external writer reused the name. Preserve the displaced
            // entry for recovery rather than replacing or deleting either.
            let _preserved = quarantine.keep();
        }
        Err(invalid("a different autostart entry was preserved"))
    }
}

fn restore_without_replace(source: &Path, target: &Path) -> io::Result<()> {
    let source = CString::new(source.as_os_str().as_bytes())?;
    let target = CString::new(target.as_os_str().as_bytes())?;
    // SAFETY: the paths are NUL-terminated and live through the call. Both
    // belong to this user's one XDG autostart directory.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_lock_releases_even_with_a_shared_descriptor() {
        let directory = tempfile::tempdir().unwrap();
        let lock = DirectoryLock::acquire(directory.path()).unwrap();
        let shared = lock.0.try_clone().unwrap();
        assert_eq!(
            DirectoryLock::acquire(directory.path())
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(lock);
        let _next = DirectoryLock::acquire(directory.path()).unwrap();
        drop(shared);
    }

    #[test]
    fn only_exact_user_entry_is_written_or_removed_and_system_entry_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path().join("user");
        let system = temp.path().join("system");
        let exe = Path::new("/opt/Cumulus \u{4e91}/cumulus");
        let entry = Registration::in_directories(exe, user, vec![system.clone()]).unwrap();
        assert_eq!(entry.status().unwrap(), Status::Disabled);
        assert!(!entry.directory.exists());
        entry.change(Status::Disabled, Status::Enabled).unwrap();
        let path = entry.directory.join(NAME);
        let exact = fs::read_to_string(&path).unwrap();
        assert!(exact.contains("Exec=\"/opt/Cumulus 云/cumulus\" --autostart\n"));
        assert_eq!(entry.status().unwrap(), Status::Enabled);

        fs::create_dir_all(system.join("autostart")).unwrap();
        let system_file = system.join("autostart").join(NAME);
        fs::write(&system_file, b"foreign system").unwrap();
        entry.change(Status::Enabled, Status::Disabled).unwrap();
        assert_eq!(entry.status().unwrap(), Status::External);
        assert!(!path.exists());
        assert_eq!(fs::read(&system_file).unwrap(), b"foreign system");
        assert!(entry.change(Status::Disabled, Status::Enabled).is_err());
        fs::write(&path, b"foreign").unwrap();
        assert!(entry.change(Status::Enabled, Status::Disabled).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"foreign");
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&system_file, &path).unwrap();
        assert_eq!(entry.status().unwrap(), Status::External);
        assert!(entry.change(Status::Enabled, Status::Disabled).is_err());
        assert!(path.is_symlink());
    }

    #[test]
    fn concurrent_foreign_capture_can_never_be_destroyed_or_overwrite_the_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let entry =
            Registration::in_directories(Path::new("/opt/cumulus"), temp.path().into(), vec![])
                .unwrap();
        fs::create_dir(&entry.directory).unwrap();
        let path = entry.directory.join(NAME);
        fs::write(&path, b"foreign raced the read").unwrap();
        assert!(entry.remove_owned(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"foreign raced the read");

        let captured = entry.directory.join("already-captured");
        fs::write(&captured, b"preserved foreign").unwrap();
        assert!(restore_without_replace(&captured, &path).is_err());
        assert_eq!(fs::read(&captured).unwrap(), b"preserved foreign");
        assert_eq!(fs::read(&path).unwrap(), b"foreign raced the read");
    }
}
