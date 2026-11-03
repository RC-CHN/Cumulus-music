//! Local library lyric lookup and the separate published download boundary.
use crate::{Error, Result, protocol::api::ApiFuture};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalLyricSource {
    Sidecar,
    Embedded,
}

pub trait LocalLyrics: Send + Sync {
    fn read<'a>(&'a self, audio: &'a Path) -> ApiFuture<'a, Option<String>>;

    fn read_local<'a>(
        &'a self,
        audio: &'a Path,
    ) -> ApiFuture<'a, Option<(String, LocalLyricSource)>> {
        Box::pin(async move {
            Ok(self
                .read(audio)
                .await?
                .map(|text| (text, LocalLyricSource::Sidecar)))
        })
    }
}

pub struct SidecarLyrics;
const LIMIT: usize = 4 * 1024 * 1024;

impl LocalLyrics for SidecarLyrics {
    fn read<'a>(&'a self, audio: &'a Path) -> ApiFuture<'a, Option<String>> {
        Box::pin(async move {
            let path = audio.with_extension("lrc");
            tokio::task::spawn_blocking(move || read_sidecar(path))
                .await
                .map_err(|_| Error::State("lyric file worker stopped"))?
        })
    }
}

/// Read embedded metadata only for a local-library selection. The ordinary
/// sidecar method remains available to the separately guarded download path.
pub(crate) struct LibraryLyrics;

impl LocalLyrics for LibraryLyrics {
    fn read<'a>(&'a self, audio: &'a Path) -> ApiFuture<'a, Option<String>> {
        SidecarLyrics.read(audio)
    }

    fn read_local<'a>(
        &'a self,
        audio: &'a Path,
    ) -> ApiFuture<'a, Option<(String, LocalLyricSource)>> {
        Box::pin(async move {
            if let Some(text) = self
                .read(audio)
                .await?
                .filter(|text| !text.trim().is_empty())
            {
                return Ok(Some((text, LocalLyricSource::Sidecar)));
            }
            let audio = audio.to_owned();
            tokio::task::spawn_blocking(move || {
                super::embedded::read(&audio, LIMIT).map(|text| (text, LocalLyricSource::Embedded))
            })
            .await
            .map_err(|_| Error::State("embedded lyric worker stopped"))
        })
    }
}

fn read_sidecar(path: PathBuf) -> Result<Option<String>> {
    match std::fs::metadata(&path) {
        Ok(meta) if !meta.is_file() => {
            return Err(Error::InvalidInput("lyric sidecar must be a regular file"));
        }
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !file.metadata()?.is_file() {
        return Err(Error::InvalidInput("lyric sidecar must be a regular file"));
    }
    decode_sidecar(file)
}

/// The downloaded fallback is admitted by its published task and must not
/// open a redirected or special leaf in a user-chosen download directory.
pub(crate) fn read_published_sidecar(audio: &Path) -> Result<Option<String>> {
    let path = audio.with_extension("lrc");
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: inspect the opened leaf, not its target.
        options.custom_flags(0x0020_0000);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Ok(None);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT.
        if metadata.file_attributes() & 0x0400 != 0 {
            return Ok(None);
        }
    }
    decode_sidecar(file)
}

fn decode_sidecar(file: File) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err(Error::InvalidInput("lyric file exceeds size limit"));
    }
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            return Err(Error::InvalidInput("truncated UTF-16 lyric"));
        }
        let little = bytes[0] == 0xff;
        let units: Vec<_> = bytes[2..]
            .chunks_exact(2)
            .map(|v| {
                if little {
                    u16::from_le_bytes([v[0], v[1]])
                } else {
                    u16::from_be_bytes([v[0], v[1]])
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| Error::InvalidInput("invalid UTF-16 lyric"))?
    } else {
        let data = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
        String::from_utf8(data.to_vec())
            .map_err(|_| Error::InvalidInput("lyric text requires UTF-8 or UTF-16 BOM"))?
    };
    Ok(Some(text))
}
