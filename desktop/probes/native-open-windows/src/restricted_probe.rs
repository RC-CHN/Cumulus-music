//! Restricted *thread* ACL experiment. The child retains its original logon
//! and never contacts the pipe under an impersonated token after denial.

use super::{
    HANDSHAKE, READY, at, demand, expect_open_error, fail, open, open_handle, read_code, receipt,
    spawn, verify_acl, wait, wide, with_logon_security,
};
use crate::{
    identity::{Identity, authentication_id, only_restricting_sid},
    stable::{Server, Stop},
};
use std::{
    io,
    marker::PhantomData,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    rc::Rc,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_NO_TOKEN},
    Security::{
        CreateRestrictedToken, CreateWellKnownSid, DISABLE_MAX_PRIVILEGE, ImpersonateLoggedOnUser,
        IsTokenRestricted, RevertToSelf, SECURITY_MAX_SID_SIZE, SID_AND_ATTRIBUTES,
        TOKEN_DUPLICATE, TOKEN_QUERY, WinRestrictedCodeSid,
    },
    System::{
        Pipes::PeekNamedPipe,
        Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
    },
};

const RESTRICTED_CONTROL_OK: u8 = 31;
const REVERT_FAILED: i32 = 32;

#[repr(align(8))]
struct Restriction([u8; SECURITY_MAX_SID_SIZE as usize]);

fn original_token() -> io::Result<OwnedHandle> {
    let mut raw = ptr::null_mut();
    // SAFETY: current-process pseudo-handle and writable output; no privilege
    // adjustment or rights to assign this token to another process are sought.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut raw) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    demand(!raw.is_null(), "missing original child token")?;
    // SAFETY: the API returned a fresh owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

fn thread_token() -> io::Result<Option<OwnedHandle>> {
    let mut raw = ptr::null_mut();
    // SAFETY: OpenAsSelf only affects the permission check for querying this
    // handle. It does not replace the token used by the later CreateFileW.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut raw) } == 0 {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_NO_TOKEN as i32) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    demand(!raw.is_null(), "missing effective child token")?;
    // SAFETY: the API returned a fresh owned handle.
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(raw) }))
}

fn restricted_token(
    original: &OwnedHandle,
    sid: &mut Restriction,
) -> io::Result<(OwnedHandle, usize)> {
    let mut length = SECURITY_MAX_SID_SIZE;
    let address = sid.0.as_mut_ptr().cast();
    // SAFETY: fixed aligned writable storage for a well-known local SID.
    if unsafe { CreateWellKnownSid(WinRestrictedCodeSid, ptr::null_mut(), address, &mut length) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    demand(
        (8..=SECURITY_MAX_SID_SIZE).contains(&length),
        "unexpected restricted SID size",
    )?;
    let rule = SID_AND_ATTRIBUTES {
        Sid: address,
        Attributes: 0,
    };
    let mut raw = ptr::null_mut();
    // SAFETY: the borrowed original token and fixed restricting SID live for
    // the call. No elevation, disabled SID mutation, credentials, or logon.
    if unsafe {
        CreateRestrictedToken(
            original.as_raw_handle(),
            DISABLE_MAX_PRIVILEGE,
            0,
            ptr::null(),
            0,
            ptr::null(),
            1,
            &rule,
            &mut raw,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    demand(!raw.is_null(), "missing newly restricted child token")?;
    // SAFETY: the returned token handle is newly owned.
    Ok((
        unsafe { OwnedHandle::from_raw_handle(raw) },
        length as usize,
    ))
}

struct ThreadImpersonation {
    active: bool,
    // Thread security context must never be restored from a different thread.
    _thread_only: PhantomData<Rc<()>>,
}

impl ThreadImpersonation {
    fn apply(token: &OwnedHandle) -> io::Result<Self> {
        // SAFETY: the restricted primary token is our own same-user token with
        // query/duplicate rights; no call is attempted if impersonation fails.
        if unsafe { ImpersonateLoggedOnUser(token.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            active: true,
            _thread_only: PhantomData,
        })
    }

    fn restore(&mut self) {
        if std::mem::take(&mut self.active) {
            // SAFETY: this guard is confined to its creating thread and no
            // asynchronous work can run while it is held.
            if unsafe { RevertToSelf() } == 0 {
                // A child whose context cannot be restored must do no more
                // work. Exit immediately; never continue with another token.
                std::process::exit(REVERT_FAILED);
            }
        }
    }
}

impl Drop for ThreadImpersonation {
    fn drop(&mut self) {
        self.restore();
    }
}

fn require_kernel_acl_denial(identity: &Identity, address: &str) -> io::Result<()> {
    demand(thread_token()?.is_none(), "child already impersonating")?;
    let original = original_token()?;
    let origin_session = authentication_id(original.as_raw_handle())?;
    let mut sid = Restriction([0; SECURITY_MAX_SID_SIZE as usize]);
    let (restricted, len) = restricted_token(&original, &mut sid)?;
    let mut guard = ThreadImpersonation::apply(&restricted)?;
    let result = (|| {
        let effective_token =
            thread_token()?.ok_or_else(|| fail("child has no effective token"))?;
        let effective = effective_token.as_raw_handle();
        // SAFETY: the queried thread token handle remains live during the call.
        demand(
            unsafe { IsTokenRestricted(effective) } != 0,
            "thread token is not restricted",
        )?;
        demand(
            Identity::for_token(effective)? == *identity
                && authentication_id(effective)? == origin_session,
            "restricted token changed user or logon",
        )?;
        demand(
            only_restricting_sid(effective, &sid.0[..len])?,
            "unexpected effective restricting SID list",
        )?;
        // Only the raw CreateFileW error can pass the proof. Do not call any
        // runtime registration or attempt a pipe read/write in this context.
        expect_open_error(
            "kernel did not deny the restricted pipe open",
            open_handle(address),
            &[ERROR_ACCESS_DENIED],
        )
    })();
    guard.restore();
    demand(thread_token()?.is_none(), "child thread token not restored")?;
    result
}

pub(super) async fn child(identity: &Identity, address: &str) -> io::Result<u8> {
    // Synchronous, before any .await or bytes on the pipe.
    require_kernel_acl_denial(identity, address)?;
    let mut pipe = open(address)?;
    identity.verify_peer(pipe.as_raw_handle(), false)?;
    demand(
        read_code(&mut pipe).await? == READY,
        "restricted control readiness missing",
    )?;
    // The allowed control sends zero client bytes and never opens a path.
    Ok(RESTRICTED_CONTROL_OK)
}

fn no_client_bytes(server: &Server) -> io::Result<()> {
    let mut available = 0;
    // SAFETY: the connected lab pipe is live; only the bounded count output is
    // requested, no bytes or identities are read from the kernel buffer.
    if unsafe {
        PeekNamedPipe(
            server.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &mut available,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    demand(
        available == 0,
        "a client frame arrived before restored readiness",
    )
}

pub(super) async fn run() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let identity = Identity::current()?;
    let address = wide(&identity.address(root.path())?);
    let stop = Stop::new()?;
    let mut server = with_logon_security(&identity, |security| {
        // SAFETY: the fixed local scratch address and descriptor remain live.
        unsafe { Server::create(&address, security, stop) }
    })?;
    verify_acl(&server, &identity)?;
    let child = at(
        "spawn restricted ACL child",
        spawn(root.path(), "restricted-acl"),
    )?;
    at(
        "accept only the restored-token control",
        server.connect(Instant::now() + HANDSHAKE),
    )?;
    at(
        "verify restored child primary identity",
        identity.verify_peer(server.as_raw_handle(), true),
    )?;
    at(
        "verify no client frame before readiness",
        no_client_bytes(&server),
    )?;
    at(
        "write readiness without accepting path bytes",
        server.write_all(&receipt(READY), Instant::now() + HANDSHAKE),
    )?;
    at(
        "confirm raw denial and restored child readiness",
        wait(child, RESTRICTED_CONTROL_OK).await,
    )?;
    at("disconnect restricted control", server.disconnect())?;
    println!(concat!(
        "{{\"ok\":true,\"nativeWindowsNamedPipe\":true,\"actualChildProcess\":true,",
        "\"logonOnlyDaclObserved\":true,\"effectiveThreadTokenRestricted\":true,",
        "\"sameUserAndLogonLuid\":true,\"kernelAccessDenied\":true,",
        "\"samePipeRestoredTokenControl\":true,\"sameUserAndLogonBothDirections\":true,",
        "\"noFrameQueuedBeforeReady\":true,\"clientBytesSent\":0,",
        "\"distinctLogonVerified\":false,",
        "\"productionEnabled\":false,\"realFilesOpened\":0,\"registryWrites\":0}}"
    ));
    Ok(())
}
