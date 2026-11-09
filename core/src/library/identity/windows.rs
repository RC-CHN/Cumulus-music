//! A separate Windows handle identity. It does not change library fingerprints.

mod drive_path;
mod drive_root;

use std::{fs::File, io, os::windows::io::AsRawHandle};
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED},
    Storage::FileSystem::{
        FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_ATTRIBUTE_TAG_INFO, FILE_ID_INFO, FILE_TYPE_DISK, FileAttributeTagInfo, FileIdInfo,
        GetFileInformationByHandleEx, GetFileType,
    },
};

/// Volume serial and full 128-bit ID supplied by the same live disk handle.
/// This is an opaque comparison value, not an account grant, a pathname
/// identity, or a serialized replacement for `FileFingerprint`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsFileIdentity {
    volume: u64,
    id: [u8; 16],
}

impl WindowsFileIdentity {
    /// Query an already opened file without reopening its path. The native
    /// caller must have opened the leaf with FILE_FLAG_OPEN_REPARSE_POINT
    /// and separately checked its parent path. This cannot tell whether a
    /// caller already followed a link before supplying the handle.
    ///
    /// Returns `None` for an ineligible handle or a file system without
    /// this identity class; never falls back to a low-resolution file
    /// index or timestamps. Keep the same handle after comparing IDs.
    pub fn from_nofollow_file(file: &File) -> io::Result<Option<Self>> {
        let handle = file.as_raw_handle();
        // SAFETY: File keeps the borrowed handle live throughout both queries.
        if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
            return Ok(None);
        }
        let mut attributes = FILE_ATTRIBUTE_TAG_INFO::default();
        // SAFETY: This exact file information class writes the supplied
        // initialized FILE_ATTRIBUTE_TAG_INFO, with its actual buffer size.
        let status = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileAttributeTagInfo,
                (&raw mut attributes).cast(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        };
        if status == 0 {
            return unsupported_or_error(io::Error::last_os_error());
        }
        if !ordinary(attributes) {
            return Ok(None);
        }
        let mut identity = FILE_ID_INFO::default();
        // SAFETY: Same borrowed file handle and the exact initialized
        // FILE_ID_INFO layout required for this file information class.
        let status = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&raw mut identity).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if status == 0 {
            return unsupported_or_error(io::Error::last_os_error());
        }
        let id = identity.FileId.Identifier;
        Ok(
            (identity.VolumeSerialNumber != 0 && id != [0; 16]).then_some(Self {
                volume: identity.VolumeSerialNumber,
                id,
            }),
        )
    }
}

fn ordinary(attributes: FILE_ATTRIBUTE_TAG_INFO) -> bool {
    attributes.ReparseTag == 0
        && attributes.FileAttributes
            & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_DEVICE)
            == 0
}

fn unsupported_or_error(error: io::Error) -> io::Result<Option<WindowsFileIdentity>> {
    match error.raw_os_error().map(|code| code as u32) {
        Some(ERROR_INVALID_FUNCTION | ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED) => Ok(None),
        _ => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, FileTimes},
        os::windows::fs::{FileTimesExt, OpenOptionsExt},
        path::Path,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    fn open_leaf(path: &Path, flags: u32) -> File {
        fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | flags)
            .open(path)
            .unwrap()
    }

    #[test]
    fn same_handle_survives_rename_but_same_name_replacement_has_a_distinct_id() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("song.flac");
        let moved = root.path().join("held.flac");
        fs::write(&original, b"synthetic").unwrap();
        let held = open_leaf(&original, 0);
        let indexed = WindowsFileIdentity::from_nofollow_file(&held)
            .unwrap()
            .expect("test volume supports full file IDs");
        let legacy = held.metadata().unwrap();
        fs::rename(&original, &moved).unwrap();
        fs::write(&original, b"synthetic").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&original)
            .unwrap()
            .set_times(
                FileTimes::new()
                    .set_created(legacy.created().unwrap())
                    .set_modified(legacy.modified().unwrap()),
            )
            .unwrap();
        let replacement = open_leaf(&original, 0);
        let replacement_metadata = replacement.metadata().unwrap();
        assert_eq!(
            replacement_metadata.created().unwrap(),
            legacy.created().unwrap()
        );
        assert_eq!(
            replacement_metadata.modified().unwrap(),
            legacy.modified().unwrap()
        );
        assert_eq!(replacement_metadata.len(), legacy.len());
        assert_eq!(
            WindowsFileIdentity::from_nofollow_file(&held).unwrap(),
            Some(indexed)
        );
        assert_eq!(
            WindowsFileIdentity::from_nofollow_file(&open_leaf(&moved, 0)).unwrap(),
            Some(indexed)
        );
        let changed = WindowsFileIdentity::from_nofollow_file(&replacement)
            .unwrap()
            .expect("replacement also has a full ID");
        assert_ne!(changed, indexed);
    }

    #[test]
    fn directory_handle_is_ineligible_and_reparse_attributes_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let dir = open_leaf(root.path(), FILE_FLAG_BACKUP_SEMANTICS);
        assert_eq!(WindowsFileIdentity::from_nofollow_file(&dir).unwrap(), None);
        for flags in [
            FILE_ATTRIBUTE_DEVICE,
            FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_REPARSE_POINT,
        ] {
            assert!(!ordinary(FILE_ATTRIBUTE_TAG_INFO {
                FileAttributes: flags,
                ReparseTag: 0,
            }));
        }
        assert!(!ordinary(FILE_ATTRIBUTE_TAG_INFO {
            FileAttributes: FILE_ATTRIBUTE_NORMAL,
            ReparseTag: 0xa000_000c, // IO_REPARSE_TAG_SYMLINK
        }));
        assert!(ordinary(FILE_ATTRIBUTE_TAG_INFO {
            FileAttributes: FILE_ATTRIBUTE_NORMAL,
            ReparseTag: 0,
        }));
    }

    #[test]
    #[ignore = "actual Windows symlink creation requires Developer Mode or privilege"]
    fn actual_reparse_handle_is_rejected_without_following_its_file_target() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("target.flac");
        let link = root.path().join("symlink.flac");
        fs::write(&path, b"synthetic").unwrap();
        std::os::windows::fs::symlink_file(&path, &link).unwrap();
        let target = open_leaf(&path, 0);
        let reparse = open_leaf(&link, 0);
        assert!(
            WindowsFileIdentity::from_nofollow_file(&target)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            WindowsFileIdentity::from_nofollow_file(&reparse).unwrap(),
            None
        );
    }
}
