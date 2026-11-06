//! Reveal one native-validated folder as an item in its containing directory.
use cumulus_core::{Error, Result};
#[cfg(any(target_os = "linux", windows))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(any(target_os = "linux", windows))]
fn checked_parent(path: &Path) -> Result<&Path> {
    if !path.is_absolute() || !path.is_dir() || path.canonicalize()? != path {
        return Err(Error::QueueChanged);
    }
    path.parent()
        .ok_or(Error::State("folder has no containing directory"))
}

#[cfg(target_os = "linux")]
pub(crate) async fn reveal_directory(path: PathBuf) -> Result<()> {
    use gio::prelude::CancellableExt;
    use std::time::Duration;

    let cancel = gio::Cancellable::new();
    let guard = cancel.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || -> Result<Option<PathBuf>> {
        checked_parent(&path)?;
        if linux_show_item(&path, &guard)? {
            Ok(None)
        } else {
            // The session bus or method was definitively unavailable. Keep
            // the meaning of “所在目录” even without item selection support.
            Ok(Some(checked_parent(&path)?.to_path_buf()))
        }
    });
    let parent = match tokio::time::timeout(Duration::from_secs(4), worker).await {
        Ok(reply) => reply.map_err(|_| Error::State("folder reveal worker stopped"))??,
        Err(_) => {
            cancel.cancel();
            return Err(Error::State(
                "file manager did not acknowledge folder reveal",
            ));
        }
    };
    if let Some(parent) = parent {
        super::open_directory(parent).await?;
    }
    Ok(())
}

/// False means the requested method never ran (no bus or explicit absence).
/// Timeouts and other ambiguous failures cannot trigger another shell request.
#[cfg(target_os = "linux")]
fn linux_show_item(path: &Path, cancel: &gio::Cancellable) -> Result<bool> {
    use gio::{
        glib::{VariantTy, variant::ToVariant},
        prelude::{CancellableExt, FileExt},
    };
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let connection = match gio::bus_get_sync(gio::BusType::Session, Some(cancel)) {
        Ok(connection) => connection,
        Err(_) if cancel.is_cancelled() => return Err(Error::Cancelled),
        Err(_) => return Ok(false),
    };
    connection.set_exit_on_close(false);
    let uri = gio::File::for_path(path).uri().to_string();
    let params = (vec![uri], "").to_variant();
    const SERVICE: &str = "org.freedesktop.FileManager1";
    // Canonicalization or opening the session bus can finish after the caller's
    // outer deadline. In that case do not start a late shell request.
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    match connection.call_sync(
        Some(SERVICE),
        "/org/freedesktop/FileManager1",
        SERVICE,
        "ShowItems",
        Some(&params),
        Some(VariantTy::UNIT),
        gio::DBusCallFlags::NONE,
        2_000,
        Some(cancel),
    ) {
        Ok(_) => Ok(true),
        Err(error) if dbus_reveal_unavailable(&error) => Ok(false),
        Err(_) => Err(Error::State("file manager could not reveal folder")),
    }
}

#[cfg(target_os = "linux")]
fn dbus_reveal_unavailable(error: &gio::glib::Error) -> bool {
    use gio::glib::{
        GString,
        translate::{ToGlibPtr, from_glib_full},
    };
    // SAFETY: error stays live for this call. GIO returns an allocated,
    // NUL-terminated UTF-8 name on a remote error, or null otherwise.
    let name = unsafe { gio::ffi::g_dbus_error_get_remote_error(error.to_glib_none().0) };
    if name.is_null() {
        return false;
    }
    // SAFETY: take ownership of the single returned gchar allocation.
    let name: GString = unsafe { from_glib_full(name) };
    matches!(
        name.as_str(),
        "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.DBus.Error.UnknownInterface"
            | "org.freedesktop.DBus.Error.UnknownMethod"
    )
}

#[cfg(windows)]
pub(crate) async fn reveal_directory(path: PathBuf) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        // The shell uses an owned STA; leave Tauri's/runtime's COM mode alone.
        std::thread::Builder::new()
            .name("file-manager-reveal".into())
            .spawn(move || {
                let parent = checked_parent(&path)?.to_path_buf();
                windows_show_item(path, parent)
            })?
            .join()
            .map_err(|_| Error::State("file manager shell stopped"))?
    })
    .await
    .map_err(|_| Error::State("folder reveal worker stopped"))?
}

#[cfg(windows)]
fn windows_show_item(path: PathBuf, parent: PathBuf) -> Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoTaskMemFree,
            CoUninitialize,
        },
        UI::Shell::{
            Common::ITEMIDLIST, ILFindLastID, SHOpenFolderAndSelectItems, SHParseDisplayName,
        },
    };

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: constructed only after successful COM initialization on
            // this owned STA, and not shared across threads.
            unsafe { CoUninitialize() };
        }
    }
    struct Pidl(*mut ITEMIDLIST);
    impl Drop for Pidl {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: this non-null pointer is owned CoTaskMem from the parser.
                unsafe { CoTaskMemFree(self.0.cast()) };
            }
        }
    }
    fn parse(path: PathBuf) -> Result<Pidl> {
        let path = super::windows_shell_path(path);
        let mut wide: Vec<_> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(Error::State("folder path is invalid"));
        }
        wide.push(0);
        let mut pidl = Pidl(ptr::null_mut());
        // SAFETY: the input is NUL-terminated, pidl is an initialized output
        // slot, and null context/attributes are accepted by the shell.
        let hr = unsafe {
            SHParseDisplayName(
                wide.as_ptr(),
                ptr::null_mut(),
                &mut pidl.0,
                0,
                ptr::null_mut(),
            )
        };
        if hr < 0 || pidl.0.is_null() {
            Err(Error::State("file manager could not parse folder"))
        } else {
            Ok(pidl)
        }
    }
    // SAFETY: null reserved parameter and fixed COM flags; success is balanced
    // by Apartment on this same thread after both owned PIDLs are released.
    let hr = unsafe {
        CoInitializeEx(
            ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };
    if hr < 0 {
        return Err(Error::State("file manager COM initialization failed"));
    }
    let _apartment = Apartment;
    let parent = parse(parent)?;
    let item = parse(path)?;
    // SAFETY: the child aliases the tail of the still-owned absolute PIDL.
    let child = unsafe { ILFindLastID(item.0) };
    if child.is_null() {
        return Err(Error::State("file manager could not locate folder"));
    }
    let children = [child.cast_const()];
    // SAFETY: one relative child PIDL and its containing absolute PIDL remain
    // valid for this synchronous Shell call; no command line is built.
    let hr = unsafe { SHOpenFolderAndSelectItems(parent.0, 1, children.as_ptr(), 0) };
    if hr < 0 {
        Err(Error::State("file manager could not reveal folder"))
    } else {
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) async fn reveal_directory(_: PathBuf) -> Result<()> {
    Err(Error::State("folder reveal not supported on this platform"))
}
