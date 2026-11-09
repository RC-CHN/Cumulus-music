//! Attest the process token and the security context actually attached to the pipe.
use super::{
    identity::{Identity, TokenBuffer, process_token, token_statistics},
    stable::Server,
};
use std::{
    io,
    mem::{offset_of, size_of},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_TOKEN, HANDLE},
    Security::{
        GetTokenInformation, LUID_AND_ATTRIBUTES, RevertToSelf, SID_AND_ATTRIBUTES,
        SecurityIdentification, TOKEN_APPCONTAINER_INFORMATION, TOKEN_GROUPS,
        TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_POLICY, TOKEN_OWNER, TOKEN_PRIMARY_GROUP,
        TOKEN_PRIVILEGES, TOKEN_QUERY, TokenAppContainerSid, TokenDeviceClaimAttributes,
        TokenDeviceGroups, TokenGroups, TokenHasRestrictions, TokenIsAppContainer,
        TokenMandatoryPolicy, TokenOwner, TokenPrimaryGroup, TokenPrivileges, TokenRestrictedSids,
        TokenSessionId, TokenUIAccess, TokenUserClaimAttributes, TokenVirtualizationAllowed,
        TokenVirtualizationEnabled,
    },
    System::{
        Pipes::ImpersonateNamedPipeClient,
        Threading::{GetCurrentProcess, GetCurrentThread, OpenThreadToken},
    },
};

pub(super) const HELLO: u8 = 8;

#[derive(PartialEq, Eq)]
pub(super) struct AccessShape {
    identity: Identity,
    logon_luid: (i32, u32),
    owner: Vec<u8>,
    primary_group: Vec<u8>,
    groups: Vec<(Vec<u8>, u32)>,
    device_groups: Vec<(Vec<u8>, u32)>,
    privileges: Vec<(i32, u32, u32)>,
    policy: [u32; 6],
}

impl AccessShape {
    pub(super) fn matches_identity(&self, identity: &Identity) -> bool {
        self.identity == *identity
    }

    pub(super) fn current() -> io::Result<Self> {
        if current_thread_token()?.is_some() {
            return Err(denied());
        }
        // SAFETY: the current process pseudo-handle is valid without ownership.
        let token = process_token(unsafe { GetCurrentProcess() })?;
        Self::read(token.as_raw_handle())
    }

    fn read(token: HANDLE) -> io::Result<Self> {
        // The ordinary desktop lane does not accept AppContainer, an explicit
        // restricting SID list, or dynamic access-control claims. Unknown or
        // unreadable token information disables the lane.
        let container = TokenBuffer::query(token, TokenAppContainerSid)?;
        // SAFETY: this class returns one fixed pointer header. Check the
        // pointer itself; a scalar zero alone on an identification token is
        // not evidence that an AppContainer SID is absent.
        let container_sid = unsafe { read::<TOKEN_APPCONTAINER_INFORMATION>(&container, 0)? };
        if !container_sid.TokenAppContainer.is_null()
            || scalar(token, TokenIsAppContainer)? != 0
            || !groups(token, TokenRestrictedSids)?.is_empty()
            || !claims_empty(token, TokenUserClaimAttributes)?
            || !claims_empty(token, TokenDeviceClaimAttributes)?
        {
            return Err(denied());
        }
        let stats = token_statistics(token)?;
        let owner = TokenBuffer::query(token, TokenOwner)?;
        let primary = TokenBuffer::query(token, TokenPrimaryGroup)?;
        // SAFETY: the token APIs returned these fixed Win32 POD headers.
        let owner_sid = unsafe { read::<TOKEN_OWNER>(&owner, 0)? }.Owner;
        let primary_sid = unsafe { read::<TOKEN_PRIMARY_GROUP>(&primary, 0)? }.PrimaryGroup;
        let mut policy = [0; 6];
        for (value, class) in policy.iter_mut().zip([
            TokenHasRestrictions,
            TokenMandatoryPolicy,
            TokenSessionId,
            TokenUIAccess,
            TokenVirtualizationAllowed,
            TokenVirtualizationEnabled,
        ]) {
            *value = scalar(token, class)?;
        }
        Ok(Self {
            identity: Identity::for_token(token)?,
            logon_luid: (
                stats.AuthenticationId.HighPart,
                stats.AuthenticationId.LowPart,
            ),
            owner: owner.sid(owner_sid)?,
            primary_group: primary.sid(primary_sid)?,
            groups: groups(token, TokenGroups)?,
            device_groups: groups(token, TokenDeviceGroups)?,
            privileges: privileges(token)?,
            policy,
        })
    }

    pub(super) fn verify_process(&self, pipe: HANDLE, client_is_peer: bool) -> io::Result<()> {
        let token = self.identity.verified_peer_token(pipe, client_is_peer)?;
        if Self::read(token.as_raw_handle())? == *self {
            Ok(())
        } else {
            Err(denied())
        }
    }

    pub(super) fn authenticate(
        &self,
        pipe: &mut Server,
        deadline: Instant,
        failure: impl FnOnce(&'static str, Option<i32>),
    ) -> Connector {
        if let Err(error) = self.verify_process(pipe.as_raw_handle(), true) {
            failure("owner.sender_process_denied", error.raw_os_error());
            return Connector::Denied;
        }
        let mut hello = [0];
        if let Err(error) = pipe.read_exact(&mut hello, deadline) {
            failure("owner.hello_read_failed", error.raw_os_error());
            return Connector::Denied;
        }
        if hello != [HELLO] {
            failure("owner.hello_invalid", None);
            return Connector::Denied;
        }
        let connector = self.verify_connector(pipe.as_raw_handle());
        if connector == Connector::Denied {
            failure("owner.sender_effective_token_denied", None);
        }
        // RetireWorker deliberately produces no diagnostic on this thread.
        connector
    }

    /// Must run synchronously on the dedicated relay thread, after a bounded
    /// pipe read and before dispatch. No file, app, or async API runs here.
    pub(super) fn verify_connector(&self, pipe: HANDLE) -> Connector {
        if !matches!(current_thread_token(), Ok(None)) {
            return Connector::RetireWorker;
        }
        // SAFETY: live server handle with a preceding read on this connection.
        if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
            return Connector::Denied;
        }
        let revert = Revert { active: true };
        let result = current_thread_token()
            .and_then(|token| token.ok_or_else(denied))
            .and_then(|token| {
                let raw = token.as_raw_handle();
                if token_statistics(raw)?.ImpersonationLevel != SecurityIdentification {
                    return Err(denied());
                }
                Self::read(raw)
            });
        if !revert.finish() || !matches!(current_thread_token(), Ok(None)) {
            // Do not send replies, dispatch, log, or serve another connection
            // on a thread whose original context cannot be established.
            return Connector::RetireWorker;
        }
        if result.is_ok_and(|shape| shape == *self) {
            Connector::Verified
        } else {
            Connector::Denied
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Connector {
    Verified,
    Denied,
    RetireWorker,
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "unsupported local peer token",
    )
}

fn current_thread_token() -> io::Result<Option<OwnedHandle>> {
    let mut raw = ptr::null_mut();
    // SAFETY: writable output and live pseudo-handle; OpenAsSelf is required
    // to inspect an identification-level token without acting as that client.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut raw) } == 0 {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_NO_TOKEN as i32) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if raw.is_null() {
        return Err(denied());
    }
    // SAFETY: a successful query transferred ownership of a real handle.
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(raw) }))
}

struct Revert {
    active: bool,
}
impl Revert {
    fn finish(mut self) -> bool {
        // SAFETY: this thread began a successful pipe impersonation above.
        if unsafe { RevertToSelf() } == 0 {
            return false;
        }
        self.active = false;
        true
    }
}
impl Drop for Revert {
    fn drop(&mut self) {
        if self.active {
            // SAFETY: best effort on error/unwind; caller still retires the worker.
            let _ = unsafe { RevertToSelf() };
        }
    }
}

// Only call with plain Win32 integer/POD record types from the corresponding
// token information class. A pointer field is never dereferenced directly.
unsafe fn read<T: Copy>(buffer: &TokenBuffer, offset: usize) -> io::Result<T> {
    if offset
        .checked_add(size_of::<T>())
        .is_none_or(|end| end > buffer.len)
    {
        return Err(denied());
    }
    // SAFETY: the POD record lies in the bounded initialized token response.
    Ok(unsafe {
        buffer
            .words
            .as_ptr()
            .cast::<u8>()
            .add(offset)
            .cast::<T>()
            .read_unaligned()
    })
}

// `value` must match the verified fixed-size Win32 POD output for `class`.
unsafe fn query_fixed<T>(
    token: HANDLE,
    class: TOKEN_INFORMATION_CLASS,
    value: &mut T,
) -> (bool, u32) {
    let mut actual = 0;
    // SAFETY: caller selects the exact POD type and this call writes at most its size.
    let success = unsafe {
        GetTokenInformation(
            token,
            class,
            ptr::from_mut(value).cast(),
            size_of::<T>() as u32,
            &mut actual,
        )
    } != 0;
    (success, actual)
}

fn scalar(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<u32> {
    if class == TokenHasRestrictions {
        let (success, actual, value) = query_restriction_boolean(token);
        return restriction_boolean(success, actual, value);
    }
    if class == TokenMandatoryPolicy {
        let mut value = TOKEN_MANDATORY_POLICY::default();
        // SAFETY: this fixed class returns exactly TOKEN_MANDATORY_POLICY.
        let (success, actual) = unsafe { query_fixed(token, class, &mut value) };
        return if success && actual as usize == size_of::<TOKEN_MANDATORY_POLICY>() {
            Ok(value.Policy)
        } else {
            Err(denied())
        };
    }
    if ![
        TokenIsAppContainer,
        TokenSessionId,
        TokenUIAccess,
        TokenVirtualizationAllowed,
        TokenVirtualizationEnabled,
    ]
    .contains(&class)
    {
        return Err(denied());
    }
    // A zero-length sizing probe is not required for these fixed DWORD outputs.
    let mut value = 0u32;
    // SAFETY: every class admitted above documents a DWORD output.
    let (success, actual) = unsafe { query_fixed(token, class, &mut value) };
    if success && actual as usize == size_of::<u32>() {
        Ok(value)
    } else {
        Err(denied())
    }
}

fn query_restriction_boolean(token: HANDLE) -> (bool, u32, u8) {
    // Hosted Windows x64 rejected the one-byte buffer but accepted a DWORD
    // buffer, reported length one, and wrote only its first byte. Preserve the
    // accepted alignment/capacity; the sentinel rejects an unwritten result.
    let mut buffer = u32::MAX;
    // SAFETY: the DWORD is writable and the API receives its full capacity.
    let (success, actual) = unsafe { query_fixed(token, TokenHasRestrictions, &mut buffer) };
    (success, actual, buffer.to_ne_bytes()[0])
}

fn restriction_boolean(success: bool, actual: u32, value: u8) -> io::Result<u32> {
    if success && actual as usize == size_of::<u8>() && matches!(value, 0 | 1) {
        Ok(u32::from(value))
    } else {
        Err(denied())
    }
}

fn claims_empty(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<bool> {
    let buffer = TokenBuffer::query(token, class)?;
    // SAFETY: CLAIM_SECURITY_ATTRIBUTES_INFORMATION starts with Version,
    // Reserved (both WORD) and AttributeCount (DWORD).
    let version: u16 = unsafe { read(&buffer, 0)? };
    let count: u32 = unsafe { read(&buffer, 4)? };
    Ok(version == 1 && count == 0)
}

fn checked_entries(buffer: &TokenBuffer, offset: usize, record_size: usize) -> io::Result<usize> {
    // SAFETY: TOKEN_GROUPS and TOKEN_PRIVILEGES both start with a DWORD count.
    let count = unsafe { read::<u32>(buffer, 0)? } as usize;
    if count == 0 {
        return Ok(0);
    }
    if count > buffer.len.saturating_sub(offset) / record_size {
        return Err(denied());
    }
    Ok(count)
}

fn groups(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<(Vec<u8>, u32)>> {
    let buffer = TokenBuffer::query(token, class)?;
    let offset = offset_of!(TOKEN_GROUPS, Groups);
    let size = size_of::<SID_AND_ATTRIBUTES>();
    let count = checked_entries(&buffer, offset, size)?;
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: this class returned TOKEN_GROUPS and the full entry fits.
        let group = unsafe { read::<SID_AND_ATTRIBUTES>(&buffer, offset + i * size)? };
        result.push((buffer.sid(group.Sid)?, group.Attributes));
    }
    result.sort_unstable();
    Ok(result)
}

fn privileges(token: HANDLE) -> io::Result<Vec<(i32, u32, u32)>> {
    let buffer = TokenBuffer::query(token, TokenPrivileges)?;
    let offset = offset_of!(TOKEN_PRIVILEGES, Privileges);
    let size = size_of::<LUID_AND_ATTRIBUTES>();
    let count = checked_entries(&buffer, offset, size)?;
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: TokenPrivileges returned TOKEN_PRIVILEGES with a bounded count.
        let p = unsafe { read::<LUID_AND_ATTRIBUTES>(&buffer, offset + i * size)? };
        result.push((p.Luid.HighPart, p.Luid.LowPart, p.Attributes));
    }
    result.sort_unstable();
    Ok(result)
}

#[cfg(test)]
fn restricted_same_logon_for_test() -> OwnedHandle {
    use windows_sys::Win32::{
        Security::{CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, TOKEN_DUPLICATE, TokenLogonSid},
        System::Threading::OpenProcessToken,
    };
    let mut raw = ptr::null_mut();
    // SAFETY: pseudo-handle and output valid; these rights only read and
    // derive a less privileged token from the test process.
    assert_ne!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut raw) },
        0
    );
    assert!(!raw.is_null());
    // SAFETY: successful call returned a uniquely owned handle.
    let base = unsafe { OwnedHandle::from_raw_handle(raw) };
    let groups = groups(base.as_raw_handle(), TokenLogonSid).unwrap();
    assert_eq!(groups.len(), 1);
    let sid_bytes = &groups[0].0;
    let mut sid = vec![0u32; sid_bytes.len().div_ceil(size_of::<u32>())];
    // SAFETY: the destination allocation covers all SID bytes with DWORD alignment.
    unsafe {
        ptr::copy_nonoverlapping(sid_bytes.as_ptr(), sid.as_mut_ptr().cast(), sid_bytes.len())
    };
    let restricting = SID_AND_ATTRIBUTES {
        Sid: sid.as_mut_ptr().cast(),
        Attributes: 0,
    };
    let mut result = ptr::null_mut();
    // SAFETY: one live, previously validated SID; optional arrays are empty.
    assert_ne!(
        unsafe {
            CreateRestrictedToken(
                base.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE,
                0,
                ptr::null(),
                0,
                ptr::null(),
                1,
                &restricting,
                &mut result,
            )
        },
        0
    );
    assert!(!result.is_null());
    // SAFETY: CreateRestrictedToken returned ownership of a new handle.
    unsafe { OwnedHandle::from_raw_handle(result) }
}

#[cfg(test)]
fn with_test_impersonation<T>(token: &OwnedHandle, action: impl FnOnce() -> T) -> T {
    use windows_sys::Win32::Security::ImpersonateLoggedOnUser;
    assert!(matches!(current_thread_token(), Ok(None)));
    // SAFETY: valid test-only restricted token; the scope restores it.
    assert_ne!(unsafe { ImpersonateLoggedOnUser(token.as_raw_handle()) }, 0);
    let guard = Revert { active: true };
    let value = action();
    assert!(guard.finish() && matches!(current_thread_token(), Ok(None)));
    value
}

#[cfg(test)]
mod tests {
    use super::super::{
        security::{open_handle, wide, with_logon_security},
        stable::Stop,
    };
    use super::*;
    use std::{os::windows::io::IntoRawHandle, sync::mpsc, thread, time::Duration};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::windows::named_pipe::NamedPipeClient,
        time::timeout,
    };

    const LIMIT: Duration = Duration::from_secs(5);

    #[test]
    fn restriction_boolean_rejects_unwritten_invalid_and_wrong_size_results() {
        assert_eq!(restriction_boolean(true, 1, 0).unwrap(), 0);
        assert_eq!(restriction_boolean(true, 1, 1).unwrap(), 1);
        for (success, actual, value) in [
            (false, 1, 1),
            (true, 0, 0),
            (true, 4, 1),
            (true, 1, 2),
            (true, 1, u8::MAX),
        ] {
            assert_eq!(
                restriction_boolean(success, actual, value)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::PermissionDenied
            );
        }
    }

    #[test]
    fn effective_connector_rejects_same_logon_restricted_thread_before_ready() {
        let scratch = tempfile::tempdir().unwrap();
        let identity = Identity::current().unwrap();
        let address = identity.address(scratch.path()).unwrap();
        let address_wide = wide(&address);
        let stop = Stop::new().unwrap();
        // SAFETY: derived private NUL-terminated name and live DACL.
        let mut server = with_logon_security(&identity, |attrs| unsafe {
            Server::create(&address_wide, attrs, stop)
        })
        .unwrap();
        let (reset, disconnected) = mpsc::channel();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        let restricted = restricted_same_logon_for_test();
        let owner = AccessShape::current().unwrap();
        assert!(owner.matches_identity(&identity));

        thread::scope(|scope| {
            let owner = &owner;
            scope.spawn(move || {
                server.connect(Instant::now() + LIMIT).unwrap();
                assert_eq!(
                    owner.authenticate(&mut server, Instant::now() + LIMIT, |_, _| {
                        panic!("trusted connector emitted a failure stage");
                    }),
                    Connector::Verified
                );
                server.write_all(&[1], Instant::now() + LIMIT).unwrap();
                let mut synthetic = [0];
                server
                    .read_exact(&mut synthetic, Instant::now() + LIMIT)
                    .unwrap();
                assert_eq!(synthetic, [2]);
                assert_eq!(
                    owner.verify_connector(server.as_raw_handle()),
                    Connector::Verified
                );
                server.write_all(&[3], Instant::now() + LIMIT).unwrap();
                server
                    .read_exact(&mut synthetic, Instant::now() + LIMIT)
                    .unwrap();
                assert_eq!(synthetic, [4]);
                server.disconnect().unwrap();
                reset.send(()).unwrap();

                server.connect(Instant::now() + LIMIT).unwrap();
                // The pipe ACL and the unrestricted primary process both
                // pass. Only the actual connecting thread is restricted.
                owner.verify_process(server.as_raw_handle(), true).unwrap();
                let mut failure = None;
                assert_eq!(
                    owner.authenticate(&mut server, Instant::now() + LIMIT, |stage, code| {
                        failure = Some((stage, code));
                    }),
                    Connector::Denied
                );
                assert_eq!(failure, Some(("owner.sender_effective_token_denied", None)));
                server.disconnect().unwrap();
            });

            runtime.block_on(async {
                let raw = open_handle(&address).unwrap().into_raw_handle();
                // SAFETY: unique overlapped handle transferred to Tokio.
                let mut client = unsafe { NamedPipeClient::from_raw_handle(raw) }.unwrap();
                owner.verify_process(client.as_raw_handle(), false).unwrap();
                timeout(LIMIT, client.write_all(&[HELLO]))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(timeout(LIMIT, client.read_u8()).await.unwrap().unwrap(), 1);
                timeout(LIMIT, client.write_all(&[2]))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(timeout(LIMIT, client.read_u8()).await.unwrap().unwrap(), 3);
                timeout(LIMIT, client.write_all(&[4]))
                    .await
                    .unwrap()
                    .unwrap();
            });
            disconnected.recv_timeout(LIMIT).unwrap();
            with_test_impersonation(&restricted, || {
                let raw = open_handle(&address)
                    .expect("matching logon ACL allows this transport open")
                    .into_raw_handle();
                runtime.block_on(async {
                    // SAFETY: unique overlapped handle transferred to Tokio.
                    let mut client = unsafe { NamedPipeClient::from_raw_handle(raw) }.unwrap();
                    timeout(LIMIT, client.write_all(&[HELLO]))
                        .await
                        .unwrap()
                        .unwrap();
                    // Require the server to close without a ready byte.
                    assert!(timeout(LIMIT, client.read_u8()).await.unwrap().is_err());
                });
            });
        });
    }
}
