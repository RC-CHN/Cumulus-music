//! Failure-only Windows fixture observations. Never format token data.
use super::*;
use windows_sys::Win32::Foundation::GetLastError;

pub(crate) fn current_process() -> String {
    // SAFETY: current process pseudo-handle, never closed by this call.
    let token = match process_token(unsafe { GetCurrentProcess() }) {
        Ok(token) => token,
        Err(error) => {
            return format!(
                "process_token_open=failed kind={:?} win32={:?}",
                error.kind(),
                error.raw_os_error()
            );
        }
    };
    let raw = token.as_raw_handle();
    format!(
        "process_token_open=ok; TokenUser={}; TokenLogonSid={}; TokenGroups={}; TokenIntegrityLevel={}; TokenElevation={}; TokenElevationDirect={}",
        observe(raw, TokenUser, |buffer| {
            sid_status(
                buffer,
                header::<TOKEN_USER>(buffer).map(|user| user.User.Sid),
            )
        }),
        observe(raw, TokenLogonSid, |buffer| logon_status(buffer, true)),
        observe(raw, TokenGroups, |buffer| logon_status(buffer, false)),
        observe(raw, TokenIntegrityLevel, |buffer| {
            sid_status(
                buffer,
                header::<TOKEN_MANDATORY_LABEL>(buffer).map(|label| label.Label.Sid),
            )
        }),
        observe(raw, TokenElevation, |buffer| {
            if header::<TOKEN_ELEVATION>(buffer).is_some() {
                "header_ok"
            } else {
                "short_header"
            }
        }),
        elevation_direct(raw),
    )
}

pub(crate) fn observe(
    token: HANDLE,
    class: TOKEN_INFORMATION_CLASS,
    inspect: impl FnOnce(&TokenBuffer) -> &'static str,
) -> String {
    let mut required = 0;
    // SAFETY: documented zero-length sizing query with writable output.
    let sizing_ok =
        unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut required) } != 0;
    let sizing_error = status_error(sizing_ok);
    let bounded = (1..=MAX_TOKEN_BYTES).contains(&(required as usize));
    if sizing_ok || sizing_error != Some(ERROR_INSUFFICIENT_BUFFER) || !bounded {
        return format!(
            "sizing(success={sizing_ok},win32={sizing_error:?},bounded={bounded}); read=not_attempted"
        );
    }

    let mut words = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
    let mut actual = 0;
    // SAFETY: initialized aligned storage is bounded by the successful sizing
    // request. The handle remains owned by the outer process probe.
    let read_ok = unsafe {
        GetTokenInformation(
            token,
            class,
            words.as_mut_ptr().cast(),
            required,
            &mut actual,
        )
    } != 0;
    let read_error = status_error(read_ok);
    let length_ok = (1..=required).contains(&actual);
    let shape = if read_ok && length_ok {
        inspect(&TokenBuffer {
            words,
            len: actual as usize,
        })
    } else {
        "not_inspected"
    };
    format!(
        "sizing(expected_insufficient_buffer=true); read(success={read_ok},win32={read_error:?},bounded={length_ok},shape={shape})"
    )
}

pub(crate) fn status_error(success: bool) -> Option<u32> {
    // SAFETY: no intervening Windows call; use this only immediately after a
    // failing Win32 API so a stale success-side error is not printed.
    (!success).then(|| unsafe { GetLastError() })
}

fn elevation_direct(token: HANDLE) -> String {
    let mut elevation = TOKEN_ELEVATION::default();
    let expected = size_of::<TOKEN_ELEVATION>() as u32;
    let mut returned = 0;
    // SAFETY: exactly one correctly typed writable fixed-size record and an
    // owned token. The returned elevation flag is deliberately not inspected.
    let success = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            ptr::from_mut(&mut elevation).cast(),
            expected,
            &mut returned,
        )
    } != 0;
    let error = status_error(success);
    format!(
        "success={success},win32={error:?},returned_bytes={returned},exact_header={}",
        returned == expected
    )
}

// Only Win32 POD headers from the matching class are used here.
fn header<T: Copy>(buffer: &TokenBuffer) -> Option<T> {
    (buffer.len >= size_of::<T>()).then(|| {
        // SAFETY: the aligned, initialized Win32 response contains the POD
        // header. Pointer fields are validated separately by TokenBuffer::sid.
        unsafe { buffer.words.as_ptr().cast::<T>().read_unaligned() }
    })
}

fn sid_status(buffer: &TokenBuffer, sid: Option<PSID>) -> &'static str {
    match sid {
        None => "short_header",
        Some(sid) if buffer.sid(sid).is_ok() => "sid_valid",
        Some(_) => "sid_invalid",
    }
}

fn logon_status(buffer: &TokenBuffer, single: bool) -> &'static str {
    let Some(count) = header::<u32>(buffer) else {
        return "short_count_header";
    };
    if count == 0 {
        return "zero_groups";
    }
    let count = count as usize;
    let start = offset_of!(TOKEN_GROUPS, Groups);
    if count > buffer.len.saturating_sub(start) / size_of::<SID_AND_ATTRIBUTES>() {
        return "short_group_records";
    }
    if single && count != 1 {
        return "multiple_groups";
    }
    // SAFETY: offset and initialized count fit within the bounded Win32 buffer.
    let records = unsafe {
        std::slice::from_raw_parts(
            buffer
                .words
                .as_ptr()
                .cast::<u8>()
                .add(start)
                .cast::<SID_AND_ATTRIBUTES>(),
            count,
        )
    };
    let logon = SE_GROUP_LOGON_ID as u32;
    let mut marked = records
        .iter()
        .filter(|entry| entry.Attributes & logon == logon);
    let Some(entry) = marked.next() else {
        return "no_marked_logon_group";
    };
    if marked.next().is_some() {
        return "multiple_marked_logon_groups";
    }
    if buffer.sid(entry.Sid).is_err() {
        return "marked_logon_sid_invalid";
    }
    let access = (SE_GROUP_ENABLED | SE_GROUP_USE_FOR_DENY_ONLY) as u32;
    if entry.Attributes & access == SE_GROUP_ENABLED as u32 {
        "valid_enabled_logon_group"
    } else {
        "valid_logon_group_not_enabled"
    }
}
