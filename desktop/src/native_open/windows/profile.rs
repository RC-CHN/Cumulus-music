//! The warm owner and sender derive identity without reopening a DOS path.
//! Filesystem opens start at an already validated native volume object.
use super::{
    identity::{DirectoryId, directory_id},
    profile_path::{drive_path, first_fixed_device, is_volume_guid_root},
};
use std::{
    ffi::OsString,
    fs::File,
    io,
    os::windows::{
        ffi::OsStringExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_IF,
            FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, FileFsDeviceInformation,
            NtCreateFile, NtQueryVolumeInformationFile,
        },
        System::SystemServices::{
            FILE_FS_DEVICE_INFORMATION, FILE_REMOTE_DEVICE, FILE_REMOVABLE_MEDIA,
        },
    },
    Win32::{
        Foundation::{
            HANDLE, INVALID_HANDLE_VALUE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE,
            RtlNtStatusToDosError, UNICODE_STRING,
        },
        Globalization::lstrlenW,
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_NORMAL,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_DEVICE_DISK, FILE_NAME_NORMALIZED,
            FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
            FILE_TYPE_DISK, FILE_WRITE_DATA, GetFileInformationByHandle, GetFileType,
            GetFinalPathNameByHandleW, GetVolumeInformationByHandleW, QueryDosDeviceW, SYNCHRONIZE,
            VOLUME_NAME_DOS, VOLUME_NAME_GUID,
        },
        System::{Com::CoTaskMemFree, IO::IO_STATUS_BLOCK, Ioctl::FILE_DEVICE_DISK_FILE_SYSTEM},
        UI::Shell::{FOLDERID_RoamingAppData, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath},
    },
};

const DIRECTORY_ACCESS: u32 = FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE;
const LOCK_ACCESS: u32 = FILE_READ_ATTRIBUTES | FILE_READ_DATA | FILE_WRITE_DATA | SYNCHRONIZE;
const SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;
const NOT_ORDINARY: u32 = FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DEVICE;
pub(crate) const LOCK_NAME: &str = ".native-open-process.lock";

pub(super) fn roaming_app_data() -> io::Result<PathBuf> {
    let mut pointer = null_mut();
    // SAFETY: folder ID and pointer output are live. DONT_VERIFY returns the
    // configured name without first probing a redirected folder's existence.
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_RoamingAppData,
            KF_FLAG_DONT_VERIFY as u32,
            null_mut(),
            &mut pointer,
        )
    };
    struct Allocation(*mut u16);
    impl Drop for Allocation {
        fn drop(&mut self) {
            // SAFETY: SHGetKnownFolderPath uses the COM task allocator, even
            // for a non-null partial result on failure; null is accepted.
            unsafe { CoTaskMemFree(self.0.cast()) };
        }
    }
    let path = Allocation(pointer);
    if result < 0 || path.0.is_null() {
        return Err(io::Error::other("application data directory unavailable"));
    }
    // SAFETY: a successful known-folder lookup returned a NUL-terminated
    // UTF-16 string owned by path.
    let length = unsafe { lstrlenW(path.0) } as usize;
    if !(1..=32_767).contains(&length) {
        return Err(io::Error::other("invalid application data directory"));
    }
    // SAFETY: lstrlenW determined the initialized length; no buffer is retained.
    let value = unsafe { std::slice::from_raw_parts(path.0, length) };
    Ok(PathBuf::from(OsString::from_wide(value)))
}

pub(crate) struct Profile {
    // Keep every validated directory open without FILE_SHARE_DELETE while the
    // owner holds its process lock. The only route to the lock is this chain.
    directories: Vec<File>,
    id: DirectoryId,
    volume: String,
    path: PathBuf,
}

impl Profile {
    /// Unsupported profile namespaces do not acquire warm authority.
    pub(super) fn open(path: &Path, create: bool) -> io::Result<Option<Self>> {
        let Some(input) = path.to_str().and_then(drive_path) else {
            return Ok(None);
        };
        let Some(native) = device_target(input.drive)? else {
            return Ok(None);
        };
        // Open the captured native device, never the mutable DOS letter.
        let root = open_nt(None, &native, OpenKind::Directory, false)?;
        if !ordinary_directory(&root)? || !fixed_ntfs(&root)? {
            return Ok(None);
        }
        let volume = name_by_handle(&root, VOLUME_NAME_GUID)?;
        if !is_volume_guid_root(&volume) {
            return Ok(None);
        }
        let mut directories = vec![root];
        for component in input.relative.split('\\') {
            let child = open_nt(directories.last(), component, OpenKind::Directory, create)?;
            if !ordinary_directory(&child)? {
                return Ok(None);
            }
            directories.push(child);
        }
        let directory = directories.last().expect("held profile directory");
        let id = directory_id(directory)?;
        // Retain the existing DOS-form state path for the rest of desktop.
        // This is a handle-derived name, not another filesystem path open.
        let path = name_by_handle(directory, VOLUME_NAME_DOS)?;
        let Some(dos) = drive_path(&path) else {
            return Ok(None);
        };
        if dos.drive != input.drive
            || device_target(input.drive)?.as_deref() != Some(native.as_str())
        {
            return Ok(None);
        }
        Ok(Some(Self {
            directories,
            id,
            volume,
            path: PathBuf::from(path),
        }))
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn identity(&self) -> (DirectoryId, &str) {
        (self.id, &self.volume)
    }

    pub(super) fn same_directory(&self, path: &Path) -> bool {
        Self::open(path, false).is_ok_and(|actual| {
            actual.is_some_and(|other| {
                self.id == other.id && self.volume.eq_ignore_ascii_case(&other.volume)
            })
        })
    }

    pub(super) fn lock_file(&self) -> io::Result<File> {
        let directory = self.directories.last().expect("held profile directory");
        let file = open_nt(Some(directory), LOCK_NAME, OpenKind::Lock, true)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.file_attributes() & NOT_ORDINARY != 0 {
            return Err(io::Error::other("invalid native profile lock file"));
        }
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file is alive, and info is writable and exactly the Win32 structure.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.nNumberOfLinks != 1 || !disk(&file) {
            return Err(io::Error::other(
                "invalid native profile lock file identity",
            ));
        }
        Ok(file)
    }
}

fn device_target(drive: u8) -> io::Result<Option<String>> {
    let dos = [u16::from(drive), u16::from(b':'), 0];
    let mut map = [0u16; 4096];
    // SAFETY: DOS name is NUL-terminated; output covers all declared units.
    let size = unsafe { QueryDosDeviceW(dos.as_ptr(), map.as_mut_ptr(), map.len() as u32) };
    if size == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(map.get(..size as usize).and_then(first_fixed_device))
}

fn disk(file: &File) -> bool {
    // SAFETY: the file handle remains valid for this query.
    unsafe { GetFileType(file.as_raw_handle()) == FILE_TYPE_DISK }
}

fn ordinary_directory(file: &File) -> io::Result<bool> {
    let metadata = file.metadata()?;
    Ok(metadata.is_dir() && metadata.file_attributes() & NOT_ORDINARY == 0 && disk(file))
}

fn fixed_ntfs(root: &File) -> io::Result<bool> {
    let mut device = FILE_FS_DEVICE_INFORMATION::default();
    let mut completion = IO_STATUS_BLOCK::default();
    // SAFETY: open_nt created a synchronous root handle. Both output buffers
    // have the precise WDK layout, remain live and are not read during I/O.
    let status = unsafe {
        NtQueryVolumeInformationFile(
            root.as_raw_handle(),
            &mut completion,
            (&raw mut device).cast(),
            size_of::<FILE_FS_DEVICE_INFORMATION>() as u32,
            FileFsDeviceInformation,
        )
    };
    if status < 0 {
        // SAFETY: the value is the scalar status returned by the query.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    if completion.Information != size_of::<FILE_FS_DEVICE_INFORMATION>()
        || !matches!(
            device.DeviceType,
            FILE_DEVICE_DISK | FILE_DEVICE_DISK_FILE_SYSTEM
        )
        || device.Characteristics & (FILE_REMOTE_DEVICE | FILE_REMOVABLE_MEDIA) != 0
    {
        return Ok(false);
    }
    let mut filesystem = [0u16; 32];
    // SAFETY: no optional outputs except the writable filesystem buffer.
    if unsafe {
        GetVolumeInformationByHandleW(
            root.as_raw_handle(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let Some(end) = filesystem.iter().position(|unit| *unit == 0) else {
        return Ok(false);
    };
    Ok(String::from_utf16(&filesystem[..end]).is_ok_and(|name| name.eq_ignore_ascii_case("NTFS")))
}

fn name_by_handle(file: &File, namespace: u32) -> io::Result<String> {
    let read = |name: &mut [u16]| -> io::Result<usize> {
        // SAFETY: initialized buffer has the declared capacity; file owns handle.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                name.as_mut_ptr(),
                name.len() as u32,
                FILE_NAME_NORMALIZED | namespace,
            )
        };
        if length == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(length as usize)
        }
    };
    let mut name = vec![0u16; 512];
    let mut written = read(&mut name)?;
    if written >= name.len() {
        if written > 32_768 {
            return Err(io::Error::other("native profile path is too long"));
        }
        name.resize(written, 0);
        written = read(&mut name)?;
    }
    if written >= name.len() || name[written] != 0 {
        return Err(io::Error::other("native profile path changed"));
    }
    String::from_utf16(&name[..written])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid native profile path"))
}

enum OpenKind {
    Directory,
    Lock,
}

fn open_nt(parent: Option<&File>, name: &str, kind: OpenKind, create: bool) -> io::Result<File> {
    let mut wide: Vec<_> = name.encode_utf16().collect();
    let length = u16::try_from(wide.len() * size_of::<u16>()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "native profile component too long",
        )
    })?;
    let counted = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: wide.as_mut_ptr(),
    };
    let attrs = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.map_or(null_mut(), AsRawHandle::as_raw_handle),
        ObjectName: &counted,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        SecurityDescriptor: null(),
        SecurityQualityOfService: null(),
    };
    let mut handle: HANDLE = null_mut();
    let mut status_block = IO_STATUS_BLOCK::default();
    let (access, kind) = match kind {
        OpenKind::Directory => (DIRECTORY_ACCESS, FILE_DIRECTORY_FILE),
        OpenKind::Lock => (LOCK_ACCESS, FILE_NON_DIRECTORY_FILE),
    };
    // SAFETY: the borrowed root, counted string and initialized out-parameters
    // outlive the synchronous call. Relative names are single checked leaves.
    // FILE_OPEN_IF preserves existing content; no handle is inheritable.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attrs,
            &mut status_block,
            null(),
            FILE_ATTRIBUTE_NORMAL,
            SHARE,
            if create { FILE_OPEN_IF } else { FILE_OPEN },
            kind | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            null(),
            0,
        )
    };
    if status < 0 {
        // SAFETY: conversion accepts an NTSTATUS value from the function above.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::other("native profile returned no file handle"));
    }
    // SAFETY: the successful open transferred a unique live File handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write};

    fn profile(path: &Path) -> Profile {
        Profile::open(path, true)
            .unwrap()
            .expect("test requires a fixed local NTFS temporary directory")
    }

    #[test]
    fn same_held_profile_can_create_its_relative_lock_and_prevents_a_swap() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("合成").join("Cumulus");
        assert!(!path.exists());
        let held = profile(&path);
        assert!(held.same_directory(&path));
        assert!(!held.same_directory(&root.path().join("not-created")));
        let other = profile(&root.path().join("other"));
        assert!(!held.same_directory(other.path()));

        let mut lock = held.lock_file().unwrap();
        lock.write_all(b"synthetic").unwrap();
        let competing = held.lock_file().unwrap();
        assert_eq!(competing.metadata().unwrap().len(), 9);
        lock.try_lock().unwrap();
        assert!(matches!(
            competing.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        lock.unlock().unwrap();

        let moved = root.path().join("moved");
        assert!(fs::rename(&path, &moved).is_err());
        drop(competing);
        drop(lock);
        drop(held);
        fs::rename(&path, &moved).unwrap();
        let replacement = profile(&path);
        assert!(!replacement.same_directory(&moved));
    }

    #[test]
    fn a_linked_lock_cannot_claim_an_unrelated_file() {
        let root = tempfile::tempdir().unwrap();
        let held = profile(&root.path().join("Cumulus"));
        let outside = root.path().join("unrelated");
        fs::write(&outside, b"unrelated").unwrap();
        fs::hard_link(&outside, held.path().join(LOCK_NAME)).unwrap();
        assert!(held.lock_file().is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"unrelated");
    }

    #[test]
    #[ignore = "requires Windows permission to create symlinks"]
    fn a_reparse_ancestor_or_lock_cannot_open_its_target() {
        use std::os::windows::fs::{symlink_dir, symlink_file};

        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("other");
        fs::create_dir(&outside).unwrap();
        let link = root.path().join("redirect");
        symlink_dir(&outside, &link).unwrap();
        let candidate = link.join("Cumulus");
        assert!(!matches!(Profile::open(&candidate, true), Ok(Some(_))));
        assert!(!outside.join("Cumulus").exists());

        let held = profile(&root.path().join("Cumulus"));
        let target = outside.join("foreign");
        fs::write(&target, b"unchanged").unwrap();
        symlink_file(&target, held.path().join(LOCK_NAME)).unwrap();
        assert!(held.lock_file().is_err());
        assert_eq!(fs::read(&target).unwrap(), b"unchanged");
    }
}
