//! Win32 ACL and handle access are identical in production and in the native lab.
use super::identity::Identity;
use std::{
    ffi::OsStr,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{FromRawHandle, OwnedHandle},
    },
    ptr,
};
use windows_sys::Win32::{
    Foundation::{INVALID_HANDLE_VALUE, LocalFree},
    Security::{
        Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_WRITE_DATA,
        OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, SYNCHRONIZE,
    },
};

pub(crate) fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

pub(crate) struct Descriptor(pub(crate) PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: unique pointer allocated by the SDDL conversion routine.
        unsafe { LocalFree(self.0) };
    }
}

pub(crate) fn with_logon_security<T>(
    identity: &Identity,
    create: impl FnOnce(&mut SECURITY_ATTRIBUTES) -> io::Result<T>,
) -> io::Result<T> {
    // Protected DACL with exactly the current Windows logon SID; default
    // named-pipe permissions include Everyone read and are not used.
    let text = wide(&format!("D:P(A;;GA;;;{})", identity.logon_sddl()));
    let mut raw = ptr::null_mut();
    // SAFETY: null-terminated SDDL and writable descriptor out-pointer.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            SDDL_REVISION_1,
            &mut raw,
            ptr::null_mut(),
        )
    } == 0
        || raw.is_null()
    {
        return Err(io::Error::last_os_error());
    }
    let descriptor = Descriptor(raw);
    create(&mut SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    })
}

pub(crate) fn open_handle(address: &str) -> io::Result<OwnedHandle> {
    let address = wide(address);
    // SAFETY: exact locally scoped pipe name, non-inheritable handle with only
    // individual client rights (no FILE_CREATE_PIPE_INSTANCE) and no
    // impersonation authority. Busy is returned immediately, never waited.
    let raw = unsafe {
        CreateFileW(
            address.as_ptr(),
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful CreateFileW returned an owned pipe handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}
