use crate::{
    Error, Result,
    download::file_identity::{Identity, identity},
};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub(super) fn root(path: &Path, state: &Path, create: bool) -> Result<PathBuf> {
    if create {
        fs::create_dir_all(path)?;
    }
    let path = path.canonicalize()?;
    if path.parent().is_none() || path.to_str().is_none() || path.starts_with(state.canonicalize()?)
    {
        return Err(Error::InvalidInput(
            "choose a UTF-8 media directory outside private state and filesystem root",
        ));
    }
    if !fs::symlink_metadata(&path)?.is_dir() {
        return Err(Error::InvalidInput("download root must be a directory"));
    }
    Ok(path)
}
pub(super) fn writable(path: &Path) -> Result<()> {
    let probe = tempfile::Builder::new()
        .prefix(".cumulus-write-check-")
        .tempfile_in(path)?;
    probe.as_file().sync_all()?;
    probe.close()?;
    Ok(())
}
pub(super) fn check(path: &Path, expected: &Identity) -> Result<()> {
    if path.canonicalize()? != path
        || !fs::symlink_metadata(path)?.is_dir()
        || identity(path)? != *expected
    {
        return Err(Error::State("migration root was replaced"));
    }
    Ok(())
}
pub(in crate::download) fn mapped(path: &Path, source: &Path, target: &Path) -> Result<PathBuf> {
    let relative = path
        .strip_prefix(source)
        .map_err(|_| Error::State("download path escapes migration source"))?;
    if relative
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::State("invalid relative migration path"));
    }
    Ok(target.join(relative))
}
/// Check each existing component before creating descendants. create_dir_all
/// followed by canonicalize could create directories through an existing link.
pub(in crate::download) fn ensure_directory(path: &Path, root: &Path) -> Result<()> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::State("directory escapes migration root"))?;
    let mut current = root.to_path_buf();
    if current.canonicalize()? != current || !fs::symlink_metadata(&current)?.is_dir() {
        return Err(Error::State("migration root contains a link"));
    }
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(Error::State("invalid migration directory component"));
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(e) => return Err(e.into()),
            Ok(_) => (),
        }
        if !fs::symlink_metadata(&current)?.is_dir() || current.canonicalize()? != current {
            return Err(Error::State("migration destination contains a link"));
        }
    }
    Ok(())
}
pub(in crate::download) fn prepare_target(path: &Path, root: &Path) -> Result<()> {
    ensure_directory(
        path.parent()
            .ok_or(Error::State("missing migration target parent"))?,
        root,
    )
}
pub(super) fn available(path: &Path) -> Result<u64> {
    #[cfg(unix)]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| Error::InvalidInput("NUL in disk path"))?;
        let mut result = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: path is NUL-terminated; result points to a correctly aligned
        // writable statvfs. The value is read only after the syscall succeeds.
        if unsafe { libc::statvfs(path.as_ptr(), result.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let result = unsafe { result.assume_init() };
        let bytes = u128::from(result.f_bavail) * u128::from(result.f_frsize);
        Ok(bytes.min(u128::from(u64::MAX)) as u64)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(
                path: *const u16,
                available: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(Error::InvalidInput("NUL in disk path"));
        }
        wide.push(0);
        let mut free = 0;
        // SAFETY: live NUL-terminated UTF-16 buffer and writable output; optional
        // total/free pointers are null as permitted by the Windows API.
        if unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(free)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(Error::State(
            "disk-space probe unsupported on this platform",
        ))
    }
}
