//! Windows PC localmusic/lyric path order for an already selected library file.
//! The separately authorized published download lookup does not use this module.
use super::{decode_text, read_sidecar_bytes};
use crate::{Error, Result};
use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io,
    path::Path,
};

pub(super) fn read(audio: &Path) -> Result<Option<String>> {
    let name = audio
        .file_name()
        .ok_or(Error::InvalidInput("local lyric source has no file name"))?;
    let parent = audio
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let Some(root) = Folder::root(parent)? else {
        return Ok(None);
    };
    if let Some(text) = root.read(name)? {
        return Ok(Some(text));
    }
    let Some(lyric) = root.directory("lyric")? else {
        return Ok(None);
    };
    if let Some(text) = lyric.read(name)? {
        return Ok(Some(text));
    }
    match lyric.directory("lrc")? {
        Some(lrc) => lrc.read(name),
        None => Ok(None),
    }
}

impl Folder {
    fn read(&self, audio_name: &OsStr) -> Result<Option<String>> {
        for extension in ["lrc", "krc"] {
            let name = Path::new(audio_name).with_extension(extension);
            let Some(file) = self.open(&name, false)? else {
                continue;
            };
            let bytes = read_sidecar_bytes(file)?;
            let text = if extension == "krc" {
                // The official local route serves text; this adapter does not
                // decode packed KRC or introduce another timestamp grammar.
                decode_text(&bytes)
                    .ok()
                    .filter(|s| {
                        !s.chars()
                            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                    })
                    .ok_or(Error::InvalidInput("binary KRC lyrics are unsupported"))?
            } else {
                decode_text(&bytes)?
            };
            return Ok(Some(text));
        }
        Ok(None)
    }
}

fn missing<T>(result: io::Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn check_kind(file: File, directory: bool) -> Result<File> {
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Error::InvalidInput("redirected local lyric path"));
        }
    }
    if (directory && metadata.is_dir()) || (!directory && metadata.is_file()) {
        Ok(file)
    } else {
        Err(Error::InvalidInput("invalid local lyric file or folder"))
    }
}

#[cfg(unix)]
struct Folder(File);

#[cfg(unix)]
impl Folder {
    fn root(path: &Path) -> Result<Option<Self>> {
        use std::os::unix::fs::OpenOptionsExt;
        missing(
            OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | DIRECTORY_ACCESS)
                .open(path),
        )
        .map(|file| file.map(Self))
    }

    fn directory(&self, name: &str) -> Result<Option<Self>> {
        self.open(Path::new(name), true).map(|file| file.map(Self))
    }

    fn open(&self, name: &Path, directory: bool) -> Result<Option<File>> {
        use std::{
            ffi::CString,
            os::{
                fd::{AsRawFd, FromRawFd},
                unix::ffi::OsStrExt,
            },
        };
        let name = CString::new(name.as_os_str().as_bytes())
            .map_err(|_| Error::InvalidInput("invalid local lyric file name"))?;
        let access = if directory {
            libc::O_DIRECTORY | DIRECTORY_ACCESS
        } else {
            libc::O_RDONLY | libc::O_NONBLOCK
        };
        // SAFETY: name is one derived child component with a NUL terminator,
        // and the directory fd remains live for the entire call. No creation.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                access | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return missing(Err(io::Error::last_os_error()));
        }
        // SAFETY: this successful openat returned a new, uniquely owned fd.
        check_kind(unsafe { File::from_raw_fd(fd) }, directory).map(Some)
    }
}

#[cfg(target_os = "linux")]
const DIRECTORY_ACCESS: libc::c_int = libc::O_PATH;
#[cfg(all(unix, not(target_os = "linux")))]
const DIRECTORY_ACCESS: libc::c_int = libc::O_RDONLY;

#[cfg(windows)]
struct Folder {
    path: std::path::PathBuf,
    _directory: File,
}

#[cfg(windows)]
impl Folder {
    fn root(path: &Path) -> Result<Option<Self>> {
        match missing(path.canonicalize())? {
            Some(path) => Self::at(path),
            None => Ok(None),
        }
    }

    fn at(path: std::path::PathBuf) -> Result<Option<Self>> {
        open_windows(&path, true).map(|file| file.map(|_directory| Self { path, _directory }))
    }

    fn directory(&self, name: &str) -> Result<Option<Self>> {
        Self::at(self.path.join(name))
    }

    fn open(&self, name: &Path, directory: bool) -> Result<Option<File>> {
        open_windows(&self.path.join(name), directory)
    }
}

#[cfg(windows)]
fn open_windows(path: &Path, directory: bool) -> Result<Option<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    if directory {
        // The parent handles remain open during lookup and omit DELETE share
        // so the checked directories cannot themselves be swapped for links.
        options
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(0x0000_0003) // FILE_SHARE_READ | FILE_SHARE_WRITE
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    }
    missing(options.open(path))?
        .map(|file| check_kind(file, directory))
        .transpose()
}
