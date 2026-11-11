use super::{
    WindowsFileIdentity,
    drive_path::{is_local_disk_device, is_volume_guid_root, relative_drive_path},
};
use std::{
    fs::File,
    io,
    os::windows::{
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_NO_RECALL,
            FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, FileFsDeviceInformation,
            NtCreateFile, NtQueryVolumeInformationFile,
        },
    },
    Win32::{
        Foundation::{
            HANDLE, INVALID_HANDLE_VALUE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE,
            RtlNtStatusToDosError, STATUS_FILE_IS_A_DIRECTORY, STATUS_REPARSE_POINT_ENCOUNTERED,
            STATUS_STOPPED_ON_SYMLINK, STATUS_SUCCESS, UNICODE_STRING,
        },
        Storage::FileSystem::{
            FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE,
            FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_DEVICE_DISK, FILE_NAME_NORMALIZED,
            FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, FILE_STANDARD_INFO, FILE_TRAVERSE, FILE_TYPE_DISK, FileStandardInfo,
            GetFileInformationByHandleEx, GetFileType, GetFinalPathNameByHandleW,
            GetVolumeInformationByHandleW, QueryDosDeviceW, SYNCHRONIZE, VOLUME_NAME_GUID,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

impl WindowsFileIdentity {
    /// Opt-in checked file open for an ordinary path on a fixed local NTFS
    /// drive. The returned identity belongs to the returned read-only handle.
    /// Neither a reparse point nor a substitute drive rooted at a subdirectory
    /// supplies a file. There is no alternate open or weaker identity path.
    ///
    /// The link count and file attributes are point-in-time checks; this is
    /// neither a content hash nor a replacement for caller-owned index/queue
    /// generation checks. This does not enable a new scanner or playback route.
    pub fn open_local_fixed_ntfs_nofollow(path: &Path) -> io::Result<Option<(File, Self)>> {
        let Some((drive, relative)) = path.to_str().and_then(relative_drive_path) else {
            return Ok(None);
        };
        let Some(root) = fixed_ntfs_root(drive)? else {
            return Ok(None);
        };
        let Some(file) = open_relative(&root, relative)? else {
            return Ok(None);
        };
        let Some(identity) = Self::from_nofollow_file(&file)? else {
            return Ok(None);
        };
        let offline = file.metadata()?.file_attributes()
            & (FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0;
        Ok((!offline && is_single_live_link(&file)?).then_some((file, identity)))
    }
}

fn fixed_ntfs_root(drive: u8) -> io::Result<Option<File>> {
    let drive_name = [u16::from(drive), b':' as u16, 0];
    let mut mappings = [0u16; 512];
    // SAFETY: the drive name is NUL-terminated and the output capacity is
    // in UTF-16 units. QueryDosDeviceW only queries the namespace mapping.
    let length = unsafe {
        QueryDosDeviceW(
            drive_name.as_ptr(),
            mappings.as_mut_ptr(),
            mappings.len() as u32,
        )
    } as usize;
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    let Some(end) = mappings
        .get(..length)
        .and_then(|names| names.iter().position(|&unit| unit == 0))
    else {
        return Ok(None);
    };
    let Ok(device) = String::from_utf16(&mappings[..end]) else {
        return Ok(None);
    };
    if !is_local_disk_device(&device) {
        return Ok(None);
    }
    // Use the accepted native device name, never the mutable DOS mapping
    // again. The trailing separator opens its filesystem root directory.
    let Some(root) = open_nt(null_mut(), &format!("{device}\\"), true)? else {
        return Ok(None);
    };
    let handle = root.as_raw_handle();
    // SAFETY: root owns handle for the remainder of these queries.
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
        return Ok(None);
    }
    let metadata = root.metadata()?;
    if !metadata.is_dir()
        || metadata.file_attributes() & (FILE_ATTRIBUTE_DEVICE | FILE_ATTRIBUTE_REPARSE_POINT) != 0
    {
        return Ok(None);
    }

    if !is_fixed_disk(&root)? {
        return Ok(None);
    }

    // A true GUID root fits in this buffer. A longer name is a mounted
    // subdirectory or otherwise outside scope; do not reopen it.
    let mut guid = [0u16; 64];
    // SAFETY: handle is live, and the output buffer covers the declared units.
    let len = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            guid.as_mut_ptr(),
            guid.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_GUID,
        )
    } as usize;
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    if len >= guid.len()
        || guid[len] != 0
        || !String::from_utf16(&guid[..len]).is_ok_and(|name| is_volume_guid_root(&name))
    {
        return Ok(None);
    }
    let mut fs_name = [0u16; 16];
    // SAFETY: root still owns handle; optional unused outputs are null and
    // fs_name has the declared capacity for the filesystem name.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            handle,
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            fs_name.as_mut_ptr(),
            fs_name.len() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let is_ntfs = fs_name[..5].iter().copied().eq("NTFS\0".encode_utf16());
    Ok(is_ntfs.then_some(root))
}

// FILE_FS_DEVICE_INFORMATION (MS-FSCC 2.5.10). The output is a pair of
// ULONGs, with no Rust bool, pointer, or implicit enum discriminant.
#[repr(C)]
#[derive(Default)]
struct VolumeDeviceInfo {
    device_type: u32,
    characteristics: u32,
}

fn is_fixed_disk(root: &File) -> io::Result<bool> {
    const FILE_REMOVABLE_MEDIA: u32 = 0x0000_0001;
    const FILE_REMOTE_DEVICE: u32 = 0x0000_0010;
    const FILE_REMOTE_DEVICE_VSMB: u32 = 0x0008_0000;
    let mut info = VolumeDeviceInfo::default();
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: a live synchronous filesystem root handle and initialized
    // storage of the two documented ULONGs and IO_STATUS_BLOCK.
    let status = unsafe {
        NtQueryVolumeInformationFile(
            root.as_raw_handle(),
            &mut status_block,
            (&raw mut info).cast(),
            size_of::<VolumeDeviceInfo>() as u32,
            FileFsDeviceInformation,
        )
    };
    if status != STATUS_SUCCESS {
        return Err(nt_error(status));
    }
    Ok(status_block.Information == size_of::<VolumeDeviceInfo>()
        && info.device_type == FILE_DEVICE_DISK
        && info.characteristics
            & (FILE_REMOVABLE_MEDIA | FILE_REMOTE_DEVICE | FILE_REMOTE_DEVICE_VSMB)
            == 0)
}

fn open_relative(root: &File, relative: &str) -> io::Result<Option<File>> {
    open_nt(root.as_raw_handle(), relative, false)
}

fn open_nt(root: HANDLE, path: &str, directory: bool) -> io::Result<Option<File>> {
    let mut wide: Vec<_> = path.encode_utf16().collect();
    let length = u16::try_from(wide.len() * size_of::<u16>())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NT filename is too long"))?;
    let name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: wide.as_mut_ptr(),
    };
    let attrs = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: root,
        ObjectName: &name,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        SecurityDescriptor: null(),
        SecurityQualityOfService: null(),
    };
    let mut result = null_mut();
    let mut io_status = IO_STATUS_BLOCK::default();
    let (access, share, options) = if directory {
        (
            FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            // The directory option is documented with synchronous I/O,
            // not the leaf-only reparse/no-recall flags. This name is the
            // accepted direct NT root and still uses OBJ_DONT_REPARSE.
            FILE_DIRECTORY_FILE,
        )
    } else {
        (
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_NO_RECALL | FILE_OPEN_REPARSE_POINT,
        )
    };
    // SAFETY: the optional borrowed root and the counted UTF-16 buffer outlive this
    // synchronous call. All written outputs are initialized storage of the
    // exact WDK types; optional allocation/EA inputs are absent. FILE_OPEN
    // cannot create or overwrite. OBJ_DONT_REPARSE covers the relative
    // parse; the nofollow leaf flag and later file attributes reject a leaf
    // reparse object if NT opens that object itself.
    let status = unsafe {
        NtCreateFile(
            &mut result,
            access,
            &attrs,
            &mut io_status,
            null(),
            FILE_ATTRIBUTE_NORMAL,
            share,
            FILE_OPEN,
            options | FILE_SYNCHRONOUS_IO_NONALERT,
            null(),
            0,
        )
    };
    match status {
        STATUS_REPARSE_POINT_ENCOUNTERED
        | STATUS_STOPPED_ON_SYMLINK
        | STATUS_FILE_IS_A_DIRECTORY => Ok(None),
        STATUS_SUCCESS if result.is_null() || result == INVALID_HANDLE_VALUE => {
            Err(io::Error::other("NT file open returned no handle"))
        }
        // SAFETY: the successful call transferred one valid, owned file
        // handle; nothing else wraps or closes it.
        STATUS_SUCCESS => Ok(Some(unsafe { File::from_raw_handle(result) })),
        status => Err(nt_error(status)),
    }
}

fn nt_error(status: i32) -> io::Error {
    // SAFETY: this routine accepts the returned scalar NTSTATUS.
    io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32)
}

// Win32 writes two BOOLEAN bytes. Decode as integers to avoid interpreting
// bytes written by FFI as Rust bool values before validation.
#[repr(C)]
#[derive(Default)]
struct FileStandardInfoBytes {
    allocation_size: i64,
    end_of_file: i64,
    number_of_links: u32,
    delete_pending: u8,
    directory: u8,
}

const _: () = {
    assert!(size_of::<FileStandardInfoBytes>() == size_of::<FILE_STANDARD_INFO>());
    assert!(
        std::mem::offset_of!(FileStandardInfoBytes, number_of_links)
            == std::mem::offset_of!(FILE_STANDARD_INFO, NumberOfLinks)
    );
    assert!(
        std::mem::offset_of!(FileStandardInfoBytes, delete_pending)
            == std::mem::offset_of!(FILE_STANDARD_INFO, DeletePending)
    );
    assert!(
        std::mem::offset_of!(FileStandardInfoBytes, directory)
            == std::mem::offset_of!(FILE_STANDARD_INFO, Directory)
    );
};

pub(super) fn is_single_live_link(file: &File) -> io::Result<bool> {
    let mut info = FileStandardInfoBytes::default();
    // SAFETY: File owns the borrowed handle; the raw C structure has the
    // exact FileStandardInfo size/layout and permits any BOOLEAN bytes.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStandardInfo,
            (&raw mut info).cast(),
            size_of::<FileStandardInfoBytes>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info.number_of_links == 1 && info.delete_pending == 0 && info.directory == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, FileTimes, OpenOptions},
        io::Read,
        os::windows::fs::FileTimesExt,
    };
    use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;

    fn open(path: &Path) -> Option<(File, WindowsFileIdentity)> {
        WindowsFileIdentity::open_local_fixed_ntfs_nofollow(path).unwrap()
    }

    #[test]
    #[ignore = "requires a real Windows runner with a fixed local NTFS temp directory"]
    fn fixed_drive_handle_rejects_writes_and_keeps_old_bytes_after_equal_metadata_replacement() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let path = root.join("合成 music.flac");
        let moved = root.join("held.flac");
        fs::write(&path, b"fLaC original").unwrap();
        let (mut file, original) = open(&path).expect("test temp root is fixed local NTFS");
        assert_eq!(
            OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap_err()
                .raw_os_error(),
            Some(ERROR_SHARING_VIOLATION as i32)
        );
        let metadata = file.metadata().unwrap();
        fs::rename(&path, &moved).unwrap();
        fs::write(&path, b"fLaC replaced").unwrap();
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(
                FileTimes::new()
                    .set_created(metadata.created().unwrap())
                    .set_modified(metadata.modified().unwrap()),
            )
            .unwrap();
        let (mut replacement_file, replacement) = open(&path).unwrap();
        let replaced = replacement_file.metadata().unwrap();
        assert_eq!(replaced.len(), metadata.len());
        assert_eq!(replaced.created().unwrap(), metadata.created().unwrap());
        assert_eq!(replaced.modified().unwrap(), metadata.modified().unwrap());
        assert_ne!(original, replacement);
        assert_eq!(
            WindowsFileIdentity::from_nofollow_file(&file).unwrap(),
            Some(original)
        );
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"fLaC original");
        bytes.clear();
        replacement_file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"fLaC replaced");

        drop((file, replacement_file));
        fs::hard_link(&path, root.join("second-link.flac")).unwrap();
        assert!(open(&path).is_none());
        assert!(open(&root.join("second-link.flac")).is_none());
        assert!(open(&root).is_none());
        assert_eq!(
            WindowsFileIdentity::open_local_fixed_ntfs_nofollow(&root.join("missing.flac"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    #[ignore = "requires fixed local NTFS and Windows symlink creation privilege"]
    fn fixed_drive_handle_rejects_real_leaf_and_ancestor_reparse_points() {
        use std::os::windows::fs::{symlink_dir, symlink_file};

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let ordinary = root.join("ordinary");
        let target = ordinary.join("target.flac");
        fs::create_dir(&ordinary).unwrap();
        fs::write(&target, b"fLaC synthetic").unwrap();
        assert!(open(&target).is_some());
        let leaf = root.join("leaf.flac");
        let ancestor = root.join("ancestor");
        symlink_file(&target, &leaf).unwrap();
        symlink_dir(&ordinary, &ancestor).unwrap();
        assert_eq!(fs::read(&leaf).unwrap(), b"fLaC synthetic");
        assert_eq!(
            fs::read(ancestor.join("target.flac")).unwrap(),
            b"fLaC synthetic"
        );
        assert!(open(&leaf).is_none());
        assert!(open(&ancestor.join("target.flac")).is_none());
    }
}
