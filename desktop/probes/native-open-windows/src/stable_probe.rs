//! Actual Win32 fixture for the retained pipe, separate from the baseline.
use super::{
    ACCEPTED, HANDSHAKE, INVALID, READY, TRACKS, UNKNOWN, at, demand, expect_open_error, fail,
    receipt, spawn, verify_acl, wait, wide, with_logon_security,
};
use crate::{
    identity::{Identity, local_path},
    stable::{End, Server, Stop},
    wire,
};
use std::{
    io,
    os::windows::io::AsRawHandle,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY};

fn stable_listener(address: &str, identity: &Identity, stop: Stop) -> io::Result<Server> {
    let address = wide(address);
    // SAFETY: address from the private scratch identity, NUL terminated.
    // with_logon_security keeps the descriptor live until CreateNamedPipeW.
    with_logon_security(identity, |security| unsafe {
        Server::create(&address, security, stop)
    })
}

fn stable_ready(server: &mut Server, identity: &Identity) -> io::Result<()> {
    at("stable accept", server.connect(Instant::now() + HANDSHAKE))?;
    at(
        "stable verify child token",
        identity.verify_peer(server.as_raw_handle(), true),
    )?;
    at(
        "stable write readiness",
        server.write_all(&receipt(READY), Instant::now() + HANDSHAKE),
    )
}

fn stable_group(server: &mut Server) -> io::Result<Vec<String>> {
    let deadline = Instant::now() + HANDSHAKE;
    let mut header = [0; 8];
    server.read_exact(&mut header, deadline)?;
    let mut body = vec![0; wire::size(header)?];
    server.read_exact(&mut body, deadline)?;
    let paths = wire::decode(&body)?;
    if !paths.iter().all(|path| local_path(Path::new(path))) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "non-local lab path",
        ));
    }
    Ok(paths)
}

async fn stable_round_trip(
    server: &mut Server,
    identity: &Identity,
    root: &Path,
) -> io::Result<()> {
    let child = at("spawn stable child", spawn(root, "normal"))?;
    stable_ready(server, identity)?;
    demand(
        at("read stable frame", stable_group(server))? == TRACKS,
        "stable frame mismatch",
    )?;
    at(
        "write stable success",
        server.write_all(&receipt(ACCEPTED), Instant::now() + HANDSHAKE),
    )?;
    // A Win32 disconnect can discard a receipt the child has not read.
    // The synthetic child exits only after reading it; never FlushFileBuffers.
    at("stable child terminal receipt", wait(child, ACCEPTED).await)?;
    at("stable disconnect after receipt", server.disconnect())
}

fn with_stop<T>(
    server: &mut Server,
    stop: &Stop,
    pending: impl FnOnce(&mut Server) -> io::Result<T>,
) -> io::Result<T> {
    let started = Stop::new()?;
    server.notify_next_pending(started.clone());
    thread::scope(|scope| {
        let stop = stop.clone();
        let signal = scope.spawn(move || {
            started.wait(Instant::now() + HANDSHAKE)?;
            stop.signal()
        });
        let outcome = pending(server);
        signal
            .join()
            .map_err(|_| fail("stable stop helper panicked"))??;
        outcome
    })
}

pub(super) async fn run() -> io::Result<()> {
    eprintln!("native open lab: stable overlapped listener");
    let root = tempfile::tempdir()?;
    let other = tempfile::tempdir()?;
    let identity = Identity::current()?;
    let address = identity.address(root.path())?;
    let stop = Stop::new()?;
    let mut server = at(
        "create stable first listener",
        stable_listener(&address, &identity, stop.clone()),
    )?;
    at("verify stable DACL", verify_acl(&server, &identity))?;
    expect_open_error(
        "reject stable duplicate listener",
        stable_listener(&address, &identity, stop.clone()),
        &[ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY],
    )?;
    let original = server.as_raw_handle();
    let alias = root.path().canonicalize()?;
    stable_round_trip(&mut server, &identity, &alias).await?;
    stable_round_trip(&mut server, &identity, root.path()).await?;

    eprintln!("native open lab: stable oversized frame");
    let child = at(
        "spawn stable oversized child",
        spawn(root.path(), "oversize"),
    )?;
    stable_ready(&mut server, &identity)?;
    match stable_group(&mut server) {
        Err(error) if wire::is_oversized(&error) => {}
        Err(error) => return at("stable oversized header not classified", Err(error)),
        Ok(_) => return Err(fail("stable oversized frame accepted")),
    }
    at(
        "write stable oversized rejection",
        server.write_all(&receipt(INVALID), Instant::now() + HANDSHAKE),
    )?;
    at("stable oversized child receipt", wait(child, INVALID).await)?;
    at("stable disconnect after oversize", server.disconnect())?;

    eprintln!("native open lab: stable timeout then same-handle recovery");
    let child = at("spawn stable stalled child", spawn(root.path(), "stall"))?;
    stable_ready(&mut server, &identity)?;
    let mut marker = [0; 4];
    at(
        "read stable partial header",
        server.read_exact(&mut marker, Instant::now() + HANDSHAKE),
    )?;
    demand(&marker == wire::MAGIC, "stable partial header mismatch")?;
    let mut remaining = [0; 4];
    match server.read_exact(&mut remaining, Instant::now() + Duration::from_millis(300)) {
        Err(error) if End::PendingDeadline.is(&error) => {}
        Err(error) => return at("stable read deadline did not drain", Err(error)),
        Ok(()) => return Err(fail("stable stalled child completed its header")),
    }
    at(
        "write stable stalled rejection",
        server.write_all(&receipt(INVALID), Instant::now() + HANDSHAKE),
    )?;
    at("stable stalled child receipt", wait(child, INVALID).await)?;
    at("stable disconnect after timeout", server.disconnect())?;
    stable_round_trip(&mut server, &identity, root.path()).await?;
    demand(
        server.as_raw_handle() == original,
        "stable listener handle changed",
    )?;

    eprintln!("native open lab: cancel and drain pending stable read");
    let child = at(
        "spawn stable read cancellation child",
        spawn(root.path(), "stall"),
    )?;
    stable_ready(&mut server, &identity)?;
    at(
        "read stable cancellation marker",
        server.read_exact(&mut marker, Instant::now() + HANDSHAKE),
    )?;
    demand(
        &marker == wire::MAGIC,
        "stable cancellation marker mismatch",
    )?;
    let outcome = with_stop(&mut server, &stop, |server| {
        server.read_exact(&mut remaining, Instant::now() + HANDSHAKE)
    });
    match outcome {
        Err(error) if End::PendingStopped.is(&error) => {}
        Err(error) => return at("stable read cancellation did not drain", Err(error)),
        Ok(()) => return Err(fail("stable canceled read completed")),
    }
    at("disconnect canceled stable client", server.disconnect())?;
    at(
        "stable cancellation child reports unknown",
        wait(child, UNKNOWN).await,
    )?;

    eprintln!("native open lab: cancel and drain pending stable accept");
    let accept_stop = Stop::new()?;
    let mut idle = stable_listener(
        &identity.address(other.path())?,
        &identity,
        accept_stop.clone(),
    )?;
    match with_stop(&mut idle, &accept_stop, |server| {
        server.connect(Instant::now() + HANDSHAKE)
    }) {
        Err(error) if End::PendingStopped.is(&error) => {}
        Err(error) => return at("stable accept cancellation did not drain", Err(error)),
        Ok(()) => return Err(fail("stable canceled accept completed")),
    }
    println!(
        "{{\"ok\":true,\"nativeWindowsNamedPipe\":true,\"sameStableHandleAcrossClients\":true,\"logonOnlyDaclObserved\":true,\"singleInstanceExclusive\":true,\"oversizedHeaderRejected\":true,\"pendingTimeoutDrained\":true,\"pendingReadStopDrained\":true,\"pendingAcceptStopDrained\":true,\"productionEnabled\":false,\"realFilesOpened\":0,\"registryWrites\":0}}"
    );
    Ok(())
}
