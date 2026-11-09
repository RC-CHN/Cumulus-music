//! Test-only access-shape categories; never format token data.
use super::super::identity::diagnostics::{observe, status_error};
use super::*;
use windows_sys::Win32::Security::{PSID, TOKEN_STATISTICS, TokenStatistics};

pub(super) fn current_process() -> String {
    match current_thread_token() {
        Ok(None) => {}
        Ok(Some(_)) => return "thread_token_present".into(),
        Err(error) => return error_status("thread_token_query_failed", &error),
    }
    // SAFETY: the current process pseudo-handle is valid for token query.
    match process_token(unsafe { GetCurrentProcess() }) {
        Ok(token) => snapshot(token.as_raw_handle()),
        Err(error) => error_status("process_token_open_failed", &error),
    }
}

fn error_status(label: &str, error: &io::Error) -> String {
    format!(
        "{label} kind={:?} win32={:?}",
        error.kind(),
        error.raw_os_error()
    )
}

fn snapshot(token: HANDLE) -> String {
    let mut fields = vec![
        (
            "TokenAppContainerSid",
            observe(token, TokenAppContainerSid, |b| {
                // SAFETY: the matching class returns this header; only test pointer nullness.
                match unsafe { read::<TOKEN_APPCONTAINER_INFORMATION>(b, 0) } {
                    Ok(value) if value.TokenAppContainer.is_null() => "no_app_container_sid",
                    Ok(_) => "app_container_sid_present",
                    Err(_) => "short_header",
                }
            }),
        ),
        (
            "TokenRestrictedSids",
            observe(token, TokenRestrictedSids, group_shape),
        ),
        (
            "TokenUserClaimAttributes",
            observe(token, TokenUserClaimAttributes, claim_shape),
        ),
        (
            "TokenDeviceClaimAttributes",
            observe(token, TokenDeviceClaimAttributes, claim_shape),
        ),
        (
            "TokenStatistics",
            observe(token, TokenStatistics, |b| {
                if b.len >= size_of::<TOKEN_STATISTICS>() {
                    "header_ok"
                } else {
                    "short_header"
                }
            }),
        ),
        (
            "TokenOwner",
            observe(token, TokenOwner, |b| {
                // SAFETY: this class returns TOKEN_OWNER; TokenBuffer validates the SID.
                let sid = unsafe { read::<TOKEN_OWNER>(b, 0) }.map(|header| header.Owner);
                sid_shape(b, sid)
            }),
        ),
        (
            "TokenPrimaryGroup",
            observe(token, TokenPrimaryGroup, |b| {
                // SAFETY: this class returns TOKEN_PRIMARY_GROUP; TokenBuffer validates the SID.
                let sid =
                    unsafe { read::<TOKEN_PRIMARY_GROUP>(b, 0) }.map(|header| header.PrimaryGroup);
                sid_shape(b, sid)
            }),
        ),
        ("TokenGroups", observe(token, TokenGroups, group_shape)),
        (
            "TokenDeviceGroups",
            observe(token, TokenDeviceGroups, group_shape),
        ),
        (
            "TokenPrivileges",
            observe(token, TokenPrivileges, |b| {
                if checked_entries(
                    b,
                    offset_of!(TOKEN_PRIVILEGES, Privileges),
                    size_of::<LUID_AND_ATTRIBUTES>(),
                )
                .is_ok()
                {
                    "entries_bounded"
                } else {
                    "malformed_entries"
                }
            }),
        ),
        (
            "Identity",
            if Identity::for_token(token).is_ok() {
                "ok"
            } else {
                "unavailable"
            }
            .into(),
        ),
    ];
    for (name, class) in [
        ("TokenIsAppContainer", TokenIsAppContainer),
        ("TokenHasRestrictions", TokenHasRestrictions),
        ("TokenSessionId", TokenSessionId),
        ("TokenUIAccess", TokenUIAccess),
        ("TokenVirtualizationAllowed", TokenVirtualizationAllowed),
        ("TokenVirtualizationEnabled", TokenVirtualizationEnabled),
    ] {
        let mut value = 0u32;
        // SAFETY: only the explicit DWORD classes above are observed here.
        let (success, actual) = unsafe { query_fixed(token, class, &mut value) };
        let error = status_error(success);
        let exact = actual as usize == size_of::<u32>();
        let shape = if success && exact && class == TokenIsAppContainer && value != 0 {
            "app_container_indicated"
        } else {
            "value_not_reported"
        };
        fields.push((name, fixed_status(success, error, exact, shape)));
    }
    let mut value = TOKEN_MANDATORY_POLICY::default();
    // SAFETY: the fixed class uses this exact Win32 type.
    let (success, actual) = unsafe { query_fixed(token, TokenMandatoryPolicy, &mut value) };
    fields.push((
        "TokenMandatoryPolicy",
        fixed_status(
            success,
            status_error(success),
            actual as usize == size_of::<TOKEN_MANDATORY_POLICY>(),
            "value_not_reported",
        ),
    ));
    fields.push(("TokenHasRestrictionsWrite", restriction_write_shape(token)));
    fields
        .into_iter()
        .map(|(name, status)| format!("{name}={status}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn fixed_status(success: bool, error: Option<u32>, exact: bool, shape: &str) -> String {
    format!("fixed(success={success},win32={error:?},exact_header={exact},shape={shape})")
}

fn restriction_write_shape(token: HANDLE) -> String {
    const FIRST: u32 = 0xa5c3_6996;
    const LENGTH: u32 = 0x7e19_c4d3;

    // Give the length out-parameter its own complementary markers: leaving
    // it untouched is distinguishable from explicitly reporting length zero.
    let sample = |before: u32, length_marker: u32| {
        let mut word = before;
        let mut returned = length_marker;
        // SAFETY: live token and two initialized, writable DWORDs. The
        // documented output size is exactly the size passed to Win32.
        let success = unsafe {
            GetTokenInformation(
                token,
                TokenHasRestrictions,
                ptr::from_mut(&mut word).cast(),
                size_of::<u32>() as u32,
                &mut returned,
            )
        } != 0;
        let error = status_error(success);
        let length = (returned != length_marker).then_some(returned);
        (word, success, error, length)
    };
    let (left, first_ok, first_error, first_length) = sample(FIRST, LENGTH);
    let (right, second_ok, second_error, second_length) = sample(!FIRST, !LENGTH);
    let (changed, each_full) = complementary_changed_bytes(FIRST, left, right);
    let same_output = (first_ok && second_ok).then_some(left == right);

    // A combined position mask avoids exposing either sample, or which
    // particular byte coincided with one sentinel's original value.
    format!(
        "first(success={first_ok},win32={first_error:?},returned_bytes={first_length:?}); second(success={second_ok},win32={second_error:?},returned_bytes={second_length:?}); changed_byte_mask=0x{changed:x},each_full={each_full},same_output={same_output:?}"
    )
}

fn complementary_changed_bytes(first: u32, left: u32, right: u32) -> (u8, bool) {
    let left_changed = (first ^ left).to_ne_bytes();
    let right_changed = (!first ^ right).to_ne_bytes();
    let union = left_changed
        .into_iter()
        .zip(right_changed)
        .enumerate()
        .fold(0, |mask, (offset, (a, b))| {
            if a | b != 0 {
                mask | (1 << offset)
            } else {
                mask
            }
        });
    let each_full = left_changed
        .into_iter()
        .chain(right_changed)
        .all(|byte| byte != 0);
    (union, each_full)
}

#[test]
fn complementary_markers_distinguish_full_partial_and_unchanged_words() {
    let first = u32::from_ne_bytes([0x12, 0x34, 0x56, 0x78]);
    assert_eq!(
        complementary_changed_bytes(first, first, first),
        (0b1111, false)
    );
    assert_eq!(complementary_changed_bytes(first, 0, 0), (0b1111, true));
    let mut partial = (!first).to_ne_bytes();
    partial[2] = first.to_ne_bytes()[2];
    assert_eq!(
        complementary_changed_bytes(first, first, u32::from_ne_bytes(partial)),
        (0b0100, false)
    );
    assert_eq!(
        complementary_changed_bytes(first, first, !first),
        (0, false)
    );
}

fn sid_shape(buffer: &TokenBuffer, sid: io::Result<PSID>) -> &'static str {
    match sid {
        Err(_) => "short_header",
        Ok(sid) if buffer.sid(sid).is_ok() => "sid_valid",
        Ok(_) => "sid_invalid",
    }
}

fn group_shape(buffer: &TokenBuffer) -> &'static str {
    let offset = offset_of!(TOKEN_GROUPS, Groups);
    let size = size_of::<SID_AND_ATTRIBUTES>();
    let Ok(count) = checked_entries(buffer, offset, size) else {
        return "malformed_entries";
    };
    for i in 0..count {
        // SAFETY: checked_entries has bounded every full record.
        let Ok(group) = (unsafe { read::<SID_AND_ATTRIBUTES>(buffer, offset + i * size) }) else {
            return "malformed_entries";
        };
        if buffer.sid(group.Sid).is_err() {
            return "sid_invalid";
        }
    }
    if count == 0 {
        "empty"
    } else {
        "groups_present"
    }
}

fn claim_shape(buffer: &TokenBuffer) -> &'static str {
    // SAFETY: the only callers request CLAIM_SECURITY_ATTRIBUTES_INFORMATION.
    let fields = unsafe { (read::<u16>(buffer, 0), read::<u32>(buffer, 4)) };
    match fields {
        (Ok(1), Ok(0)) => "empty_supported_claims",
        (Ok(1), Ok(_)) => "claims_present",
        (Ok(_), Ok(_)) => "unsupported_claim_version",
        _ => "short_claim_header",
    }
}

// Called only after the worker proves that its original context was restored.
pub(super) fn connector_mismatch(expected: &AccessShape, observed: &AccessShape) {
    let mut fields = vec![
        ("Identity", expected.identity == observed.identity),
        (
            "AuthenticationId",
            expected.logon_luid == observed.logon_luid,
        ),
        ("TokenOwner", expected.owner == observed.owner),
        (
            "TokenPrimaryGroup",
            expected.primary_group == observed.primary_group,
        ),
        ("TokenGroups", expected.groups == observed.groups),
        (
            "TokenDeviceGroups",
            expected.device_groups == observed.device_groups,
        ),
        (
            "TokenPrivileges",
            expected.privileges == observed.privileges,
        ),
    ];
    fields.extend(
        [
            "TokenHasRestrictions",
            "TokenMandatoryPolicy",
            "TokenSessionId",
            "TokenUIAccess",
            "TokenVirtualizationAllowed",
            "TokenVirtualizationEnabled",
        ]
        .into_iter()
        .zip(
            expected
                .policy
                .iter()
                .zip(observed.policy.iter())
                .map(|(a, b)| a == b),
        ),
    );
    let mismatch = fields
        .into_iter()
        .filter_map(|(field, equal)| (!equal).then_some(field))
        .collect::<Vec<_>>();
    eprintln!(
        "effective connector access shape mismatch: {}",
        mismatch.join(",")
    );
}
