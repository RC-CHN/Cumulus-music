//! Read only pinned, stopped Linux SQLite files; never let SQLite open the source path.
use super::{refused, safe};
use cumulus_core::Result;
use rusqlite::{Connection, OptionalExtension, ffi, params};
use serde_json::Value;
use std::{
    ffi::{CStr, CString, OsStr},
    fs::{File, Metadata},
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::{Component, Path},
    ptr,
    time::{Duration, Instant},
};

const DATABASE: &CStr = c"core.sqlite3";
const SIDECARS: [&CStr; 3] = [
    c"core.sqlite3-wal",
    c"core.sqlite3-shm",
    c"core.sqlite3-journal",
];
const SQLITE_HEADER: &[u8] = b"SQLite format 3\0";
const SOURCE_BOUNDS: u64 = 64 * 1024 * 1024;

fn open_at(parent: i32, name: &CStr, flags: i32) -> std::io::Result<File> {
    // SAFETY: `name` is terminated and `parent` is a live directory or AT_FDCWD;
    // no O_CREAT is used, and success gives this call ownership of the fd.
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: openat returned a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn directory_at(parent: i32, name: &CStr) -> Result<File> {
    safe(
        open_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY),
        "unsafe proof directory",
    )
}

pub(super) fn open_absolute(path: &Path) -> Result<File> {
    let mut parts = path.components();
    if parts.next() != Some(Component::RootDir) {
        return Err(refused("proof paths must be absolute"));
    }
    let mut current = directory_at(libc::AT_FDCWD, c"/")?;
    for part in parts {
        let Component::Normal(name) = part else {
            return Err(refused("unsafe proof path component"));
        };
        current = directory_at(current.as_raw_fd(), &name_cstr(name)?)?;
    }
    Ok(current)
}

fn name_cstr(name: &OsStr) -> Result<CString> {
    safe(
        CString::new(name.as_encoded_bytes()),
        "unsafe proof path component",
    )
}

pub(super) fn child(parent: &File, name: &str) -> Result<File> {
    let name = name_cstr(OsStr::new(name))?;
    let child = directory_at(parent.as_raw_fd(), &name)?;
    require_private(&safe(child.metadata(), "proof directory is unavailable")?)?;
    Ok(child)
}

pub(super) fn require_private(meta: &Metadata) -> Result<()> {
    // SAFETY: geteuid reads only this process's effective identity.
    if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return Err(refused(
            "proof input and output must be private and owned by the caller",
        ));
    }
    Ok(())
}

pub(super) fn same_inode(left: &Metadata, right: &Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn stable(left: &Metadata, right: &Metadata) -> bool {
    same_inode(left, right)
        && (left.mode(), left.uid(), left.nlink(), left.len())
            == (right.mode(), right.uid(), right.nlink(), right.len())
        && (
            left.mtime(),
            left.mtime_nsec(),
            left.ctime(),
            left.ctime_nsec(),
        ) == (
            right.mtime(),
            right.mtime_nsec(),
            right.ctime(),
            right.ctime_nsec(),
        )
}

fn no_sidecars(directory: &File) -> Result<()> {
    for sidecar in SIDECARS {
        match open_at(
            directory.as_raw_fd(),
            sidecar,
            libc::O_RDONLY | libc::O_NONBLOCK,
        ) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => (),
            _ => return Err(refused("source must be stopped without SQLite sidecars")),
        }
    }
    Ok(())
}

fn private_bytes_at(directory: &File, name: &CStr, bound: u64) -> Result<(Vec<u8>, Metadata)> {
    let open = || {
        safe(
            open_at(
                directory.as_raw_fd(),
                name,
                libc::O_RDONLY | libc::O_NONBLOCK,
            ),
            "private input file is unavailable",
        )
    };
    let mut file = open()?;
    let before = safe(file.metadata(), "private input file is unavailable")?;
    require_private(&before)?;
    if !before.is_file() || before.nlink() != 1 || !(1..=bound).contains(&before.len()) {
        return Err(refused("private input is not a bounded regular file"));
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    safe(
        (&mut file).take(bound + 1).read_to_end(&mut bytes),
        "private input file could not be read",
    )?;
    let after = safe(file.metadata(), "private input file is unavailable")?;
    let current = safe(open()?.metadata(), "private input file is unavailable")?;
    if bytes.len() as u64 != before.len() || !stable(&before, &after) || !stable(&before, &current)
    {
        return Err(refused("private input file changed during preparation"));
    }
    Ok((bytes, before))
}

pub(super) fn private_input(path: &Path, bound: u64) -> Result<(Vec<u8>, Metadata)> {
    let parent = path
        .parent()
        .ok_or_else(|| refused("invalid private input path"))?;
    let name = path
        .file_name()
        .ok_or_else(|| refused("invalid private input path"))?;
    private_bytes_at(&open_absolute(parent)?, &name_cstr(name)?, bound)
}

pub(super) fn stopped_database_bytes(directory: &File) -> Result<(Vec<u8>, Metadata)> {
    no_sidecars(directory)?;
    let (bytes, file) = private_bytes_at(directory, DATABASE, SOURCE_BOUNDS)?;
    if bytes.len() < 100
        || !bytes.starts_with(SQLITE_HEADER)
        || !matches!(bytes.get(18..20), Some([1, 1] | [2, 2]))
    {
        return Err(refused(
            "source must be a stable standalone SQLite database",
        ));
    }
    no_sidecars(directory)?;
    Ok((bytes, file))
}

pub(super) struct Snapshot {
    pub(super) database: Connection,
    file: Metadata,
}

impl Snapshot {
    pub(super) fn read(directory: &File) -> Result<Self> {
        let (mut bytes, file) = stopped_database_bytes(directory)?;
        // sqlite3_deserialize requires rollback mode. A closed WAL-mode
        // database may retain [2,2] in its header after the final checkpoint.
        // Normalize only our memory copy after excluding all live sidecars.
        bytes[18..20].copy_from_slice(&[1, 1]);
        let database = backup_bytes(&bytes)?;
        let snapshot = Self { database, file };
        snapshot.recheck(directory)?;
        Ok(snapshot)
    }

    pub(super) fn recheck(&self, directory: &File) -> Result<()> {
        no_sidecars(directory)?;
        let current = safe(
            open_at(
                directory.as_raw_fd(),
                DATABASE,
                libc::O_RDONLY | libc::O_NONBLOCK,
            ),
            "source database is unavailable",
        )?;
        if !stable(
            &self.file,
            &safe(current.metadata(), "source database is unavailable")?,
        ) {
            return Err(refused("source database changed during preparation"));
        }
        Ok(())
    }

    pub(super) fn row(
        &self,
        owner: &str,
        kind: &str,
        key: &str,
        max: i64,
    ) -> Result<Option<Value>> {
        let raw: Option<String> = safe(self.database.query_row(
            "SELECT CASE WHEN typeof(value)='text' AND length(CAST(value AS BLOB)) <= ?4 THEN value END
             FROM state WHERE owner=?1 AND kind=?2 AND key=?3",
            params![owner, kind, key, max], |row| row.get(0),
        ).optional(), "invalid source state row")?;
        raw.as_deref()
            .map(|raw| safe(serde_json::from_str(raw), "invalid source state row"))
            .transpose()
    }
}

fn backup_bytes(bytes: &[u8]) -> Result<Connection> {
    let input = safe(
        Connection::open_in_memory(),
        "could not open a private SQLite snapshot",
    )?;
    safe(
        input.execute_batch("PRAGMA trusted_schema=OFF;"),
        "could not configure a private SQLite snapshot",
    )?;
    // SAFETY: SQLite owns the allocated buffer after deserialize (including
    // failure with FREEONCLOSE); length is bounded and fits both integer types.
    let status = unsafe {
        let buffer = ffi::sqlite3_malloc64(bytes.len() as u64).cast::<u8>();
        if buffer.is_null() {
            return Err(refused("could not allocate a private SQLite snapshot"));
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len());
        ffi::sqlite3_deserialize(
            input.handle(),
            c"main".as_ptr(),
            buffer,
            bytes.len() as i64,
            bytes.len() as i64,
            ffi::SQLITE_DESERIALIZE_FREEONCLOSE | ffi::SQLITE_DESERIALIZE_READONLY,
        )
    };
    if status != ffi::SQLITE_OK {
        return Err(refused("could not open a private SQLite snapshot"));
    }
    let output = safe(
        Connection::open_in_memory(),
        "could not allocate a private SQLite backup",
    )?;
    // SAFETY: distinct live connections are borrowed until the one finish;
    // both are in-memory, and no operations run on either during the backup.
    let backup = unsafe {
        ffi::sqlite3_backup_init(
            output.handle(),
            c"main".as_ptr(),
            input.handle(),
            c"main".as_ptr(),
        )
    };
    if backup.is_null() {
        return Err(refused("could not initialize a private SQLite backup"));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let completed = loop {
        // SAFETY: backup was successfully initialized and is only used here.
        match unsafe { ffi::sqlite3_backup_step(backup, 128) } {
            ffi::SQLITE_DONE => break true,
            ffi::SQLITE_OK if Instant::now() < deadline => (),
            _ => break false,
        }
    };
    // SAFETY: this is the sole finish for the valid, owned backup.
    let finished = unsafe { ffi::sqlite3_backup_finish(backup) };
    if !completed || finished != ffi::SQLITE_OK {
        return Err(refused("private SQLite backup did not complete"));
    }
    safe(
        output.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;"),
        "could not restrict a private SQLite backup",
    )?;
    validate_schema(&output)?;
    Ok(output)
}

fn validate_schema(connection: &Connection) -> Result<()> {
    let row: String = safe(
        connection.query_row("PRAGMA quick_check(1)", [], |row| row.get(0)),
        "could not verify SQLite backup integrity",
    )?;
    if row != "ok" {
        return Err(refused("SQLite backup integrity check failed"));
    }
    let query = || -> rusqlite::Result<bool> {
        let count: i64 = connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='state' AND tbl_name='state'", [], |row| row.get(0))?;
        let others: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE NOT (type='table' AND name='state' AND tbl_name='state') AND NOT (type='index' AND tbl_name='state'))", [], |row| row.get(0))?;
        if count != 1 || others {
            return Ok(false);
        }
        let mut statement = connection.prepare("PRAGMA table_xinfo(state)")?;
        let mut rows = statement.query([])?;
        for (index, name) in ["owner", "kind", "key", "value"].into_iter().enumerate() {
            let Some(row) = rows.next()? else {
                return Ok(false);
            };
            let expected_pk = if index == 3 { 0 } else { index as i64 + 1 };
            if row.get::<_, i64>(0)? != index as i64
                || row.get::<_, String>(1)? != name
                || row.get::<_, String>(2)? != "TEXT"
                || row.get::<_, i64>(3)? != 1
                || row.get::<_, Option<String>>(4)?.is_some()
                || row.get::<_, i64>(5)? != expected_pk
                || row.get::<_, i64>(6)? != 0
            {
                return Ok(false);
            }
        }
        Ok(rows.next()?.is_none())
    };
    if !safe(query(), "invalid source SQLite schema")? {
        return Err(refused("invalid source SQLite schema"));
    }
    Ok(())
}
