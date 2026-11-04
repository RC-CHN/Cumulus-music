//! HKCU Run, one namespaced value. Compare and mutate in a native registry
//! transaction so the checked value cannot be replaced before deletion/write.
use super::{Status, invalid};
use std::{
    ffi::OsStr,
    io,
    os::windows::ffi::OsStrExt,
    path::{Component, Path, PathBuf, Prefix},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS, HANDLE,
        INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::{CommitTransaction, CreateTransaction},
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY,
        REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyTransactedW, RegDeleteValueW,
        RegOpenKeyExW, RegOpenKeyTransactedW, RegQueryValueExW, RegSetValueExW,
    },
};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "io.cumulus.music";
const MAX_COMMAND: usize = 260;
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: Key owns a successfully opened (non-predefined) HKEY.
        unsafe { RegCloseKey(self.0) };
    }
}
struct Transaction(HANDLE);
impl Transaction {
    fn create() -> io::Result<Self> {
        // SAFETY: all optional input pointers are null; no impersonation.
        let handle = unsafe { CreateTransaction(null_mut(), null_mut(), 0, 0, 0, 2500, null()) };
        if handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
    fn commit(&self) -> io::Result<()> {
        // SAFETY: the owned transaction handle remains valid.
        if unsafe { CommitTransaction(self.0) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        // SAFETY: closing an uncommitted transaction rolls it back.
        unsafe { CloseHandle(self.0) };
    }
}

pub(super) struct Registration {
    key: Vec<u16>,
    value: Vec<u16>,
    command: Vec<u16>,
}
impl Registration {
    pub(super) fn new(exe: PathBuf) -> io::Result<Self> {
        Self::at_key(&exe, RUN)
    }

    fn at_key(exe: &Path, key: &str) -> io::Result<Self> {
        let local_disk = matches!(exe.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)));
        if !exe.is_absolute() || !local_disk {
            return Err(invalid("startup executable must use a local drive path"));
        }
        let path = exe.as_os_str();
        if path
            .to_str()
            .is_none_or(|text| text.contains(['"', '%']) || text.chars().any(char::is_control))
        {
            return Err(invalid(
                "startup executable cannot be represented in a Run command",
            ));
        }
        let mut command = vec![u16::from(b'"')];
        command.extend(path.encode_wide());
        command.extend("\" --autostart".encode_utf16());
        command.push(0);
        if command.len() > MAX_COMMAND {
            return Err(invalid("startup command is too long"));
        }
        Ok(Self {
            key: wide(key),
            value: wide(VALUE),
            command,
        })
    }

    pub(super) fn status(&self) -> io::Result<Status> {
        let mut key = null_mut();
        // SAFETY: the key name is NUL-terminated and output is writable.
        let result = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                self.key.as_ptr(),
                0,
                KEY_QUERY_VALUE | KEY_WOW64_64KEY,
                &mut key,
            )
        };
        match result {
            ERROR_FILE_NOT_FOUND => Ok(Status::Disabled),
            ERROR_SUCCESS => self.read(&Key(key)),
            error => Err(win_error(error)),
        }
    }

    fn read(&self, key: &Key) -> io::Result<Status> {
        let mut kind = 0;
        let mut data = [0_u16; MAX_COMMAND];
        let mut bytes = std::mem::size_of_val(&data) as u32;
        // SAFETY: key is owned; aligned data buffer, length and type are
        // writable, and the value name is NUL-terminated.
        let result = unsafe {
            RegQueryValueExW(
                key.0,
                self.value.as_ptr(),
                null(),
                &mut kind,
                data.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        match result {
            ERROR_FILE_NOT_FOUND => Ok(Status::Disabled),
            ERROR_MORE_DATA => Ok(Status::External),
            ERROR_SUCCESS
                if kind == REG_SZ
                    && bytes as usize == self.command.len() * 2
                    && data[..self.command.len()] == self.command =>
            {
                Ok(Status::Enabled)
            }
            ERROR_SUCCESS => Ok(Status::External),
            error => Err(win_error(error)),
        }
    }

    pub(super) fn change(&self, expected: Status, desired: Status) -> io::Result<()> {
        let tx = Transaction::create()?;
        let flags = KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_64KEY;
        let mut key = null_mut();
        // SAFETY: the transaction is live and key is a writable output slot.
        // Only an explicit enable may create the exact per-user key, and this
        // key creation is rolled back if any later check fails.
        let result = unsafe {
            if desired == Status::Enabled {
                RegCreateKeyTransactedW(
                    HKEY_CURRENT_USER,
                    self.key.as_ptr(),
                    0,
                    null(),
                    REG_OPTION_NON_VOLATILE,
                    flags,
                    null(),
                    &mut key,
                    null_mut(),
                    tx.0,
                    null(),
                )
            } else {
                RegOpenKeyTransactedW(
                    HKEY_CURRENT_USER,
                    self.key.as_ptr(),
                    0,
                    flags,
                    &mut key,
                    tx.0,
                    null(),
                )
            }
        };
        check(result)?;
        let key = Key(key);
        if self.read(&key)? != expected {
            return Err(invalid("autostart entry changed"));
        }
        // SAFETY: the value belongs to the live transacted key; the string
        // buffer length is bounded and includes its terminal NUL.
        let result = unsafe {
            if desired == Status::Enabled {
                RegSetValueExW(
                    key.0,
                    self.value.as_ptr(),
                    0,
                    REG_SZ,
                    self.command.as_ptr().cast(),
                    (self.command.len() * 2) as u32,
                )
            } else {
                RegDeleteValueW(key.0, self.value.as_ptr())
            }
        };
        check(result)?;
        tx.commit()
    }
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain([0]).collect()
}
fn win_error(status: u32) -> io::Error {
    io::Error::from_raw_os_error(status as i32)
}
fn check(status: u32) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(win_error(status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Registry::{REG_BINARY, RegDeleteTreeW};

    #[test]
    fn scratch_hkcu_transaction_round_trips_and_refuses_foreign_type() {
        let scratch = tempfile::tempdir().unwrap();
        let key = format!(
            "Software\\Cumulus Autostart Test-{}-{}",
            std::process::id(),
            scratch.path().file_name().unwrap().to_str().unwrap()
        );
        let exe = Path::new(r"C:\Program Files\Cumulus 云\cumulus.exe");
        let entry = Registration::at_key(exe, &key).unwrap();
        let exact = String::from_utf16(&entry.command[..entry.command.len() - 1]).unwrap();
        assert_eq!(
            exact,
            r#""C:\Program Files\Cumulus 云\cumulus.exe" --autostart"#
        );
        assert_eq!(entry.status().unwrap(), Status::Disabled);
        struct Cleanup(Vec<u16>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                // SAFETY: only the unique test-only scratch key is removed.
                unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, self.0.as_ptr()) };
            }
        }
        let _cleanup = Cleanup(entry.key.clone());
        entry.change(Status::Disabled, Status::Enabled).unwrap();
        assert_eq!(entry.status().unwrap(), Status::Enabled);
        entry.change(Status::Enabled, Status::Disabled).unwrap();
        assert_eq!(entry.status().unwrap(), Status::Disabled);

        let mut raw = null_mut();
        // SAFETY: the generated scratch key (never Run) was created above.
        check(unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                entry.key.as_ptr(),
                0,
                KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_64KEY,
                &mut raw,
            )
        })
        .unwrap();
        let owned = Key(raw);
        let foreign = [1_u8, 2, 3];
        // SAFETY: this test exclusively owns the scratch key and seed buffer.
        check(unsafe {
            RegSetValueExW(
                owned.0,
                entry.value.as_ptr(),
                0,
                REG_BINARY,
                foreign.as_ptr(),
                foreign.len() as u32,
            )
        })
        .unwrap();
        assert_eq!(entry.status().unwrap(), Status::External);
        assert!(entry.change(Status::Disabled, Status::Enabled).is_err());
        assert!(entry.change(Status::Enabled, Status::Disabled).is_err());
        assert_eq!(entry.status().unwrap(), Status::External);
        let mut actual = [0_u8; 3];
        let mut length = actual.len() as u32;
        let mut kind = 0;
        // SAFETY: all out-parameters and the buffer belong to this scratch
        // query and the key and name remain valid.
        check(unsafe {
            RegQueryValueExW(
                owned.0,
                entry.value.as_ptr(),
                null(),
                &mut kind,
                actual.as_mut_ptr(),
                &mut length,
            )
        })
        .unwrap();
        assert_eq!((kind, length, actual), (REG_BINARY, 3, foreign));
    }
}
