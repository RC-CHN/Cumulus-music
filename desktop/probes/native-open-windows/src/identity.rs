//! Use only kernel-observed identities; never log or persist them.
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io,
    mem::size_of,
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Component, Path, Prefix},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, HANDLE, WAIT_TIMEOUT},
    Security::{
        GetTokenInformation, IsValidSid, PSID, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_QUERY,
        TOKEN_USER, TokenLogonSid, TokenUser,
    },
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo, GetFileInformationByHandleEx, SYNCHRONIZE,
    },
    System::{
        Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
        SystemServices::SE_GROUP_LOGON_ID,
        Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
            WaitForSingleObject,
        },
    },
};

const MAX_TOKEN_BYTES: usize = 64 * 1024;

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "local peer identity unavailable or mismatched",
    )
}

#[derive(PartialEq, Eq)]
pub struct Identity {
    user: Vec<u8>,
    logon: Vec<u8>,
}

impl Identity {
    pub fn current() -> io::Result<Self> {
        // SAFETY: pseudo-handle needs no CloseHandle and remains valid.
        Self::for_process(unsafe { GetCurrentProcess() })
    }

    fn for_process(process: HANDLE) -> io::Result<Self> {
        let mut raw = ptr::null_mut();
        // SAFETY: process is live and output points at one writable HANDLE.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) } == 0 || raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OpenProcessToken returned unique ownership of the token handle.
        let token = unsafe { OwnedHandle::from_raw_handle(raw) };
        let user = TokenBuffer::query(token.as_raw_handle(), TokenUser)?;
        let logon = TokenBuffer::query(token.as_raw_handle(), TokenLogonSid)?;
        if user.len < size_of::<TOKEN_USER>() || logon.len < size_of::<TOKEN_GROUPS>() {
            return Err(denied());
        }
        // SAFETY: Win32 initialized the correctly aligned headers, lengths checked.
        let (user_sid, groups) = unsafe {
            (
                (*user.words.as_ptr().cast::<TOKEN_USER>()).User.Sid,
                &*logon.words.as_ptr().cast::<TOKEN_GROUPS>(),
            )
        };
        let group = groups.Groups[0];
        if groups.GroupCount != 1
            || group.Attributes & SE_GROUP_LOGON_ID as u32 != SE_GROUP_LOGON_ID as u32
        {
            return Err(denied());
        }
        Ok(Self {
            user: user.sid(user_sid)?,
            logon: logon.sid(group.Sid)?,
        })
    }

    pub fn verify_peer(&self, pipe: HANDLE, client_is_peer: bool) -> io::Result<()> {
        let query = if client_is_peer {
            GetNamedPipeClientProcessId
        } else {
            GetNamedPipeServerProcessId
        };
        let mut pid = 0;
        // SAFETY: valid pipe handle and writable PID output.
        if unsafe { query(pipe, &mut pid) } == 0 || pid == 0 {
            return Err(denied());
        }
        // SAFETY: this grants only token query and exit-status synchronization.
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
        if raw.is_null() {
            return Err(denied());
        }
        // SAFETY: OpenProcess returned a uniquely owned handle.
        let process = unsafe { OwnedHandle::from_raw_handle(raw) };
        let peer = Self::for_process(process.as_raw_handle()).map_err(|_| denied())?;
        let mut observed = 0;
        // SAFETY: both handles and the PID buffer stay valid for these calls.
        let live = unsafe {
            query(pipe, &mut observed) != 0
                && observed == pid
                && WaitForSingleObject(process.as_raw_handle(), 0) == WAIT_TIMEOUT
        };
        if !live || peer != *self {
            return Err(denied());
        }
        Ok(())
    }

    pub fn logon_sddl(&self) -> String {
        // A bounded validated binary SID: revision, count, big-endian 48-bit
        // identifier authority, then little-endian sub-authorities.
        let authority = self.logon[2..8]
            .iter()
            .fold(0u64, |value, byte| value << 8 | u64::from(*byte));
        let mut text = format!("S-{}-{authority}", self.logon[0]);
        for value in self.logon[8..].chunks_exact(4) {
            use std::fmt::Write as _;
            write!(
                text,
                "-{}",
                u32::from_le_bytes(value.try_into().expect("four SID bytes"))
            )
            .expect("String formatting");
        }
        text
    }

    pub fn address(&self, root: &Path) -> io::Result<String> {
        let canonical = root.canonicalize()?;
        if !local_path(&canonical) {
            return Err(denied());
        }
        let directory = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(canonical)?;
        if !directory.metadata()?.is_dir() {
            return Err(denied());
        }
        let mut id = FILE_ID_INFO::default();
        // SAFETY: correct information class/length and live directory handle.
        if unsafe {
            GetFileInformationByHandleEx(
                directory.as_raw_handle(),
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut hash = Sha256::new();
        hash.update(b"cumulus-windows-file-open-lab-v1\0");
        hash.update(id.VolumeSerialNumber.to_le_bytes());
        hash.update(id.FileId.Identifier);
        hash.update(&self.logon);
        Ok(format!(
            r"\\.\pipe\io.cumulus.music.open.lab.{:x}",
            hash.finalize()
        ))
    }
}

pub fn local_path(path: &Path) -> bool {
    path.is_absolute()
        && matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
}

struct TokenBuffer {
    words: Vec<usize>,
    len: usize,
}

impl TokenBuffer {
    fn query(token: HANDLE, kind: TOKEN_INFORMATION_CLASS) -> io::Result<Self> {
        let mut required = 0;
        // SAFETY: documented zero-length sizing request; output is writable.
        let status = unsafe { GetTokenInformation(token, kind, ptr::null_mut(), 0, &mut required) };
        if status != 0
            || io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
            || !(1..=MAX_TOKEN_BYTES).contains(&(required as usize))
        {
            return Err(denied());
        }
        let mut words = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
        let mut actual = 0;
        // SAFETY: aligned initialized storage has at least required bytes.
        if unsafe {
            GetTokenInformation(
                token,
                kind,
                words.as_mut_ptr().cast(),
                required,
                &mut actual,
            )
        } == 0
            || actual > required
            || actual == 0
        {
            return Err(denied());
        }
        Ok(Self {
            words,
            len: actual as usize,
        })
    }

    fn sid(&self, sid: PSID) -> io::Result<Vec<u8>> {
        let offset = (sid as usize)
            .checked_sub(self.words.as_ptr() as usize)
            .ok_or_else(denied)?;
        // SAFETY: the initialized Vec covers len bytes of the token buffer.
        let data =
            unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast::<u8>(), self.len) };
        let content = data
            .get(offset..)
            .filter(|value| value.len() >= 8)
            .ok_or_else(denied)?;
        let count = usize::from(content[1]);
        let sid_bytes = content.get(..8 + 4 * count).ok_or_else(denied)?;
        // SAFETY: the complete SID lies inside the token buffer and count is bounded.
        if count > 15 || unsafe { IsValidSid(sid) } == 0 {
            return Err(denied());
        }
        Ok(sid_bytes.to_vec())
    }
}
