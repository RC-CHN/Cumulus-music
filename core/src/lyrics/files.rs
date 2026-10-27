//! Local lyric source boundary. A tag-aware library adapter can implement this
//! trait; the default adapter reads a neighboring .lrc on a blocking worker.
use crate::{Error, Result, protocol::api::ApiFuture};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

pub trait LocalLyrics: Send + Sync {
    fn read<'a>(&'a self, audio: &'a Path) -> ApiFuture<'a, Option<String>>;
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
