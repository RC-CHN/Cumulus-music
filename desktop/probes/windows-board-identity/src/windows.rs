use crate::{
    policy::{Source, complete_short, select},
    windows_identity_board_rules::board_serial,
};
use std::{
    fmt,
    ptr::{null, null_mut},
};
use windows::{
    Win32::{
        Foundation::{GetLastError, SetLastError, WIN32_ERROR},
        Globalization::{CPINFOEXW, GetACP, GetCPInfoExW},
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoInitializeSecurity, CoSetProxyBlanket, CoUninitialize, EOAC_NONE,
                RPC_C_AUTHN_LEVEL_CALL, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
            },
            Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE},
            Variant::{VARIANT, VT_BSTR, VariantClear},
            Wmi::{
                IWbemClassObject, IWbemContext, IWbemLocator, IWbemServices,
                WBEM_FLAG_CONNECT_USE_MAX_WAIT, WBEM_FLAG_FORWARD_ONLY,
                WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WBEM_S_TIMEDOUT, WbemLocator,
            },
        },
    },
    core::{BSTR, PCWSTR, w},
};

const CP_ACP: u32 = 0;
const CP_THREAD_ACP: u32 = 3;
const MAX_BSTR_UNITS: usize = 4_096;
const MAX_NARROW_BYTES: usize = MAX_BSTR_UNITS * 4 + 1;
const DIGIT_MASK: u16 = 0x4;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn WideCharToMultiByte(
        cp: u32,
        flags: u32,
        source: *const u16,
        len: i32,
        out: *mut u8,
        cap: i32,
        default_char: *const u8,
        used_default_char: *mut i32,
    ) -> i32;
    fn MultiByteToWideChar(
        cp: u32,
        flags: u32,
        source: *const u8,
        len: i32,
        out: *mut u16,
        cap: i32,
    ) -> i32;
}

#[link(name = "ucrt")]
unsafe extern "C" {
    fn iswctype(unit: u16, mask: u16) -> i32;
    fn _wcslwr_s(buffer: *mut u16, capacity: usize) -> i32;
}

struct ComThread;

impl Drop for ComThread {
    fn drop(&mut self) {
        // SAFETY: the successful CoInitializeEx occurred on this thread. All
        // interfaces and VARIANTs are dropped before the thread guard.
        unsafe { CoUninitialize() };
    }
}

struct Property(VARIANT);

impl Drop for Property {
    fn drop(&mut self) {
        // SAFETY: Get received an initialized VARIANT. VariantClear owns the
        // contained BSTR (or other tag's data) and runs before COM uninitializes.
        let _ = unsafe { VariantClear(&mut self.0) };
    }
}

#[derive(Clone, Copy, Debug)]
enum ReadState {
    QueryFailed,
    NextFailed,
    TimedOut,
    NoObject,
    BadObjectCount,
    PropertyFailed,
    NotBstr,
    TooLong,
    Empty,
    Present,
}

enum Reading {
    State(ReadState, Option<i32>),
    Serial(Vec<u16>),
}

impl Reading {
    fn state(&self) -> (ReadState, Option<i32>) {
        match self {
            Self::State(stage, error) => (*stage, *error),
            Self::Serial(_) => (ReadState::Present, None),
        }
    }
}

fn first_serial(services: &IWbemServices, source: Source) -> Reading {
    let query = match source {
        Source::Board => "SELECT * FROM Win32_BaseBoard ",
        Source::Bios => "SELECT * FROM Win32_BIOS ",
    };
    let flags = WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_RETURN_IMMEDIATELY.0 | WBEM_FLAG_FORWARD_ONLY.0);
    // SAFETY: the in-thread proxy has a fixed local namespace, query is one of
    // two fixed WQL statements, and no input context is supplied.
    let enumerator = match unsafe {
        services.ExecQuery(
            &BSTR::from("WQL"),
            &BSTR::from(query),
            flags,
            None::<&IWbemContext>,
        )
    } {
        Ok(value) => value,
        Err(error) => return Reading::State(ReadState::QueryFailed, Some(error.code().0)),
    };
    let mut first: [Option<IWbemClassObject>; 1] = [None];
    let mut count = 0;
    // SAFETY: exactly one correctly sized output slot and a live count
    // pointer. No retries or scanning other rows, even on missing property.
    let status = unsafe { enumerator.Next(1_500, &mut first, &mut count) };
    if status.is_err() {
        return Reading::State(ReadState::NextFailed, Some(status.0));
    }
    let [object] = first;
    let object = match (count, object) {
        (1, Some(object)) => object,
        (0, None) if status.0 == WBEM_S_TIMEDOUT.0 => {
            return Reading::State(ReadState::TimedOut, Some(status.0));
        }
        (0, None) => return Reading::State(ReadState::NoObject, Some(status.0)),
        _ => return Reading::State(ReadState::BadObjectCount, Some(status.0)),
    };
    let mut value = Property(VARIANT::default());
    // SAFETY: initialized output, fixed property and live first object.
    if let Err(error) = unsafe { object.Get(w!("SerialNumber"), 0, &mut value.0, None, None) } {
        return Reading::State(ReadState::PropertyFailed, Some(error.code().0));
    }
    // SAFETY: VT is the tag for a VARIANT returned by Get. Only an exact
    // non-BYREF VT_BSTR is borrowed as a BSTR, leaving VariantClear ownership.
    let variant = unsafe { &value.0.Anonymous.Anonymous };
    if variant.vt != VT_BSTR {
        return Reading::State(ReadState::NotBstr, Some(i32::from(variant.vt.0)));
    }
    let serial = unsafe { &variant.Anonymous.bstrVal };
    if serial.len() > MAX_BSTR_UNITS {
        return Reading::State(ReadState::TooLong, None);
    }
    if serial.is_empty() || serial[0] == 0 {
        return Reading::State(ReadState::Empty, None);
    }
    Reading::Serial(serial.to_vec())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Checked {
    Unavailable,
    EncodingUnavailable,
    EmptyAfterConversion,
    ShortBufferMismatch,
    CrtUnavailable,
    Filtered,
    Accepted,
}

fn check(read: &Reading) -> Checked {
    let Reading::Serial(units) = read else {
        return Checked::Unavailable;
    };
    let mut wide = units.clone();
    wide.push(0);
    // SAFETY: input is bounded and NUL-terminated. The size query has no
    // destination and uses the original CP_ACP/flags=0/null defaults.
    let encode = |out, capacity| unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            wide.as_ptr(),
            -1,
            out,
            capacity,
            null(),
            null_mut(),
        )
    };
    let required = encode(null_mut(), 0);
    let Ok(count) = usize::try_from(required) else {
        return Checked::EncodingUnavailable;
    };
    if !(1..=MAX_NARROW_BYTES).contains(&count) {
        return Checked::EncodingUnavailable;
    }
    let mut narrow = vec![0xA5; count + 1];
    if encode(narrow.as_mut_ptr(), required) != required || narrow[count - 1] != 0 {
        return Checked::EncodingUnavailable;
    }
    narrow[count] = 0; // D02970 appends its own NUL after the reported output.

    // SAFETY: narrow has a NUL at or before count-1. Destinations used below
    // are live and disjoint; the sizing query receives null/zero.
    let decode = |out, capacity| unsafe {
        MultiByteToWideChar(CP_THREAD_ACP, 0, narrow.as_ptr(), -1, out, capacity)
    };
    let required = decode(null_mut(), 0);
    let Ok(count) = usize::try_from(required) else {
        return Checked::EncodingUnavailable;
    };
    if count == 1 {
        return Checked::EmptyAfterConversion;
    }
    if !(2..=MAX_NARROW_BYTES).contains(&count) {
        return Checked::EncodingUnavailable;
    }
    let mut control = vec![0xFDD0; count];
    if decode(control.as_mut_ptr(), required) != required || control[count - 1] != 0 {
        return Checked::EncodingUnavailable;
    }
    // Initialize to a value different from every expected unit: a partial
    // write cannot coincidentally match the untouched tail. Retain a canary
    // in the terminal slot the short Win32 call was not allowed to write.
    let mut short: Vec<_> = control.iter().map(|&unit| !unit).collect();
    // SAFETY: this only clears thread-local last-error before the call.
    unsafe { SetLastError(WIN32_ERROR(0)) };
    let returned = decode(short.as_mut_ptr(), required - 1);
    // SAFETY: read last-error immediately from the same thread.
    let error = unsafe { GetLastError().0 };
    if short[count - 1] != u16::MAX
        || !complete_short(&control[..count - 1], &short[..count - 1], returned, error)
    {
        return Checked::ShortBufferMismatch;
    }
    short[count - 1] = 0; // CF7680 terminates the copied units itself.
    let mut crt_ok = true;
    let selected = board_serial(
        &short,
        // SAFETY: CRT accepts both values by copy; no pointers.
        |unit| unsafe { iswctype(unit, DIGIT_MASK) != 0 },
        // SAFETY: board_serial provides a writable copy with its own NUL.
        |text| crt_ok = unsafe { _wcslwr_s(text.as_mut_ptr(), text.len()) } == 0,
    );
    if !crt_ok {
        Checked::CrtUnavailable
    } else if selected.is_empty() {
        Checked::Filtered
    } else {
        Checked::Accepted
    }
}

pub(super) struct Startup {
    stage: &'static str,
    code: i32,
}
impl fmt::Display for Startup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "board_probe=startup_failed stage={} hresult=0x{:08x}",
            self.stage, self.code
        )
    }
}

pub(super) struct Report {
    acp: u32,
    thread_acp: Option<u32>,
    board: ((ReadState, Option<i32>), Checked),
    bios: ((ReadState, Option<i32>), Option<Checked>),
    selected: Option<Source>,
}
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "board_probe=complete acp={} thread_acp={:?} board={:?} bios={:?} selected={:?} private_values_emitted=0",
            self.acp, self.thread_acp, self.board, self.bios, self.selected
        )
    }
}

pub(super) fn run() -> Result<Report, Startup> {
    let stage = |stage| {
        move |error: windows::core::Error| Startup {
            stage,
            code: error.code().0,
        }
    };
    // SAFETY: fresh worker owns an MTA. S_OK and S_FALSE both require a
    // matching CoUninitialize, provided by ComThread.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(stage("com"))?;
    let _com = ComThread;
    // SAFETY: this process is solely the offline probe, not the app/GUI.
    // CF7D80 uses default authn with impersonation and no custom descriptor.
    unsafe {
        CoInitializeSecurity(
            None,
            -1,
            None,
            None,
            RPC_C_AUTHN_LEVEL_DEFAULT,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
            None,
        )
    }
    .map_err(stage("security"))?;
    // SAFETY: in-process WbemLocator on the initialized worker, no aggregation.
    let locator: IWbemLocator =
        unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }
            .map_err(stage("locator"))?;
    let empty = BSTR::new();
    // SAFETY: fixed local namespace, no credentials, no remote host; the
    // OS wait flag plus the separate process deadline bound the probe.
    let services = unsafe {
        locator.ConnectServer(
            &BSTR::from(r"ROOT\CIMV2"),
            &empty,
            &empty,
            &empty,
            WBEM_FLAG_CONNECT_USE_MAX_WAIT.0,
            &empty,
            None::<&IWbemContext>,
        )
    }
    .map_err(stage("local_namespace"))?;
    // SAFETY: only this local proxy is configured with CF7D80's 10/0/3/3
    // services and levels; null principal and auth info, no remote target.
    unsafe {
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
    .map_err(stage("proxy"))?;
    let observed = select(
        |source| first_serial(&services, source),
        check,
        |checked| *checked == Checked::Accepted,
    );
    let mut info = CPINFOEXW::default();
    // SAFETY: these query the current aliases only and write to one live
    // CPINFOEXW output. They do not set locale or code pages.
    let thread_acp = unsafe { GetCPInfoExW(CP_THREAD_ACP, 0, &mut info) }
        .ok()
        .map(|()| info.CodePage);
    let acp = unsafe { GetACP() };
    Ok(Report {
        acp,
        thread_acp,
        board: (observed.board.0.state(), observed.board.1),
        bios: (observed.bios.0.state(), observed.bios.1),
        selected: observed.selected,
    })
}
