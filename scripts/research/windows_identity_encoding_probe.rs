//! Offline Win32 observations for the Windows 3.1.42 x64 identity recovery.
//! No hardware, registry, WMI, accounts, command-line input or network.

#[cfg(any(all(windows, target_arch = "x86_64"), test))]
mod windows_identity_board_rules;
#[cfg(any(all(windows, target_arch = "x86_64"), test))]
use windows_identity_board_rules::board_serial;

#[cfg(any(all(windows, target_arch = "x86_64"), test))]
fn board_cases() -> Vec<(&'static str, Vec<u16>, Vec<u16>)> {
    let mut cases: Vec<_> = [
        ("remove_ascii_spaces", "AB 12 CD", "AB12CD"),
        ("five_units", "Ab12C", ""),
        ("six_units", "Ab12Cd", "Ab12Cd"),
        ("only_digits_after_spaces", "12 3456", ""),
        ("placeholder_unknown", "ABUNKNOWN77", ""),
        ("placeholder_oem", "VendorOEM-01", ""),
        ("placeholder_dotted_oem", "ABO.E.M123", ""),
        ("placeholder_invalid", "AbInVaLiD-42", ""),
        ("placeholder_diy", "SNDiY1234", ""),
        ("preserve_original_case", "ABxD-012", "ABxD-012"),
        ("tab_is_not_ascii_space", "AB\t12CD", "AB\t12CD"),
        ("stop_at_nul", "Ab12Cd\0UNKNOWN", "Ab12Cd"),
    ]
    .into_iter()
    .map(|(name, input, expected)| {
        (
            name,
            input.encode_utf16().collect(),
            expected.encode_utf16().collect(),
        )
    })
    .collect();
    let prefix = vec![u16::from(b'A'); 79];
    cases.push(("seventy_nine_units", prefix.clone(), prefix.clone()));
    let mut over = prefix.clone();
    over.push(u16::from(b'B'));
    cases.push(("eighty_units", over, prefix.clone()));
    let mut over = prefix.clone();
    over.extend("OEM".encode_utf16());
    cases.push(("placeholder_beyond_cutoff", over, prefix));
    cases
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use super::{board_cases, board_serial};
    use std::ptr::{null, null_mut};

    const CP_ACP: u32 = 0;
    const CP_THREAD_ACP: u32 = 3;
    const MAX_OUTPUT: usize = 512;
    const WIDE_GUARD: u16 = 0xfdd0;
    const NARROW_GUARD: u8 = 0xa5;
    const DIGIT_MASK: u16 = 0x4;

    #[repr(C)]
    struct CpInfoEx {
        max_char_size: u32,
        default_char: [u8; 2],
        lead_byte: [u8; 12],
        unicode_default_char: u16,
        code_page: u32,
        code_page_name: [u16; 260],
    }
    const _: () = assert!(size_of::<CpInfoEx>() == 544);
    const _: () = assert!(std::mem::offset_of!(CpInfoEx, code_page) == 20);

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetACP() -> u32;
        fn GetCPInfoExW(code_page: u32, flags: u32, info: *mut CpInfoEx) -> i32;
        fn GetLastError() -> u32;
        fn SetLastError(code: u32);
        fn MultiByteToWideChar(
            code_page: u32,
            flags: u32,
            input: *const u8,
            input_len: i32,
            output: *mut u16,
            output_len: i32,
        ) -> i32;
        fn WideCharToMultiByte(
            code_page: u32,
            flags: u32,
            input: *const u16,
            input_len: i32,
            output: *mut u8,
            output_len: i32,
            default_char: *const u8,
            used_default_char: *mut i32,
        ) -> i32;
    }

    #[link(name = "ucrt")]
    unsafe extern "C" {
        fn iswctype(unit: u16, mask: u16) -> i32;
        fn _wcslwr_s(buffer: *mut u16, count: usize) -> i32;
    }

    #[derive(Debug)]
    struct Observation<T> {
        returned: i32,
        last_error: u32,
        output: Vec<T>,
    }

    fn code_page(id: u32) -> Option<u32> {
        let mut info = std::mem::MaybeUninit::<CpInfoEx>::zeroed();
        // SAFETY: the aligned output is CPINFOEXW; the OS initializes it on success.
        let (ok, error) = unsafe {
            SetLastError(0);
            let ok = GetCPInfoExW(id, 0, info.as_mut_ptr());
            (ok, GetLastError())
        };
        if ok == 0 {
            println!("code_page requested={id} last_error={error} unavailable=true");
            return None;
        }
        // SAFETY: every field has a valid all-zero representation and the OS
        // populated the fields we use on success.
        let info = unsafe { info.assume_init() };
        println!(
            "code_page requested={id} resolved={} max_char_size={} last_error={error}",
            info.code_page, info.max_char_size
        );
        Some(info.code_page)
    }

    fn decode(code_page: u32, input: &[u8], input_len: i32, capacity: usize) -> Observation<u16> {
        assert_eq!(input.last(), Some(&0));
        assert!(input_len == -1 || usize::try_from(input_len).is_ok_and(|n| n <= input.len()));
        assert!(capacity <= MAX_OUTPUT);
        let mut guarded = [WIDE_GUARD; MAX_OUTPUT + 2];
        let dst = if capacity == 0 {
            null_mut()
        } else {
            guarded[1..=capacity].as_mut_ptr()
        };
        // SAFETY: input has a NUL for -1 or a checked explicit length;
        // dst is null for sizing or has capacity writable u16s. No aliases.
        let (returned, last_error) = unsafe {
            SetLastError(0);
            let returned = MultiByteToWideChar(
                code_page,
                0,
                input.as_ptr(),
                input_len,
                dst,
                capacity as i32,
            );
            (returned, GetLastError())
        };
        assert_eq!(guarded[0], WIDE_GUARD, "wide prefix guard changed");
        assert!(
            guarded[capacity + 1..]
                .iter()
                .all(|unit| *unit == WIDE_GUARD),
            "wide suffix guard changed"
        );
        assert!(returned >= 0 && (capacity == 0 || returned as usize <= capacity));
        Observation {
            returned,
            last_error,
            output: guarded[1..1 + capacity].to_vec(),
        }
    }

    fn encode(code_page: u32, input: &[u16], capacity: usize) -> Observation<u8> {
        assert_eq!(input.last(), Some(&0));
        assert!(capacity <= MAX_OUTPUT);
        let mut guarded = [NARROW_GUARD; MAX_OUTPUT + 2];
        let dst = if capacity == 0 {
            null_mut()
        } else {
            guarded[1..=capacity].as_mut_ptr()
        };
        // SAFETY: input and dst are disjoint bounded buffers; input is NUL terminated.
        // Both default-character arguments are null, including with UTF-8 ACP.
        let (returned, last_error) = unsafe {
            SetLastError(0);
            let returned = WideCharToMultiByte(
                code_page,
                0,
                input.as_ptr(),
                -1,
                dst,
                capacity as i32,
                null(),
                null_mut(),
            );
            (returned, GetLastError())
        };
        assert_eq!(guarded[0], NARROW_GUARD, "narrow prefix guard changed");
        assert!(
            guarded[capacity + 1..]
                .iter()
                .all(|byte| *byte == NARROW_GUARD),
            "narrow suffix guard changed"
        );
        assert!(returned >= 0 && (capacity == 0 || returned as usize <= capacity));
        Observation {
            returned,
            last_error,
            output: guarded[1..1 + capacity].to_vec(),
        }
    }

    fn compare_decode(name: &str, cp: u32, bytes: &[u8]) -> Option<Vec<u16>> {
        let query = decode(cp, bytes, -1, 0);
        let required = query.returned as usize;
        if required == 0 {
            println!(
                "decode case={name} cp={cp} sizing_return=0 sizing_error={}",
                query.last_error
            );
            return None;
        }
        let full = decode(cp, bytes, -1, required);
        assert_eq!(full.returned, query.returned);
        assert_eq!(full.output.last(), Some(&0));
        println!(
            "decode case={name} cp={cp} sizing_return={} sizing_error={} full_return={} full_error={} full_units={:?}",
            query.returned, query.last_error, full.returned, full.last_error, full.output
        );
        // A zero-length destination would ask the OS to size again. The
        // recovered helper returns early when the count minus NUL is zero.
        if required == 1 {
            println!("decode case={name} cp={cp} short_skipped_empty=true");
            return Some(full.output);
        }
        let short = decode(cp, bytes, -1, required - 1);
        let explicit = decode(cp, bytes, (bytes.len() - 1) as i32, required - 1);
        let content = &full.output[..required - 1];
        // This is a diagnostic, not an assumption that the short call works.
        // Record memory before the recovered helper writes its own terminator.
        println!(
            "short case={name} cp={cp} return={} last_error={} changed_units={} matches_full_content={} units={:?}",
            short.returned,
            short.last_error,
            short.output.iter().filter(|v| **v != WIDE_GUARD).count(),
            short.output == content,
            short.output
        );
        println!(
            "explicit case={name} cp={cp} return={} last_error={} matches_full_content={} units={:?}",
            explicit.returned,
            explicit.last_error,
            explicit.output == content,
            explicit.output
        );
        Some(full.output)
    }

    pub fn run() {
        // SAFETY: GetACP has no input and queries only the process code page.
        let acp = unsafe { GetACP() };
        assert_ne!(acp, 0);
        println!("encoding_probe=v1 get_acp={acp}");
        let acp_info = code_page(CP_ACP);
        let thread_info = code_page(CP_THREAD_ACP);
        println!(
            "aliases acp_matches_get_acp={} thread_matches_get_acp={}",
            acp_info == Some(acp),
            thread_info == Some(acp)
        );

        for (name, text) in [
            ("ascii", "AB 12 CD"),
            ("legacy_mixed", "Grüße-AB17"),
            ("cjk", "设备-AB123"),
            ("supplementary", "SN𐐷-AB12"),
            ("empty", ""),
        ] {
            let wide: Vec<_> = text.encode_utf16().chain([0]).collect();
            let query = encode(CP_ACP, &wide, 0);
            assert!(
                query.returned > 0,
                "fixed Unicode input did not encode: {name}"
            );
            let narrow = encode(CP_ACP, &wide, query.returned as usize);
            assert_eq!(query.returned, narrow.returned);
            assert_eq!(narrow.output.last(), Some(&0));
            let direct = encode(acp, &wide, query.returned as usize);
            println!(
                "encode case={name} cp=0 sizing_return={} sizing_error={} return={} last_error={} equals_get_acp={} bytes={:?}",
                query.returned,
                query.last_error,
                narrow.returned,
                narrow.last_error,
                direct.returned == narrow.returned && direct.output == narrow.output,
                narrow.output
            );
            for (cp, resolved) in [(CP_ACP, acp_info), (CP_THREAD_ACP, thread_info)] {
                let decoded = compare_decode(name, cp, &narrow.output);
                if let Some(resolved) = resolved.filter(|id| *id > CP_THREAD_ACP) {
                    let query = decode(resolved, &narrow.output, -1, 0);
                    let explicit = (query.returned > 0).then(|| {
                        decode(resolved, &narrow.output, -1, query.returned as usize).output
                    });
                    println!(
                        "alias_decode case={name} cp={cp} resolved={resolved} matches_explicit={}",
                        decoded == explicit
                    );
                }
                println!(
                    "roundtrip case={name} cp={cp} equals_input={}",
                    decoded.as_ref() == Some(&wide)
                );
                if name == "ascii" {
                    assert_eq!(
                        decoded,
                        Some(wide.clone()),
                        "ASCII full-buffer control changed"
                    );
                }
            }
        }
        for (name, cp, input) in [
            ("fixed_1252", 1252, &[0x43, 0x41, 0x46, 0xc9, 0][..]),
            ("fixed_936", 936, &[0xd6, 0xf7, 0xbb, 0xfa, 0][..]),
            ("fixed_utf8", 65001, &[0x41, 0xc3, 0xa9, 0x5a, 0][..]),
            (
                "fixed_utf8_supplementary",
                65001,
                &b"X\xf0\x90\x90\xb7Z\0"[..],
            ),
            ("malformed_utf8", 65001, &[0xc3, 0x28, 0][..]),
        ] {
            if code_page(cp).is_some() {
                compare_decode(name, cp, input);
            }
        }
        for (name, unit) in [
            ("ascii_1", 0x31),
            ("arabic_indic_1", 0x661),
            ("fullwidth_1", 0xff11),
        ] {
            // SAFETY: value and mask are passed by value; no memory is borrowed.
            println!("digit case={name} class={}", unsafe {
                iswctype(unit, DIGIT_MASK) != 0
            });
        }
        for (name, input, expected) in board_cases() {
            let actual = board_serial(
                &input,
                // SAFETY: UCRT takes a wchar value and a wctype_t mask by value.
                |unit| unsafe { iswctype(unit, DIGIT_MASK) != 0 },
                // SAFETY: this owned copy is writable, NUL terminated, and len
                // includes the NUL. Tests here only freeze ASCII behavior.
                |text| assert_eq!(unsafe { _wcslwr_s(text.as_mut_ptr(), text.len()) }, 0),
            );
            println!(
                "board case={name} input_units={} result_units={} passed={}",
                input.len(),
                actual.len(),
                actual == expected
            );
            assert_eq!(actual, expected, "board fixture {name}");
        }
        println!("encoding_probe=complete synthetic_only=true hardware_reads=0");
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    assert_eq!(std::env::args_os().len(), 1, "this probe accepts no inputs");
    windows::run();
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() -> std::process::ExitCode {
    eprintln!("The native conversion probe requires Windows x86_64.");
    std::process::ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::{board_cases, board_serial};

    #[test]
    fn synthetic_board_rules_preserve_raw_utf16_units() {
        for (name, input, expected) in board_cases() {
            let actual = board_serial(
                &input,
                |unit| (0x30..=0x39).contains(&unit),
                |units| {
                    for unit in units {
                        if (0x41..=0x5a).contains(unit) {
                            *unit += 0x20;
                        }
                    }
                },
            );
            assert_eq!(actual, expected, "{name}");
        }
    }

    #[test]
    fn cutoff_counts_utf16_units_even_when_a_pair_straddles_it() {
        let mut input = vec![u16::from(b'A'); 78];
        input.extend("𐐷z".encode_utf16());
        let result = board_serial(&input, |_| false, |_| {});
        assert_eq!(result, input[..79]);
        assert_eq!(result.last(), Some(&0xd801));
    }
}
