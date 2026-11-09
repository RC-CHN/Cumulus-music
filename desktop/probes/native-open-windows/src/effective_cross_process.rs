//! Fast native control for the product's handshake, without launching Tauri.
use crate::{
    effective::{
        AccessShape, Connector, HELLO, restricted_same_logon_for_test, with_test_impersonation,
    },
    identity::{Identity, local_path},
    security::{open_handle, wide, with_logon_security},
    stable::{Server, Stop},
    wire::{self, CONSUMED, READY, receipt, receipt_code},
};
use std::{
    env, io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, IntoRawHandle, RawHandle},
    },
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::NamedPipeClient,
    process::Command,
    time::timeout,
};
use windows_sys::Win32::{
    Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED},
    System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
};

const TEST: &str = "effective_cross_process::effective_cross_process_product_protocol";
const ROOT_PREFIX: &str = "cumulus-native-open-cross-process-";
const CHILD: &str = "CUMULUS_NATIVE_OPEN_CROSS_PROCESS_CHILD";
const ROOT: &str = "CUMULUS_NATIVE_OPEN_CROSS_PROCESS_ROOT";
const OWNER_PID: &str = "CUMULUS_NATIVE_OPEN_CROSS_PROCESS_OWNER_PID";
const IO: Duration = Duration::from_secs(2);
const CONNECT: Duration = Duration::from_secs(10);
const ACK: Duration = Duration::from_secs(20);
const ACCEPTED: u8 = 0;
const UNAVAILABLE: u8 = 2;
const INVALID: u8 = 3;
const TRACKS: &[&str] = &[r"C:\合成\01 % 空格.flac", r"C:\合成\02.mp3"];

#[derive(Clone, Copy)]
enum Case {
    Accepted,
    Invalid,
    Unready,
    Restricted,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Invalid => "invalid",
            Self::Unready => "unready",
            Self::Restricted => "restricted",
        }
    }
}

fn path_step<T>(result: io::Result<T>, stage: &'static str) -> T {
    result.unwrap_or_else(|error| {
        panic!("{stage}: {:?} ({:?})", error.kind(), error.raw_os_error());
    })
}

fn private_address(root: &Path, identity: &Identity) -> String {
    let candidate = root.file_name().and_then(|name| name.to_str());
    assert!(
        root.is_absolute()
            && root.as_os_str().encode_wide().count() <= 4096
            && candidate
                .and_then(|name| name.strip_prefix(ROOT_PREFIX))
                .is_some_and(|suffix| {
                    (1..=16).contains(&suffix.len())
                        && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
                }),
        "cross-process proof requires a bounded synthetic scratch directory"
    );
    // Identity::address derives the lab prefix, never the product endpoint.
    path_step(identity.address(root), "derive private lab endpoint")
}

fn assert_peer_pid(pipe: RawHandle, client_is_peer: bool, expected: u32) {
    let query = if client_is_peer {
        GetNamedPipeClientProcessId
    } else {
        GetNamedPipeServerProcessId
    };
    let mut actual = 0;
    // SAFETY: the supplied pipe is live and the PID output is writable.
    let success = unsafe { query(pipe, &mut actual) } != 0;
    assert!(
        success && expected != std::process::id() && actual == expected,
        "pipe peer must be the expected independent process"
    );
}

fn open(address: &str) -> NamedPipeClient {
    let handle = open_handle(address).expect("open exact production client handle");
    // SAFETY: unique overlapped handle, transferred to Tokio inside its runtime.
    unsafe { NamedPipeClient::from_raw_handle(handle.into_raw_handle()) }
        .expect("register overlapped client")
}

async fn write(pipe: &mut NamedPipeClient, frame: &[u8]) {
    timeout(IO, pipe.write_all(frame))
        .await
        .expect("client write deadline")
        .expect("client write");
}

async fn read(pipe: &mut NamedPipeClient, limit: Duration) -> u8 {
    let mut bytes = [0; 5];
    timeout(limit, pipe.read_exact(&mut bytes))
        .await
        .expect("client receipt deadline")
        .expect("client receipt read");
    receipt_code(bytes).expect("exact production receipt")
}

fn child(root: &Path, owner_pid: u32, case: Case, runtime: &tokio::runtime::Runtime) {
    let identity = Identity::current().expect("identify child process");
    let access = AccessShape::current().expect("read unrestricted child primary token");
    assert!(access.matches_identity(&identity));
    let address = private_address(root, &identity);
    if matches!(case, Case::Restricted) {
        let restricted = restricted_same_logon_for_test();
        with_test_impersonation(&restricted, || {
            runtime.block_on(async {
                let mut pipe = open(&address);
                assert_peer_pid(pipe.as_raw_handle(), false, owner_pid);
                write(&mut pipe, &[HELLO]).await;
                // An attacker tries HELLO directly. The owner must close
                // without READY even though the child's process token passes.
                let mut reply = [0; 5];
                assert!(
                    timeout(IO, pipe.read_exact(&mut reply))
                        .await
                        .expect("restricted connection must be refused before deadline")
                        .is_err(),
                    "restricted child received a receipt"
                );
            });
        });
        return;
    }
    runtime.block_on(async {
        let mut pipe = open(&address);
        assert_peer_pid(pipe.as_raw_handle(), false, owner_pid);
        access
            .verify_process(pipe.as_raw_handle(), false)
            .expect("sender verifies owner before HELLO");
        write(&mut pipe, &[HELLO]).await;
        let ready = read(&mut pipe, IO).await;
        if matches!(case, Case::Unready) {
            assert_eq!(ready, UNAVAILABLE);
            return;
        }
        assert_eq!(ready, READY);
        access
            .verify_process(pipe.as_raw_handle(), false)
            .expect("sender rechecks owner after READY");
        write(
            &mut pipe,
            &wire::encode(TRACKS).expect("encode synthetic frame"),
        )
        .await;
        assert_eq!(
            read(&mut pipe, ACK).await,
            if matches!(case, Case::Accepted) {
                ACCEPTED
            } else {
                INVALID
            }
        );
        write(&mut pipe, &[CONSUMED]).await;
    });
}

fn owner_round(server: &mut Server, access: &AccessShape, child_pid: u32, case: Case) {
    server
        .connect(Instant::now() + CONNECT)
        .expect("accept one child");
    assert_peer_pid(server.as_raw_handle(), true, child_pid);
    let mut failure = None;
    let connector = access.authenticate(server, Instant::now() + IO, |stage, code| {
        failure = Some((stage, code));
    });
    if matches!(case, Case::Restricted) {
        assert_eq!(connector, Connector::Denied);
        assert_eq!(failure, Some(("owner.sender_effective_token_denied", None)));
    } else {
        assert_eq!(failure, None, "unexpected authentication failure stage");
        assert_eq!(connector, Connector::Verified);
        if matches!(case, Case::Unready) {
            server
                .write_all(&receipt(UNAVAILABLE), Instant::now() + IO)
                .expect("send unavailable without granting READY");
            let error = server
                .read_exact(&mut [0], Instant::now() + IO)
                .expect_err("unready sender must not write any frame bytes");
            assert!(
                error.kind() == io::ErrorKind::UnexpectedEof
                    || error.raw_os_error().is_some_and(|code| {
                        [ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED]
                            .contains(&(code as u32))
                    }),
                "unready sender must close its connection, not time out"
            );
        } else {
            server
                .write_all(&receipt(READY), Instant::now() + IO)
                .expect("send exact READY");
            let deadline = Instant::now() + IO;
            let mut header = [0; 8];
            server
                .read_exact(&mut header, deadline)
                .expect("read frame header");
            let mut body = vec![0; wire::size(header).expect("bound frame length")];
            server
                .read_exact(&mut body, deadline)
                .expect("read frame body");
            let paths = wire::decode(&body).expect("decode exact frame");
            assert!(
                paths.iter().map(String::as_str).eq(TRACKS.iter().copied())
                    && paths.iter().all(|path| local_path(Path::new(path))),
                "exact bounded synthetic Unicode paths"
            );
            assert_eq!(
                access.verify_connector(server.as_raw_handle()),
                Connector::Verified
            );
            access
                .verify_process(server.as_raw_handle(), true)
                .expect("owner rechecks sender after frame");
            let reply = if matches!(case, Case::Accepted) {
                ACCEPTED
            } else {
                INVALID
            };
            server
                .write_all(&receipt(reply), Instant::now() + IO)
                .expect("write final receipt");
            let mut done = [0];
            server
                .read_exact(&mut done, Instant::now() + IO)
                .expect("await receipt consumption");
            assert_eq!(done, [CONSUMED]);
        }
    }
    server
        .disconnect()
        .expect("retain and reset the original server");
}

#[test]
fn effective_cross_process_product_protocol() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("bounded native I/O runtime");
    if let Some(mode) = env::var_os(CHILD) {
        let case = [
            Case::Accepted,
            Case::Invalid,
            Case::Unready,
            Case::Restricted,
        ]
        .into_iter()
        .find(|case| mode == case.name())
        .expect("valid child case");
        let root = env::var_os(ROOT).expect("child scratch directory");
        let owner_pid = env::var(OWNER_PID)
            .expect("parent identity")
            .parse()
            .expect("parent PID");
        child(Path::new(&root), owner_pid, case, &runtime);
        return;
    }
    let root = path_step(
        tempfile::Builder::new().prefix(ROOT_PREFIX).tempdir(),
        "create synthetic scratch directory",
    );
    let identity = Identity::current().expect("identify owner process");
    let access = AccessShape::current().expect("read owner process token");
    assert!(access.matches_identity(&identity));
    let address = wide(&private_address(root.path(), &identity));
    let mut server = with_logon_security(&identity, |attributes| {
        let stop = Stop::new()?;
        // SAFETY: a private local address and a live protected logon DACL.
        unsafe { Server::create(&address, attributes, stop) }
    })
    .expect("bind the first and only local pipe instance");
    runtime.block_on(async {
        // Reuse exactly one retained listener, including after denied/unready clients.
        for case in [
            Case::Accepted,
            Case::Unready,
            Case::Restricted,
            Case::Invalid,
        ] {
            let mut command = Command::new(path_step(env::current_exe(), "locate test executable"));
            command
                .args(["--exact", TEST, "--nocapture"])
                .env_clear()
                .env(CHILD, case.name())
                .env(ROOT, root.path())
                .env(OWNER_PID, std::process::id().to_string())
                .current_dir(root.path())
                .stdin(Stdio::null())
                .kill_on_drop(true);
            for key in ["SystemRoot", "WINDIR"] {
                if let Some(value) = env::var_os(key) {
                    command.env(key, value);
                }
            }
            eprintln!("native open cross-process: {}", case.name());
            let mut child = path_step(command.spawn(), "spawn exact test executable");
            owner_round(
                &mut server,
                &access,
                child.id().expect("child is live"),
                case,
            );
            let status = timeout(CONNECT, child.wait())
                .await
                .expect("child exit deadline")
                .expect("observe child exit");
            assert!(
                status.success(),
                "native open child did not complete its assertions"
            );
        }
    });
}
