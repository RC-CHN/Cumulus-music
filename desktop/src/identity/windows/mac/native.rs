use super::{
    DriverReply, MAX_WIDE, NDIS_OUTPUT, NetworkCards, Observation, RegistryEntry, ServiceName,
    driver_reply, registry_units, scan,
};
use std::{mem::size_of_val, ptr};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GENERIC_READ, HANDLE,
        INVALID_HANDLE_VALUE,
    },
    NetworkManagement::Ndis::OID_802_3_PERMANENT_ADDRESS,
    Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, OPEN_EXISTING},
    System::{
        IO::DeviceIoControl,
        Registry::{
            HKEY, HKEY_LOCAL_MACHINE, REG_SZ, RegCloseKey, RegEnumKeyExW, RegOpenKeyW,
            RegQueryValueExW,
        },
    },
};

const NETWORK_CARDS: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkCards\";
// The observed NDIS query, not a request to change the current address.
const IOCTL_NDIS_QUERY_GLOBAL_STATS: u32 = 0x170002;

#[link(name = "ucrt")]
unsafe extern "C" {
    fn _wcslwr_s(buffer: *mut u16, count: usize) -> i32;
}

struct Key(HKEY);

impl Key {
    fn open_hklm(path: &[u16]) -> Option<Self> {
        debug_assert_eq!(path.last(), Some(&0));
        let mut handle = ptr::null_mut();
        // SAFETY: path is NUL terminated and handle is a writable HKEY slot.
        let status = unsafe { RegOpenKeyW(HKEY_LOCAL_MACHINE, path.as_ptr(), &mut handle) };
        if status == ERROR_SUCCESS && !handle.is_null() {
            Some(Self(handle))
        } else {
            None
        }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: this guard owns the returned, non-predefined registry key.
        unsafe { RegCloseKey(self.0) };
    }
}

struct Device(HANDLE);

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: this guard owns one successful CreateFileW result.
        unsafe { CloseHandle(self.0) };
    }
}

struct WindowsNetworkCards {
    root: Key,
    prefix: Vec<u16>,
}

enum ValueRead {
    Text(Vec<u16>),
    Failed(u32),
    Malformed,
}

fn read_text(key: &Key, value: &str, require_nul: bool) -> ValueRead {
    let name: Vec<_> = value.encode_utf16().chain([0]).collect();
    let mut buffer = [0u16; MAX_WIDE];
    let mut bytes = size_of_val(&buffer) as u32;
    let mut kind = REG_SZ;
    // SAFETY: the name is terminated and the typed value/length and data
    // outputs are writable; bytes initially describes the entire data array.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            ptr::null(),
            &mut kind,
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS {
        return ValueRead::Failed(status);
    }
    if kind != REG_SZ {
        return ValueRead::Malformed;
    }
    match registry_units(&buffer, bytes, require_nul) {
        Some(value) => ValueRead::Text(value.to_vec()),
        None => ValueRead::Malformed,
    }
}

fn lower_in_crt(mut text: Vec<u16>) -> Option<Vec<u16>> {
    text.push(0);
    // SAFETY: this owned slice has a terminator, and count includes it.
    // This reads the current CRT locale without changing it.
    let result = unsafe { _wcslwr_s(text.as_mut_ptr(), text.len()) };
    text.pop();
    (result == 0).then_some(text)
}

impl NetworkCards for WindowsNetworkCards {
    type Key = Key;

    fn entry(&self, index: u32) -> RegistryEntry<Key> {
        let mut name = [0u16; MAX_WIDE];
        let mut length = MAX_WIDE as u32;
        // SAFETY: the buffers are writable, and length is the capacity in
        // wide units. The optional outputs are not requested.
        let status = unsafe {
            RegEnumKeyExW(
                self.root.0,
                index,
                name.as_mut_ptr(),
                &mut length,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        // CF0130 exits the scan on any nonzero enumeration status.
        if status != ERROR_SUCCESS {
            return RegistryEntry::End;
        }
        let length = length as usize;
        if length == 0
            || name.get(length) != Some(&0)
            || name[..length]
                .iter()
                .any(|&unit| unit == 0 || unit == u16::from(b'\\'))
            || self.prefix.len() + length + 1 > MAX_WIDE
        {
            return RegistryEntry::Malformed;
        }
        let mut path = self.prefix.clone();
        path.extend_from_slice(&name[..length]);
        path.push(0);
        match Key::open_hklm(&path) {
            Some(key) => RegistryEntry::Open(key),
            None => RegistryEntry::Skip,
        }
    }

    fn lower_description(&self, key: &Key) -> Option<Vec<u16>> {
        let text = match read_text(key, "Description", false) {
            ValueRead::Text(text) => text,
            ValueRead::Failed(ERROR_FILE_NOT_FOUND) => Vec::new(),
            ValueRead::Failed(_) | ValueRead::Malformed => return None,
        };
        lower_in_crt(text)
    }

    fn service_name(&self, key: &Key) -> ServiceName {
        match read_text(key, "ServiceName", true) {
            ValueRead::Text(name) => ServiceName::Value(name),
            ValueRead::Failed(_) => ServiceName::Unreadable,
            ValueRead::Malformed => ServiceName::Malformed,
        }
    }

    fn permanent_address(&self, path: &[u16]) -> DriverReply {
        // SAFETY: scan passes only a terminated Win32 device name built from
        // the fixed registry key's leaf. No overlapped or inherited handle.
        let raw = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE || raw.is_null() {
            return DriverReply::Failed;
        }
        let device = Device(raw);
        let oid = OID_802_3_PERMANENT_ADDRESS;
        let mut output = [0; NDIS_OUTPUT];
        let mut reported = 0;
        // SAFETY: the valid device handle and both bounded buffers live until
        // this synchronous query returns; no OVERLAPPED survives this call.
        let success = unsafe {
            DeviceIoControl(
                device.0,
                IOCTL_NDIS_QUERY_GLOBAL_STATS,
                (&raw const oid).cast(),
                size_of_val(&oid) as u32,
                output.as_mut_ptr().cast(),
                NDIS_OUTPUT as u32,
                &mut reported,
                ptr::null_mut(),
            )
        };
        driver_reply(success != 0, reported, &output)
    }
}

pub(crate) fn collect_registry_mac() -> Observation {
    let mut prefix: Vec<_> = NETWORK_CARDS.encode_utf16().collect();
    prefix.push(0);
    let Some(root) = Key::open_hklm(&prefix) else {
        return Observation::FallbackRequired(super::Fallback::NoRegistryCandidate);
    };
    prefix.pop();
    scan(&WindowsNetworkCards { root, prefix })
}

#[cfg(test)]
mod tests {
    use super::lower_in_crt;
    use crate::mac::excluded_description;

    #[test]
    fn native_crt_lowers_mixed_case_description_without_changing_locale() {
        let text = "合成 ViRTuAL 网卡".encode_utf16().collect();
        let lower = lower_in_crt(text).expect("synthetic Windows CRT conversion");
        assert!(excluded_description(&lower));
        assert_eq!(lower_in_crt(Vec::new()), Some(Vec::new()));
    }
}
