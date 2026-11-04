//! One bounded Linux AF_UNIX request and a separate terminal receipt.
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        linux::net::SocketAddrExt,
        unix::{
            ffi::OsStrExt,
            net::{SocketAddr, UnixStream},
        },
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAGIC: &[u8; 4] = b"CFO1";
const MAX_FRAME: usize = 32 * 1024;
const MAX_PATHS: usize = 8;
const IO_LIMIT: Duration = Duration::from_secs(2);
const ACK_LIMIT: Duration = Duration::from_secs(20);
pub(super) const READY: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Reply {
    Accepted = 0,
    Unavailable = 2,
    Invalid = 3,
    Unknown = 4,
    Busy = 5,
}
impl Reply {
    pub(super) fn finish(self) -> ! {
        if self != Self::Accepted {
            eprintln!(
                "{}",
                match self {
                    Self::Unavailable =>
                        "Cumulus cannot hand off to the existing app profile; try when its main player is ready.",
                    Self::Invalid => "Cumulus could not open a readable MP3 or FLAC file.",
                    Self::Unknown =>
                        "Cumulus did not confirm the handoff. Check the existing player before opening the files again.",
                    Self::Busy => "Cumulus is already processing an explicit file open.",
                    Self::Accepted => unreachable!(),
                }
            );
        }
        std::process::exit(self as i32);
    }
}

pub(super) fn address(root: &Path) -> io::Result<SocketAddr> {
    let mut hash = Sha256::new();
    hash.update(b"cumulus-native-open-v1\0");
    hash.update(root.as_os_str().as_bytes());
    SocketAddr::from_abstract_name(format!(
        "io.cumulus.music.open.{}.{:x}",
        effective_uid(),
        hash.finalize()
    ))
}

pub(super) fn effective_uid() -> libc::uid_t {
    // SAFETY: no pointers, memory or process mutation.
    unsafe { libc::geteuid() }
}

pub(super) fn same_user(stream: &UnixStream) -> io::Result<()> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of_val(&cred) as libc::socklen_t;
    // SAFETY: fixed Linux SO_PEERCRED ABI; both pointers are writable until return.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if status < 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != std::mem::size_of_val(&cred) || cred.pid <= 0 || cred.uid != effective_uid()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native relay peer uid mismatch",
        ));
    }
    Ok(())
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid native open frame")
}

fn path_bytes(path: &Path) -> io::Result<&[u8]> {
    path.to_str()
        .filter(|value| path.is_absolute() && !value.chars().any(char::is_control))
        .map(str::as_bytes)
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= 4096)
        .ok_or_else(invalid)
}

fn encode(paths: &[PathBuf]) -> io::Result<Vec<u8>> {
    if paths.is_empty() || paths.len() > MAX_PATHS {
        return Err(invalid());
    }
    let mut frame = Vec::with_capacity(128);
    frame.extend(MAGIC);
    frame.extend([0; 4]);
    frame.push(paths.len() as u8);
    for path in paths {
        let bytes = path_bytes(path)?;
        frame.extend((bytes.len() as u16).to_be_bytes());
        frame.extend(bytes);
    }
    let len = frame.len() - 8;
    if len > MAX_FRAME {
        return Err(invalid());
    }
    frame[4..8].copy_from_slice(&(len as u32).to_be_bytes());
    Ok(frame)
}

fn decode(mut frame: &[u8]) -> io::Result<Vec<PathBuf>> {
    let (&count, rest) = frame.split_first().ok_or_else(invalid)?;
    if count == 0 || usize::from(count) > MAX_PATHS {
        return Err(invalid());
    }
    frame = rest;
    let mut paths = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let (len, rest) = frame.split_at_checked(2).ok_or_else(invalid)?;
        let (bytes, rest) = rest
            .split_at_checked(usize::from(u16::from_be_bytes([len[0], len[1]])))
            .ok_or_else(invalid)?;
        let path = PathBuf::from(std::str::from_utf8(bytes).map_err(|_| invalid())?);
        path_bytes(&path)?;
        paths.push(path);
        frame = rest;
    }
    if !frame.is_empty() {
        return Err(invalid());
    }
    Ok(paths)
}

fn read_exact(stream: &mut UnixStream, mut out: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !out.is_empty() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(Some(left))?;
        match stream.read(out) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => out = &mut out[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) fn receive(stream: &mut UnixStream) -> io::Result<Vec<PathBuf>> {
    let deadline = Instant::now() + IO_LIMIT;
    let mut head = [0; 8];
    read_exact(stream, &mut head, deadline)?;
    let len = u32::from_be_bytes(head[4..8].try_into().map_err(|_| invalid())?) as usize;
    if &head[..4] != MAGIC || !(1..=MAX_FRAME).contains(&len) {
        return Err(invalid());
    }
    let mut payload = vec![0; len];
    read_exact(stream, &mut payload, deadline)?;
    let mut trailer = [0];
    // Require the sender's write half to be closed; a timeout or trailing
    // bytes cannot complete a request.
    match read_exact(stream, &mut trailer, deadline) {
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => decode(&payload),
        _ => Err(invalid()),
    }
}

pub(super) fn send_code(stream: &mut UnixStream, code: u8) -> io::Result<()> {
    stream.set_write_timeout(Some(IO_LIMIT))?;
    stream.write_all(&[MAGIC[0], MAGIC[1], MAGIC[2], MAGIC[3], code])
}

fn read_code(stream: &mut UnixStream, limit: Duration) -> io::Result<u8> {
    let mut receipt = [0; 5];
    read_exact(stream, &mut receipt, Instant::now() + limit)?;
    if &receipt[..4] != MAGIC {
        return Err(invalid());
    }
    Ok(receipt[4])
}

fn connect(address: &SocketAddr) -> io::Result<UnixStream> {
    let name = address.as_abstract_name().ok_or_else(invalid)?;
    // SAFETY: creates an independently owned Linux AF_UNIX file descriptor.
    let raw = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: socket returned a fresh descriptor; OwnedFd closes it on all paths.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut native = libc::sockaddr_un {
        sun_family: libc::AF_UNIX as libc::sa_family_t,
        sun_path: [0; 108],
    };
    if name.len() >= native.sun_path.len() {
        return Err(invalid());
    }
    for (target, value) in native.sun_path[1..].iter_mut().zip(name) {
        *target = *value as libc::c_char;
    }
    let len = std::mem::offset_of!(libc::sockaddr_un, sun_path) + 1 + name.len();
    // SAFETY: fd is a live nonblocking socket; len is the initialized
    // sockaddr_un prefix. Backlog exhaustion fails closed without a retry.
    let status = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&native as *const libc::sockaddr_un).cast(),
            len as libc::socklen_t,
        )
    };
    if status < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(false)?;
    Ok(stream)
}

pub(super) fn forward(address: &SocketAddr, paths: &[PathBuf]) -> Reply {
    let Ok(frame) = encode(paths) else {
        return Reply::Invalid;
    };
    let Ok(mut stream) = connect(address) else {
        return Reply::Unavailable;
    };
    if same_user(&stream).is_err() || stream.set_write_timeout(Some(IO_LIMIT)).is_err() {
        return Reply::Unavailable;
    }
    match read_code(&mut stream, IO_LIMIT) {
        Ok(READY) => {}
        Ok(code) if code == Reply::Busy as u8 => return Reply::Busy,
        _ => return Reply::Unavailable,
    }
    // Nothing retries after this point: a failed or lost ACK can follow a
    // successfully committed player selection.
    if stream.write_all(&frame).is_err() || stream.shutdown(std::net::Shutdown::Write).is_err() {
        return Reply::Unknown;
    }
    match read_code(&mut stream, ACK_LIMIT) {
        Ok(0) => Reply::Accepted,
        Ok(2) => Reply::Unavailable,
        Ok(3) => Reply::Invalid,
        _ => Reply::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::net::UnixListener, thread};

    #[test]
    fn wire_uses_one_bounded_lossless_path_group() {
        let expected = vec![
            PathBuf::from("/tmp/合成 空格.flac"),
            PathBuf::from("/tmp/second.mp3"),
        ];
        let frame = encode(&expected).unwrap();
        let (mut sender, mut recipient) = UnixStream::pair().unwrap();
        same_user(&sender).unwrap();
        same_user(&recipient).unwrap();
        sender.write_all(&frame).unwrap();
        sender.shutdown(std::net::Shutdown::Write).unwrap();
        assert_eq!(receive(&mut recipient).unwrap(), expected);

        let (mut sender, mut recipient) = UnixStream::pair().unwrap();
        sender.write_all(&frame).unwrap();
        sender.write_all(b"trailing request").unwrap();
        sender.shutdown(std::net::Shutdown::Write).unwrap();
        assert_eq!(
            receive(&mut recipient).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(encode(&[PathBuf::from("/tmp/invalid\nname.flac")]).is_err());
        assert!(encode(&[PathBuf::from("relative.mp3")]).is_err());
        assert!(encode(&vec![PathBuf::from("/tmp/track.flac"); 9]).is_err());
        assert!(decode(&[1, 0, 1, 0xff]).is_err());
    }

    #[test]
    fn ready_busy_and_uncertain_receipt_do_not_send_or_replay() {
        let root = tempfile::tempdir().unwrap();
        let address = address(root.path()).unwrap();
        let listener = UnixListener::bind_addr(&address).unwrap();
        let track = PathBuf::from("/tmp/合成 空格.flac");
        let expected = track.clone();
        let host = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            same_user(&stream).unwrap();
            send_code(&mut stream, Reply::Busy as u8).unwrap();
            let mut unexpected = [0];
            stream.set_read_timeout(Some(IO_LIMIT)).unwrap();
            assert_eq!(stream.read(&mut unexpected).unwrap(), 0);

            let (mut stream, _) = listener.accept().unwrap();
            send_code(&mut stream, READY).unwrap();
            assert_eq!(receive(&mut stream).unwrap(), [expected]);
            // Simulate one delivered operation whose terminal ACK was lost.
            drop(stream);
            listener.set_nonblocking(true).unwrap();
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
        });
        assert_eq!(forward(&address, std::slice::from_ref(&track)), Reply::Busy);
        assert_eq!(forward(&address, &[track]), Reply::Unknown);
        host.join().unwrap();
    }
}
