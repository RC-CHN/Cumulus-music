//! Windows API host for an isolated same-logon, same-directory transport.
use crate::{
    identity::{Identity, local_path},
    wire,
};
use std::{
    env,
    ffi::OsStr,
    io,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::Path,
    process::Stdio,
    ptr,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, PipeMode, ServerOptions},
    process::{Child, Command},
    time::timeout,
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
    io::Error::other(detail)
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
    let mut security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    // SAFETY: security/descriptor live until CreateNamedPipeW copies them;
    // Tokio creates an overlapped handle registered on this I/O runtime.
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .max_instances(1)
            .pipe_mode(PipeMode::Byte)
            .in_buffer_size((wire::MAX + 8) as u32)
            .out_buffer_size(64)
            .create_with_security_attributes_raw(
                address,
                (&mut security as *mut SECURITY_ATTRIBUTES).cast(),
            )
    }
}

fn verify_acl(server: &NamedPipeServer, identity: &Identity) -> io::Result<()> {
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
    if status != 0 || raw.is_null() {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
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
    demand(
        flags.contains('P')
            && !ace.contains(['(', ')'])
            && ace.starts_with("A;")
            && ace.split(';').count() == 6
            && ace.split(';').next_back() == Some(sid.as_str()),
        "pipe DACL is not restricted to the current logon",
    )
}

fn open(address: &str) -> io::Result<NamedPipeClient> {
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
    demand(
        result.code() == Some(i32::from(expected)),
        "unexpected lab child exit",
    )
}

async fn incoming(pipe: &NamedPipeServer, identity: &Identity) -> io::Result<()> {
    timeout(HANDSHAKE, pipe.connect()).await??;
    identity.verify_peer(pipe.as_raw_handle(), true)
}

async fn run_lab() -> io::Result<()> {
    eprintln!("native open lab: directory and logon scope");
    let root = tempfile::tempdir()?;
    let other = tempfile::tempdir()?;
    let identity = Identity::current()?;
    let address = identity.address(root.path())?;
    let alias = root.path().canonicalize()?;
    demand(
        identity.address(&alias)? == address && identity.address(other.path())? != address,
        "directory identity does not follow canonical aliases",
    )?;
    demand(
        !local_path(Path::new(r"\\server\share\song.mp3"))
            && !local_path(Path::new("https://example.invalid/song.flac")),
        "non-local path accepted",
    )?;
    let mut server = listener(&address, &identity)?;
    verify_acl(&server, &identity)?;
    demand(
        listener(&address, &identity)
            .err()
            .and_then(|e| e.raw_os_error())
            == Some(ERROR_ACCESS_DENIED as i32),
        "first pipe instance was not exclusive",
    )?;
    wait(spawn(other.path(), "normal")?, UNAVAILABLE).await?;

    eprintln!("native open lab: authenticated child, exact frame and busy path");
    let first = spawn(&alias, "normal")?;
    incoming(&server, &identity).await?;
    // One persistent server instance; a simultaneous sender cannot open it.
    demand(
        open(&address).err().and_then(|e| e.raw_os_error()) == Some(ERROR_PIPE_BUSY as i32),
        "competing sender did not receive pipe busy",
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
    let invalid = spawn(root.path(), "oversize")?;
    incoming(&server, &identity).await?;
    write_code(&mut server, READY).await?;
    demand(
        read_group(&mut server, HANDSHAKE)
            .await
            .is_err_and(|error| error.kind() == io::ErrorKind::InvalidData),
        "oversized frame accepted",
    )?;
    write_code(&mut server, INVALID).await?;
    wait(invalid, INVALID).await?;
    server.disconnect()?;

    eprintln!("native open lab: lost receipt without replay");
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
                && ["normal", "oversize", "stall"]
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
    demand(kind.is_none(), "unexpected lab arguments")?;
    runtime.block_on(run_lab())
}
