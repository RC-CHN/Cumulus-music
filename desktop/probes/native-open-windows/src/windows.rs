//! Windows API host for an isolated same-logon, same-directory transport.
#[path = "restricted_probe.rs"]
mod restricted_probe;
#[path = "stable_probe.rs"]
mod stable_probe;

use crate::{
    identity::{Identity, local_path},
    wire,
};
use std::{
    env,
    ffi::OsStr,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle},
    },
    path::Path,
    process::Stdio,
    ptr,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, PipeMode, ServerOptions},
    process::{Child, Command},
    time::{sleep, timeout},
};
use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, INVALID_HANDLE_VALUE, LocalFree},
    Security::{
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW,
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
            SE_KERNEL_OBJECT,
        },
        DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_WRITE_DATA,
        OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, SYNCHRONIZE,
    },
};

const HANDSHAKE: Duration = Duration::from_secs(5);
const CHILD: Duration = Duration::from_secs(10);
const READY: u8 = 6;
const ACCEPTED: u8 = 0;
const UNAVAILABLE: u8 = 2;
const INVALID: u8 = 3;
const UNKNOWN: u8 = 4;
const BUSY: u8 = 5;
const TRACKS: &[&str] = &[r"C:\合成\01 % 空格.mp3", r"C:\合成\02.flac"];

fn fail(detail: &'static str) -> io::Error {
    // Every detail is a fixed lab assertion, never error Display, a path,
    // a pipe name, a SID, or any value returned by a Win32 API.
    eprintln!("native open lab assertion: {detail}");
    io::Error::other(detail)
}

fn at<T>(stage: &'static str, result: io::Result<T>) -> io::Result<T> {
    result.inspect_err(|error| {
        eprintln!(
            "native open lab stage: {stage}: {:?} ({:?})",
            error.kind(),
            error.raw_os_error()
        );
    })
}

fn expect_open_error<T>(
    stage: &'static str,
    result: io::Result<T>,
    codes: &[u32],
) -> io::Result<()> {
    match result {
        Err(error)
            if codes
                .iter()
                .any(|&code| error.raw_os_error() == Some(code as i32)) =>
        {
            Ok(())
        }
        Err(error) => at(stage, Err(error)),
        Ok(_) => Err(fail(stage)),
    }
}
fn demand(ok: bool, detail: &'static str) -> io::Result<()> {
    if ok { Ok(()) } else { Err(fail(detail)) }
}
fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: unique pointer allocated by the SDDL conversion routine.
        unsafe { LocalFree(self.0) };
    }
}

fn listener(address: &str, identity: &Identity) -> io::Result<NamedPipeServer> {
    create_listener(address, identity, true, 1)
}

fn create_listener(
    address: &str,
    identity: &Identity,
    first: bool,
    limit: usize,
) -> io::Result<NamedPipeServer> {
    with_logon_security(identity, |security| {
        // SAFETY: security storage is live for the duration of this call.
        unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .max_instances(limit)
                .pipe_mode(PipeMode::Byte)
                .in_buffer_size((wire::MAX + 8) as u32)
                .out_buffer_size(64)
                .create_with_security_attributes_raw(
                    address,
                    (security as *mut SECURITY_ATTRIBUTES).cast(),
                )
        }
    })
}

fn with_logon_security<T>(
    identity: &Identity,
    create: impl FnOnce(&mut SECURITY_ATTRIBUTES) -> io::Result<T>,
) -> io::Result<T> {
    // Protected DACL with exactly the current Windows logon SID; default
    // named-pipe permissions include Everyone read and are not used.
    let text = wide(&format!("D:P(A;;GA;;;{})", identity.logon_sddl()));
    let mut raw = ptr::null_mut();
    // SAFETY: null-terminated SDDL and writable descriptor out-pointer.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            SDDL_REVISION_1,
            &mut raw,
            ptr::null_mut(),
        )
    } == 0
        || raw.is_null()
    {
        return Err(io::Error::last_os_error());
    }
    let descriptor = Descriptor(raw);
    create(&mut SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    })
}

async fn replace_used_listener(
    server: NamedPipeServer,
    address: &str,
    identity: &Identity,
) -> io::Result<NamedPipeServer> {
    // Mio prefetches a read after consuming the previous frame. Disconnect
    // resets the kernel connection, but an old EOF/completion may still be
    // cached in that Mio instance. Use a new overlapped I/O state for the next
    // child. The old handle can remain alive until IOCP cancellation settles.
    drop(server);
    timeout(HANDSHAKE, async {
        loop {
            match listener(address, identity) {
                Err(error)
                    if matches!(
                        error.raw_os_error().map(|code| code as u32),
                        Some(ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY)
                    ) =>
                {
                    // Only re-arm this lab-owned listener; no client exists
                    // yet, no request is resent, and the first-instance flag
                    // and DACL remain in force for every create attempt.
                    sleep(Duration::from_millis(10)).await;
                }
                result => return result,
            }
        }
    })
    .await?
}

fn verify_first_instance_flag(address: &str, identity: &Identity) -> io::Result<()> {
    // This isolated pipe has capacity for a second instance. The flag must
    // forbid it even when a full instance cap cannot explain the denial.
    let address = format!("{address}.first-instance-control");
    let first = at(
        "create first-instance control pipe",
        create_listener(&address, identity, true, 2),
    )?;
    expect_open_error(
        "reject duplicate first flag with capacity available",
        create_listener(&address, identity, true, 2),
        &[ERROR_ACCESS_DENIED],
    )?;
    let second = at(
        "open available control slot without first flag",
        create_listener(&address, identity, false, 2),
    )?;
    drop((first, second));
    Ok(())
}

fn verify_acl(server: &impl AsRawHandle, identity: &Identity) -> io::Result<()> {
    let mut raw = ptr::null_mut();
    // SAFETY: live pipe handle, only the final writable descriptor is requested.
    let status = unsafe {
        GetSecurityInfo(
            server.as_raw_handle(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut raw,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    demand(!raw.is_null(), "pipe ACL descriptor missing")?;
    let descriptor = Descriptor(raw);
    let mut text = ptr::null_mut();
    let mut length = 0;
    // SAFETY: the returned descriptor is owned and both outputs are writable.
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor.0,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            &mut length,
        )
    } == 0
        || text.is_null()
    {
        return Err(io::Error::last_os_error());
    }
    let _owned_text = Descriptor(text.cast());
    demand((1..=512).contains(&length), "unexpected pipe ACL length")?;
    // SAFETY: Win32 reports this count in UTF-16 code units; bound checked above.
    let value =
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length as usize) });
    let (flags, ace) = value
        .trim_end_matches('\0')
        .strip_prefix("D:")
        .and_then(|text| text.split_once('('))
        .ok_or_else(|| fail("pipe DACL missing"))?;
    let ace = ace
        .strip_suffix(')')
        .ok_or_else(|| fail("pipe DACL malformed"))?;
    let sid = identity.logon_sddl();
    demand(flags.contains('P'), "pipe DACL protection bit absent")?;
    demand(
        !ace.contains(['(', ')']) && ace.starts_with("A;") && ace.split(';').count() == 6,
        "pipe DACL does not contain one allow entry",
    )?;
    demand(
        ace.split(';').next_back() == Some(sid.as_str()),
        "pipe DACL allow entry does not match current logon",
    )
}

fn open_handle(address: &str) -> io::Result<OwnedHandle> {
    let address = wide(address);
    // SAFETY: exact lab-owned local name, non-inheritable handle with only
    // individual client rights (no FILE_CREATE_PIPE_INSTANCE) and no
    // impersonation authority. Busy is returned immediately, never waited.
    let raw = unsafe {
        CreateFileW(
            address.as_ptr(),
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful CreateFileW returned an owned pipe handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

fn open(address: &str) -> io::Result<NamedPipeClient> {
    let raw = open_handle(address)?.into_raw_handle();
    // SAFETY: the fresh overlapped pipe handle transfers ownership to Tokio,
    // including on registration error; this is called inside its I/O runtime.
    unsafe { NamedPipeClient::from_raw_handle(raw) }
}

fn receipt(code: u8) -> [u8; 5] {
    [
        wire::MAGIC[0],
        wire::MAGIC[1],
        wire::MAGIC[2],
        wire::MAGIC[3],
        code,
    ]
}

async fn write_code(pipe: &mut NamedPipeServer, code: u8) -> io::Result<()> {
    timeout(HANDSHAKE, pipe.write_all(&receipt(code))).await??;
    Ok(())
}

async fn read_code(pipe: &mut NamedPipeClient) -> io::Result<u8> {
    let mut bytes = [0; 5];
    timeout(HANDSHAKE, pipe.read_exact(&mut bytes)).await??;
    demand(&bytes[..4] == wire::MAGIC, "wrong lab receipt")?;
    Ok(bytes[4])
}

async fn read_group(pipe: &mut NamedPipeServer, limit: Duration) -> io::Result<Vec<String>> {
    timeout(limit, async {
        let mut header = [0; 8];
        pipe.read_exact(&mut header).await?;
        let mut data = vec![0; wire::size(header)?];
        pipe.read_exact(&mut data).await?;
        let paths = wire::decode(&data)?;
        if !paths.iter().all(|value| local_path(Path::new(value))) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "non-local lab file path",
            ));
        }
        Ok(paths)
    })
    .await?
}

async fn client(root: &Path, scenario: &OsStr) -> io::Result<u8> {
    let identity = Identity::current()?;
    let address = identity.address(root)?;
    if scenario == "restricted-acl" {
        return restricted_probe::child(&identity, &address).await;
    }
    let mut pipe = match open(&address) {
        Ok(pipe) => pipe,
        Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => return Ok(BUSY),
        Err(_) => return Ok(UNAVAILABLE),
    };
    if identity.verify_peer(pipe.as_raw_handle(), false).is_err()
        || read_code(&mut pipe).await.ok() != Some(READY)
    {
        return Ok(UNAVAILABLE);
    }
    let frame = if scenario == "oversize" {
        let mut header = wire::MAGIC.to_vec();
        header.extend(((wire::MAX + 1) as u32).to_be_bytes());
        header
    } else if scenario == "stall" {
        // Send an incomplete header, not any path, so the receiver's single
        // read deadline can be observed without relying on wall-clock sleeps.
        wire::MAGIC.to_vec()
    } else {
        wire::encode(TRACKS)?
    };
    if !matches!(timeout(HANDSHAKE, pipe.write_all(&frame)).await, Ok(Ok(()))) {
        return Ok(UNKNOWN);
    }
    // Never reconnect or resend once the frame could have reached the peer.
    Ok(match read_code(&mut pipe).await {
        Ok(ACCEPTED) => ACCEPTED,
        Ok(INVALID) => INVALID,
        _ => UNKNOWN,
    })
}

fn spawn(root: &Path, scenario: &str) -> io::Result<Child> {
    let mut command = Command::new(env::current_exe()?);
    command
        .arg("--lab-child")
        .arg(root)
        .arg(scenario)
        .current_dir(root)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // Only Windows loader prerequisites; no CUMULUS, account or app paths.
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = env::var_os(key) {
            command.env(key, value);
        }
    }
    command.spawn()
}

async fn wait(mut child: Child, expected: u8) -> io::Result<()> {
    let result = timeout(CHILD, child.wait()).await??;
    let code = result.code();
    if code != Some(i32::from(expected)) {
        eprintln!("native open lab child exit: expected {expected}, got {code:?}");
        return Err(fail("unexpected lab child exit"));
    }
    Ok(())
}

async fn incoming(pipe: &NamedPipeServer, identity: &Identity) -> io::Result<()> {
    timeout(HANDSHAKE, pipe.connect()).await??;
    identity.verify_peer(pipe.as_raw_handle(), true)
}

async fn run_lab() -> io::Result<()> {
    eprintln!("native open lab: directory and logon scope");
    let root = at("create primary scratch directory", tempfile::tempdir())?;
    let other = at("create alternate scratch directory", tempfile::tempdir())?;
    let identity = at("current process token", Identity::current())?;
    let address = at("primary directory identity", identity.address(root.path()))?;
    let alias = at(
        "primary directory canonicalization",
        root.path().canonicalize(),
    )?;
    demand(
        at("canonical alias identity", identity.address(&alias))? == address,
        "canonical alias changed the directory identity",
    )?;
    demand(
        at(
            "alternate directory identity",
            identity.address(other.path()),
        )? != address,
        "different scratch directories share a directory identity",
    )?;
    demand(
        !local_path(Path::new(r"\\server\share\song.mp3"))
            && !local_path(Path::new("https://example.invalid/song.flac")),
        "non-local path accepted",
    )?;
    let mut server = at("create first pipe instance", listener(&address, &identity))?;
    at("read and verify pipe DACL", verify_acl(&server, &identity))?;
    // Windows Server 2022 can report the single-instance cap before the
    // first-instance flag (ERROR_PIPE_BUSY in run 41904927043). Neither code
    // admits another listener; the separate control below still requires
    // ACCESS_DENIED specifically for FIRST_PIPE_INSTANCE.
    expect_open_error(
        "reject duplicate single-slot pipe instance",
        listener(&address, &identity),
        &[ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY],
    )?;
    verify_first_instance_flag(&address, &identity)?;
    let child = at("spawn wrong-directory child", spawn(other.path(), "normal"))?;
    at("wrong-directory child", wait(child, UNAVAILABLE).await)?;

    eprintln!("native open lab: authenticated child, exact frame and busy path");
    let first = spawn(&alias, "normal")?;
    incoming(&server, &identity).await?;
    // One persistent server instance; a simultaneous sender cannot open it.
    expect_open_error(
        "reject competing sender as busy",
        open(&address),
        &[ERROR_PIPE_BUSY],
    )?;
    write_code(&mut server, READY).await?;
    demand(
        read_group(&mut server, HANDSHAKE).await? == TRACKS,
        "synthetic paths did not round trip",
    )?;
    write_code(&mut server, ACCEPTED).await?;
    wait(first, ACCEPTED).await?;
    server.disconnect()?;

    eprintln!("native open lab: oversized frame");
    let mut server = at(
        "re-arm same address for oversized child",
        replace_used_listener(server, &address, &identity).await,
    )?;
    let invalid = at("spawn oversized child", spawn(root.path(), "oversize"))?;
    at(
        "authenticate oversized child",
        incoming(&server, &identity).await,
    )?;
    at(
        "send oversized child readiness",
        write_code(&mut server, READY).await,
    )?;
    match read_group(&mut server, HANDSHAKE).await {
        Err(error) if wire::is_oversized(&error) => {}
        Err(error) => return at("oversized header not classified", Err(error)),
        Ok(_) => return Err(fail("oversized frame accepted")),
    }
    at(
        "send oversized rejection",
        write_code(&mut server, INVALID).await,
    )?;
    at("oversized child receipt", wait(invalid, INVALID).await)?;
    server.disconnect()?;

    eprintln!("native open lab: lost receipt without replay");
    let mut server = at(
        "re-arm same address for lost receipt",
        replace_used_listener(server, &address, &identity).await,
    )?;
    let lost = spawn(root.path(), "normal")?;
    incoming(&server, &identity).await?;
    write_code(&mut server, READY).await?;
    demand(
        read_group(&mut server, HANDSHAKE).await? == TRACKS,
        "lost-ack fixture not delivered",
    )?;
    server.disconnect()?;
    wait(lost, UNKNOWN).await?;
    demand(
        timeout(Duration::from_millis(250), server.connect())
            .await
            .is_err(),
        "unexpected retry connection",
    )?;
    // A cancelled read keeps its owned overlapped I/O state until close;
    // use a fresh listener to test the timeout in isolation.
    drop(server);
    eprintln!("native open lab: bounded stalled frame");
    let mut server = listener(&identity.address(other.path())?, &identity)?;
    let stalled = spawn(other.path(), "stall")?;
    incoming(&server, &identity).await?;
    write_code(&mut server, READY).await?;
    demand(
        read_group(&mut server, Duration::from_millis(600))
            .await
            .is_err_and(|error| error.kind() == io::ErrorKind::TimedOut),
        "stalled frame not timed out",
    )?;
    write_code(&mut server, INVALID).await?;
    wait(stalled, INVALID).await?;
    server.disconnect()?;
    println!(
        "{{\"ok\":true,\"nativeWindowsNamedPipe\":true,\"actualChildProcesses\":true,\"childEnvironmentIsolated\":true,\"canonicalDirectoryIsolation\":true,\"firstInstanceExclusive\":true,\"logonOnlyDaclObserved\":true,\"sameUserAndLogonBothDirections\":true,\"unicodeSyntheticFrame\":true,\"busyBeforeSend\":true,\"boundedMalformedAndStalledFrames\":true,\"lostReceiptNotRetried\":true,\"productionEnabled\":false,\"realFilesOpened\":0,\"registryWrites\":0}}"
    );
    Ok(())
}

pub fn run() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    let kind = args.next();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    if kind.as_deref() == Some(OsStr::new("--lab-child")) {
        let root = args.next().ok_or_else(|| fail("missing lab directory"))?;
        let scenario = args.next().ok_or_else(|| fail("missing lab scenario"))?;
        demand(
            args.next().is_none()
                && ["normal", "oversize", "stall", "restricted-acl"]
                    .iter()
                    .any(|name| scenario == *name),
            "unexpected lab child arguments",
        )?;
        demand(
            env::vars_os().all(|(key, _)| {
                !key.to_string_lossy()
                    .to_ascii_uppercase()
                    .starts_with("CUMULUS_")
            }),
            "lab child inherited a Cumulus environment variable",
        )?;
        let root = Path::new(&root);
        demand(
            env::current_dir()?.canonicalize()? == root.canonicalize()?,
            "lab child current directory is not its scratch root",
        )?;
        let code = runtime.block_on(client(root, &scenario))?;
        std::process::exit(i32::from(code));
    }
    if kind.as_deref() == Some(OsStr::new("--stable-listener")) {
        demand(args.next().is_none(), "unexpected stable lab arguments")?;
        return runtime.block_on(stable_probe::run());
    }
    if kind.as_deref() == Some(OsStr::new("--restricted-acl")) {
        demand(args.next().is_none(), "unexpected restricted lab arguments")?;
        return runtime.block_on(restricted_probe::run());
    }
    demand(kind.is_none(), "unexpected lab arguments")?;
    runtime.block_on(run_lab())
}
