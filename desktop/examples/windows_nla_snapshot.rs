//! Passive Windows-only proof for the recovered PC os.isOnLine sample.
//! Emits call outcome categories and a boolean; it is not a production event source.
#![cfg_attr(not(any(test, all(windows, target_arch = "x86_64"))), allow(dead_code))]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Snapshot {
    ProviderPresent,
    ProviderResultTooSmall,
    NoMoreProviders,
    BeginFailed,
    OtherNextFailure,
}

impl Snapshot {
    fn from_next(status: std::result::Result<(), i32>) -> Self {
        match status {
            Ok(()) => Self::ProviderPresent,
            Err(10014) => Self::ProviderResultTooSmall, // WSAEFAULT
            Err(10102 | 10110) => Self::NoMoreProviders, // WSAENOMORE / WSA_E_NO_MORE
            Err(_) => Self::OtherNextFailure,
        }
    }

    fn official_online(self) -> bool {
        !matches!(self, Self::NoMoreProviders | Self::OtherNextFailure)
    }
}

trait LookupApi {
    type Session;
    type Lookup;

    fn startup(&mut self) -> Option<Self::Session>;
    fn begin(&mut self, session: &mut Self::Session) -> Option<Self::Lookup>;
    fn next(&mut self, lookup: &mut Self::Lookup) -> bool;
    fn last_error(&mut self) -> i32;
    fn end(&mut self, lookup: Self::Lookup) -> bool;
    fn cleanup(&mut self, session: Self::Session) -> bool;
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Probe {
    sample: Option<Snapshot>,
    startup: bool,
    begin: Option<bool>,
    next: Option<bool>,
    last_error_read: bool,
    end: Option<bool>,
    cleanup: Option<bool>,
}

fn read(api: &mut impl LookupApi) -> Probe {
    let mut report = Probe::default();
    let Some(mut session) = api.startup() else {
        return report;
    };
    report.startup = true;
    let lookup = api.begin(&mut session);
    report.begin = Some(lookup.is_some());
    report.sample = Some(if let Some(mut lookup) = lookup {
        let succeeded = api.next(&mut lookup);
        report.next = Some(succeeded);
        let status = if succeeded {
            Ok(())
        } else {
            report.last_error_read = true;
            Err(api.last_error())
        };
        report.end = Some(api.end(lookup));
        Snapshot::from_next(status)
    } else {
        Snapshot::BeginFailed
    });
    report.cleanup = Some(api.cleanup(session));
    report
}

fn status(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "\"ok\"",
        Some(false) => "\"error\"",
        None => "null",
    }
}

impl Probe {
    fn json(&self) -> String {
        let (sample, online) = match self.sample {
            Some(sample) => (
                format!("\"{sample:?}\""),
                if sample.official_online() {
                    "true"
                } else {
                    "false"
                },
            ),
            None => ("null".to_owned(), "null"),
        };
        format!(
            concat!(
                "{{\"actualWindowsNla\":true,\"applicationNetworkRequests\":0,",
                "\"parsedProviderRecords\":0,\"listenerEnabled\":false,\"skipEnabled\":false,",
                "\"sample\":{},\"officialOnline\":{},\"calls\":{{\"startup\":{},\"begin\":{},",
                "\"next\":{},\"getLastError\":{},\"end\":{},\"cleanup\":{}}}}}"
            ),
            sample,
            online,
            status(Some(self.startup)),
            status(self.begin),
            status(self.next),
            self.last_error_read,
            status(self.end),
            status(self.cleanup)
        )
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use super::LookupApi;
    use std::{mem::size_of, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Networking::WinSock::{
            LUP_RETURN_ALL, LUP_RETURN_NAME, NS_NLA, WSACleanup, WSADATA, WSAGetLastError,
            WSALookupServiceBeginW, WSALookupServiceEnd, WSALookupServiceNextW, WSAQUERYSETW,
            WSAStartup,
        },
    };

    pub(super) struct Winsock(bool);

    impl Winsock {
        fn close(&mut self) -> bool {
            if !std::mem::take(&mut self.0) {
                return false;
            }
            // SAFETY: an active guard exists only after one successful WSAStartup.
            unsafe { WSACleanup() == 0 }
        }
    }

    impl Drop for Winsock {
        fn drop(&mut self) {
            self.close();
        }
    }

    pub(super) struct Lookup {
        handle: HANDLE,
        active: bool,
    }

    impl Lookup {
        fn close(&mut self) -> bool {
            if !std::mem::take(&mut self.active) {
                return false;
            }
            // SAFETY: one successful WSALookupServiceBeginW owns this handle.
            unsafe { WSALookupServiceEnd(self.handle) == 0 }
        }
    }

    impl Drop for Lookup {
        fn drop(&mut self) {
            self.close();
        }
    }

    #[repr(C)]
    struct QueryBuffer {
        header: WSAQUERYSETW,
        extra: [u8; 256],
    }

    const _: () = assert!(size_of::<WSAQUERYSETW>() == 120 && size_of::<QueryBuffer>() == 376);

    pub(super) struct Native;

    impl LookupApi for Native {
        type Session = Winsock;
        type Lookup = Lookup;

        fn startup(&mut self) -> Option<Winsock> {
            let mut data = WSADATA::default();
            // SAFETY: data is live and writable; Winsock version is 2.2.
            let started = unsafe { WSAStartup(0x0202, &mut data) == 0 };
            started.then(|| Winsock(true))
        }

        fn begin(&mut self, _session: &mut Winsock) -> Option<Lookup> {
            let restrictions = WSAQUERYSETW {
                dwSize: size_of::<WSAQUERYSETW>() as u32,
                dwNameSpace: NS_NLA,
                ..Default::default()
            };
            let mut handle = null_mut();
            // SAFETY: WinSock restrictions and output handle live for the call.
            // NS_NLA queries the local Network Location provider.
            let started =
                unsafe { WSALookupServiceBeginW(&restrictions, LUP_RETURN_ALL, &mut handle) == 0 };
            started.then(|| Lookup {
                handle,
                active: true,
            })
        }

        fn next(&mut self, lookup: &mut Lookup) -> bool {
            let mut buffer = QueryBuffer {
                header: WSAQUERYSETW {
                    dwSize: size_of::<WSAQUERYSETW>() as u32,
                    ..Default::default()
                },
                extra: [0; 256],
            };
            let mut size = size_of::<QueryBuffer>() as u32;
            // SAFETY: repr(C) storage starts with the aligned header and covers
            // exactly size writable bytes. Returned records are never read.
            unsafe {
                WSALookupServiceNextW(
                    lookup.handle,
                    LUP_RETURN_NAME,
                    &mut size,
                    std::ptr::addr_of_mut!(buffer).cast(),
                ) == 0
            }
        }

        fn last_error(&mut self) -> i32 {
            // SAFETY: read the immediately preceding Winsock error on this thread.
            unsafe { WSAGetLastError() }
        }

        fn end(&mut self, mut lookup: Lookup) -> bool {
            lookup.close()
        }

        fn cleanup(&mut self, mut session: Winsock) -> bool {
            session.close()
        }
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    let report = read(&mut windows::Native);
    println!("{}", report.json());
    if !report.startup {
        std::process::exit(1);
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!("The recovered NLA sample is available only on Windows x86_64.");
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::{LookupApi, Snapshot, read};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Call {
        Startup,
        Begin,
        Next,
        LastError,
        End,
        Cleanup,
    }

    struct Fixture {
        startup: bool,
        begin: bool,
        next: Result<(), i32>,
        end: bool,
        cleanup: bool,
        calls: Vec<Call>,
    }

    impl Default for Fixture {
        fn default() -> Self {
            Self {
                startup: true,
                begin: true,
                next: Ok(()),
                end: true,
                cleanup: true,
                calls: Vec::new(),
            }
        }
    }

    impl LookupApi for Fixture {
        type Session = ();
        type Lookup = ();

        fn startup(&mut self) -> Option<()> {
            self.calls.push(Call::Startup);
            self.startup.then_some(())
        }

        fn begin(&mut self, _: &mut ()) -> Option<()> {
            self.calls.push(Call::Begin);
            self.begin.then_some(())
        }

        fn next(&mut self, _: &mut ()) -> bool {
            self.calls.push(Call::Next);
            self.next.is_ok()
        }

        fn last_error(&mut self) -> i32 {
            self.calls.push(Call::LastError);
            self.next.unwrap_err()
        }

        fn end(&mut self, _: ()) -> bool {
            self.calls.push(Call::End);
            self.end
        }

        fn cleanup(&mut self, _: ()) -> bool {
            self.calls.push(Call::Cleanup);
            self.cleanup
        }
    }

    #[test]
    fn startup_and_begin_failure_do_not_use_unowned_lookup_resources() {
        use Call::*;
        let mut startup = Fixture {
            startup: false,
            ..Fixture::default()
        };
        let report = read(&mut startup);
        assert_eq!(startup.calls, [Startup]);
        assert!(!report.startup);
        assert_eq!(report.sample, None);
        let mut begin = Fixture {
            begin: false,
            ..Fixture::default()
        };
        let report = read(&mut begin);
        assert_eq!(begin.calls, [Startup, Begin, Cleanup]);
        assert_eq!(report.sample, Some(Snapshot::BeginFailed));
        assert!(report.sample.unwrap().official_online());
        assert_eq!(report.begin, Some(false));
        assert_eq!(report.cleanup, Some(true));
        assert_eq!(report.end, None);
    }

    #[test]
    fn one_next_and_one_end_match_the_native_oracle_even_when_cleanup_fails() {
        use Call::*;
        let cases = [
            (Ok(()), Snapshot::ProviderPresent, true),
            (Err(10014), Snapshot::ProviderResultTooSmall, true),
            (Err(10102), Snapshot::NoMoreProviders, false),
            (Err(10110), Snapshot::NoMoreProviders, false),
            (Err(10022), Snapshot::OtherNextFailure, false),
        ];
        for (next, sample, online) in cases {
            for (end, cleanup) in [(true, true), (false, true), (true, false), (false, false)] {
                let mut fixture = Fixture {
                    next,
                    end,
                    cleanup,
                    ..Fixture::default()
                };
                let report = read(&mut fixture);
                let expected = if next.is_ok() {
                    [Startup, Begin, Next, End, Cleanup].as_slice()
                } else {
                    [Startup, Begin, Next, LastError, End, Cleanup].as_slice()
                };
                assert_eq!(fixture.calls, expected);
                assert_eq!(report.sample, Some(sample));
                assert_eq!(report.sample.unwrap().official_online(), online);
                assert_eq!(report.end, Some(end));
                assert_eq!(report.cleanup, Some(cleanup));
                let json = report.json();
                assert!(json.contains(&format!("\"officialOnline\":{online},")));
                assert!(!json.contains("10014") && !json.contains("10022"));
            }
        }
    }
}
