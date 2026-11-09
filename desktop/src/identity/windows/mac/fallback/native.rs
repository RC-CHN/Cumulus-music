use super::super::{MAX_WIDE, native::permanent_address};
use super::{
    Adapter, ETHERNET, FallbackError, FallbackObservation, Records, select, short_name_buffer,
    verified_short_name,
};
use std::{mem::offset_of, ptr};
use windows_sys::Win32::{
    Foundation::{
        ERROR_BUFFER_OVERFLOW, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_DATA, ERROR_SUCCESS,
        GetLastError, SetLastError,
    },
    Globalization::{CP_THREAD_ACP, MultiByteToWideChar},
    NetworkManagement::IpHelper::{GetAdaptersInfo, IP_ADAPTER_INFO, MIB_IF_TYPE_ETHERNET},
    Networking::WinSock::{
        AF_INET, INADDR_NONE, WSACleanup, WSADATA, WSAStartup, gethostbyname, gethostname,
        inet_addr,
    },
};

const INITIAL_BYTES: usize = 0x2c0;
const MAX_BYTES: usize = 1024 * 1024;
const _: () = {
    assert!(ETHERNET == MIB_IF_TYPE_ETHERNET);
    assert!(size_of::<IP_ADAPTER_INFO>() == INITIAL_BYTES);
    assert!(align_of::<IP_ADAPTER_INFO>() == align_of::<u64>());
    assert!(offset_of!(IP_ADAPTER_INFO, AdapterName) == 0x0c);
    assert!(offset_of!(IP_ADAPTER_INFO, Address) == 0x198);
    assert!(offset_of!(IP_ADAPTER_INFO, Type) == 0x1a4);
    assert!(offset_of!(IP_ADAPTER_INFO, IpAddressList) == 0x1b8);
};

struct Winsock;

impl Winsock {
    fn start() -> Result<Self, FallbackError> {
        let mut info = WSADATA::default();
        // SAFETY: info is a writable WSADATA, and this guard owns one startup.
        if unsafe { WSAStartup(2, &mut info) } != 0 {
            return Err(FallbackError::WinsockStartup);
        }
        Ok(Self)
    }

    fn first_ipv4(&self) -> Result<u32, FallbackError> {
        let mut name = [0u8; MAX_WIDE];
        // SAFETY: the byte array is writable and the given capacity is exact.
        if unsafe { gethostname(name.as_mut_ptr(), MAX_WIDE as i32) } != 0
            || name.first() == Some(&0)
            || !name.contains(&0)
        {
            return Err(FallbackError::HostName);
        }
        // SAFETY: the bounded hostname is NUL-terminated. Winsock owns the
        // result on this thread until the next Winsock call; copy the first
        // address before making one, and do not follow aliases or other IPs.
        let entry = unsafe { gethostbyname(name.as_ptr()) };
        if entry.is_null() || !entry.is_aligned() {
            return Err(FallbackError::HostIpv4);
        }
        // SAFETY: after null/alignment checks the returned pointer is the
        // documented OS-owned HOSTENT, in its Winsock lifetime.
        let entry = unsafe { &*entry };
        if entry.h_addrtype != AF_INET as i16
            || entry.h_length != 4
            || entry.h_addr_list.is_null()
            || !entry.h_addr_list.is_aligned()
        {
            return Err(FallbackError::HostIpv4);
        }
        // SAFETY: Winsock documents a null-terminated pointer array; even an
        // empty array has the first slot. The validated h_length is four.
        let first = unsafe { entry.h_addr_list.read() };
        if first.is_null() {
            return Err(FallbackError::HostIpv4);
        }
        // SAFETY: the non-null first address is readable for the declared
        // h_length and the byte array needs no alignment. Retain native bits.
        let address = u32::from_ne_bytes(unsafe { first.cast::<[u8; 4]>().read() });
        if address == INADDR_NONE {
            // inet_addr uses the same all-ones value as its parse failure.
            return Err(FallbackError::HostIpv4);
        }
        Ok(address)
    }
}

impl Drop for Winsock {
    fn drop(&mut self) {
        // SAFETY: this guard owns one successful WSAStartup.
        unsafe { WSACleanup() };
    }
}

struct Adapters {
    buffer: Vec<u64>,
    used: usize,
}

impl Adapters {
    fn buffer(bytes: usize) -> Result<Vec<u64>, FallbackError> {
        if !(INITIAL_BYTES..=MAX_BYTES).contains(&bytes) {
            return Err(FallbackError::AdapterList);
        }
        let words = bytes.div_ceil(size_of::<u64>());
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(words)
            .map_err(|_| FallbackError::AdapterList)?;
        buffer.resize(words, 0);
        Ok(buffer)
    }

    fn read() -> Result<Self, FallbackError> {
        let mut size = INITIAL_BYTES as u32;
        let mut buffer = Self::buffer(INITIAL_BYTES)?;
        // SAFETY: the typed output pointer is suitably aligned, owns the
        // requested writable capacity, and size is an in/out ULONG.
        let status = unsafe { GetAdaptersInfo(buffer.as_mut_ptr().cast(), &mut size) };
        if status == ERROR_BUFFER_OVERFLOW {
            buffer = Self::buffer(size as usize)?;
        } else if size as usize > INITIAL_BYTES {
            return Err(FallbackError::AdapterList);
        }
        // CEFD10 calls the API again even if the initial request succeeded.
        // Do not retain bytes from the first snapshot on buffer reuse.
        buffer.fill(0);
        let capacity = size as usize;
        // SAFETY: capacity is no larger than the initialized owned allocation,
        // and its address is stable through the call and subsequent reads.
        let status = unsafe { GetAdaptersInfo(buffer.as_mut_ptr().cast(), &mut size) };
        let used = size as usize;
        match status {
            ERROR_NO_DATA => Ok(Self { buffer, used: 0 }),
            ERROR_SUCCESS if (INITIAL_BYTES..=capacity).contains(&used) => {
                Ok(Self { buffer, used })
            }
            _ => Err(FallbackError::AdapterList),
        }
    }

    fn records(&self) -> impl Iterator<Item = Result<Adapter<'_>, FallbackError>> {
        let base = self.buffer.as_ptr().addr();
        let mut next = (self.used != 0).then_some(base);
        let mut layout = Records::new(
            base,
            self.used,
            INITIAL_BYTES,
            align_of::<IP_ADAPTER_INFO>(),
        );
        std::iter::from_fn(move || {
            let address = next.take()?;
            let offset = match layout.next_offset(address) {
                Ok(offset) => offset,
                Err(error) => return Some(Err(error)),
            };
            // SAFETY: layout checks alignment and the entire in-buffer record,
            // rejects repeats/overlap, and bounds the offset to the initialized
            // allocation. Derive provenance from the original buffer, not Next.
            // IP_ADAPTER_INFO contains only C integers, arrays and raw pointers.
            let record = unsafe {
                &*self
                    .buffer
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<IP_ADAPTER_INFO>()
            };
            next = (!record.Next.is_null()).then(|| record.Next.addr());
            Some(Ok(Adapter {
                kind: record.Type,
                first_ip: &record.IpAddressList.IpAddress.String,
                name: &record.AdapterName,
                address: &record.Address,
                address_length: record.AddressLength,
            }))
        })
    }
}

fn parse_ip(input: &[u8]) -> Option<u32> {
    if input.last() != Some(&0) {
        return None;
    }
    // SAFETY: select admits only a bounded NUL-terminated address string.
    let address = unsafe { inet_addr(input.as_ptr()) };
    (address != INADDR_NONE).then_some(address)
}

fn convert_name(input: &[u8]) -> Option<Vec<u16>> {
    if input.last() != Some(&0) {
        return None;
    }
    // SAFETY: the bounded NUL-terminated ANSI buffer lives through each call.
    let units =
        unsafe { MultiByteToWideChar(CP_THREAD_ACP, 0, input.as_ptr(), -1, ptr::null_mut(), 0) };
    if !(2..=MAX_WIDE as i32).contains(&units) {
        return None;
    }
    let mut control = vec![u16::MAX; units as usize];
    // SAFETY: control has the measured length, including one terminator.
    if unsafe {
        MultiByteToWideChar(
            CP_THREAD_ACP,
            0,
            input.as_ptr(),
            -1,
            control.as_mut_ptr(),
            units,
        )
    } != units
        || control.pop() != Some(0)
        || control.contains(&0)
    {
        return None;
    }
    let mut short = short_name_buffer(&control);
    // SAFETY: reproduce the second CEE4D0 call with one fewer output unit.
    // The destination is initialized to per-unit unequal values, with space
    // on both sides to check the observed write. GetLastError is immediate.
    let (written, error) = unsafe {
        SetLastError(ERROR_SUCCESS);
        let written = MultiByteToWideChar(
            CP_THREAD_ACP,
            0,
            input.as_ptr(),
            -1,
            short.as_mut_ptr().add(1),
            units - 1,
        );
        (written, GetLastError())
    };
    if written != 0 || error != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }
    // Use the bytes actually written by the legacy call, not the full control.
    verified_short_name(&short, &control).map(<[u16]>::to_vec)
}

pub(crate) fn collect_fallback_mac() -> Result<FallbackObservation, FallbackError> {
    let winsock = Winsock::start()?;
    let first = winsock.first_ipv4()?;
    let adapters = Adapters::read()?;
    select(
        first,
        adapters.records(),
        parse_ip,
        convert_name,
        permanent_address,
    )
}

#[cfg(test)]
mod tests {
    use super::convert_name;

    #[test]
    fn native_thread_acp_legacy_short_buffer_matches_control() {
        let input = b"{f4d82c44-0aba-4435-a476-1216e5055534}\0";
        let expected: Vec<_> = input[..input.len() - 1]
            .iter()
            .copied()
            .map(u16::from)
            .collect();
        assert_eq!(convert_name(input), Some(expected));
        assert!(convert_name(b"\0").is_none());
    }
}
