use crate::{
    policy::{Source, complete_short, select},
    windows_identity_board_rules::board_serial,
    wmi::{
        Failure, MAX_CONVERSION_UNITS, ReadState, Startup, acp_with_terminal, first_property,
        with_local_wmi,
    },
};
use std::{fmt, ptr::null_mut};
use windows::{
    Win32::{
        Foundation::{GetLastError, SetLastError, WIN32_ERROR},
        Globalization::{CPINFOEXW, GetACP, GetCPInfoExW},
        System::Wmi::IWbemServices,
    },
    core::w,
};

const CP_THREAD_ACP: u32 = 3;
const DIGIT_MASK: u16 = 0x4;

#[link(name = "kernel32")]
unsafe extern "system" {
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
    match first_property(services, query, w!("SerialNumber"))
        .and_then(|value| value.bounded_bstr().map(<[u16]>::to_vec))
    {
        Ok(units) => Reading::Serial(units),
        Err(Failure(state, detail)) => Reading::State(state, detail),
    }
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
    let Some(narrow) = acp_with_terminal(units) else {
        return Checked::EncodingUnavailable;
    };

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
    if !(2..=MAX_CONVERSION_UNITS).contains(&count) {
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
    with_local_wmi(observe)
}

fn observe(services: &IWbemServices) -> Report {
    let observed = select(
        |source| first_serial(services, source),
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
    Report {
        acp,
        thread_acp,
        board: (observed.board.0.state(), observed.board.1),
        bios: (observed.bios.0.state(), observed.bios.1),
        selected: observed.selected,
    }
}
