//! Passive Windows-only proof for the recovered PC os.isOnLine sample.
//! Emits the result class and boolean only; it is not a production event source.

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
#[derive(Debug, Eq, PartialEq)]
enum Snapshot {
    ProviderPresent,
    ProviderResultTooSmall,
    NoMoreProviders,
    BeginFailed,
    OtherNextFailure,
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
impl Snapshot {
    fn from_next(status: std::result::Result<(), i32>) -> Self {
        match status {
            Ok(()) => Self::ProviderPresent,
            Err(10014) => Self::ProviderResultTooSmall, // WSAEFAULT
            Err(10102 | 10110) => Self::NoMoreProviders, // WSAENOMORE / WSA_E_NO_MORE
            Err(_) => Self::OtherNextFailure,
        }
    }

    fn official_online(&self) -> bool {
        !matches!(self, Self::NoMoreProviders | Self::OtherNextFailure)
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use super::Snapshot;
    use std::{mem::size_of, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Networking::WinSock::{
            LUP_RETURN_ALL, LUP_RETURN_NAME, NS_NLA, WSACleanup, WSADATA, WSAGetLastError,
            WSALookupServiceBeginW, WSALookupServiceEnd, WSALookupServiceNextW, WSAQUERYSETW,
            WSAStartup,
        },
    };

    struct Winsock;

    impl Drop for Winsock {
        fn drop(&mut self) {
            // SAFETY: a Winsock guard is created only after successful WSAStartup.
            unsafe { WSACleanup() };
        }
    }

    struct Lookup(HANDLE);

    impl Drop for Lookup {
        fn drop(&mut self) {
            // SAFETY: a successful WSALookupServiceBeginW created this handle.
            unsafe { WSALookupServiceEnd(self.0) };
        }
    }

    #[repr(C)]
    struct QueryBuffer {
        header: WSAQUERYSETW,
        extra: [u8; 256],
    }

    const _: () = assert!(size_of::<WSAQUERYSETW>() == 120 && size_of::<QueryBuffer>() == 376);

    pub(super) fn read() -> Result<Snapshot, i32> {
        let mut data = WSADATA::default();
        // SAFETY: data is live and writable; Winsock version is 2.2.
        let startup = unsafe { WSAStartup(0x0202, &mut data) };
        if startup != 0 {
            return Err(startup);
        }
        let _winsock = Winsock;
        let restrictions = WSAQUERYSETW {
            dwSize: size_of::<WSAQUERYSETW>() as u32,
            dwNameSpace: NS_NLA,
            ..Default::default()
        };
        let mut lookup = null_mut();
        // SAFETY: restrictions and the output handle have the WinSock ABI and
        // remain live for the call. NS_NLA is the local Network Location provider.
        if unsafe { WSALookupServiceBeginW(&restrictions, LUP_RETURN_ALL, &mut lookup) } != 0 {
            return Ok(Snapshot::BeginFailed);
        }
        let lookup = Lookup(lookup);
        let mut buffer = QueryBuffer {
            header: WSAQUERYSETW {
                dwSize: size_of::<WSAQUERYSETW>() as u32,
                ..Default::default()
            },
            extra: [0; 256],
        };
        let mut size = size_of::<QueryBuffer>() as u32;
        // SAFETY: the repr(C) storage starts with an aligned header and covers
        // exactly size writable bytes. No returned pointers or data are read.
        let status = unsafe {
            WSALookupServiceNextW(
                lookup.0,
                LUP_RETURN_NAME,
                &mut size,
                std::ptr::addr_of_mut!(buffer).cast(),
            )
        };
        if status == 0 {
            Ok(Snapshot::from_next(Ok(())))
        } else {
            // SAFETY: get the immediately preceding Winsock error on this thread.
            Ok(Snapshot::from_next(Err(unsafe { WSAGetLastError() })))
        }
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    match windows::read() {
        Ok(sample) => println!(
            "sample={sample:?} officialOnline={}",
            sample.official_online()
        ),
        Err(_) => {
            eprintln!("Winsock startup unavailable");
            std::process::exit(1);
        }
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!("The recovered NLA sample is available only on Windows x86_64.");
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::Snapshot;

    #[test]
    fn recovered_binary_treats_unknown_as_online_but_a_failed_next_as_offline() {
        let cases = [
            (Ok(()), Snapshot::ProviderPresent, true),
            (Err(10014), Snapshot::ProviderResultTooSmall, true),
            (Err(10102), Snapshot::NoMoreProviders, false),
            (Err(10110), Snapshot::NoMoreProviders, false),
            (Err(10022), Snapshot::OtherNextFailure, false),
        ];
        for (status, expected, online) in cases {
            let sample = Snapshot::from_next(status);
            assert_eq!(sample, expected);
            assert_eq!(sample.official_online(), online);
        }
        assert!(Snapshot::BeginFailed.official_online());
    }
}
