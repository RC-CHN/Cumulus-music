use super::{LocalLibrary, OWNER};
use crate::{
    Error, Result,
    library::{LocalResource, LocalRevision, LocalSegment, LocalSource, identity::fingerprint},
    media::ncm::{AudioFormat, HeaderPolicy, NcmReader, extract},
    storage::OperationLock,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Decoded {
    audio_sha256: String,
    audio_bytes: u64,
    format: AudioFormat,
}
const KIND: &str = "decoded_ncm_v1";

fn digest(mut reader: impl Read) -> Result<(u64, String)> {
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut block = [0; 65536];
    loop {
        let n = reader.read(&mut block)?;
        if n == 0 {
            break;
        }
        hash.update(&block[..n]);
        bytes += n as u64;
    }
    Ok((bytes, hex::encode(hash.finalize())))
}
impl LocalLibrary {
    pub fn playable_path(&self, path: impl AsRef<Path>) -> Result<PathBuf> {
        let path = path.as_ref().canonicalize()?;
        let before = fingerprint(&path)?;
        let file = crate::media::open_regular(&path)?;
        if path
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("cue"))
        {
            return Err(Error::InvalidInput(
                "import the CUE and select a virtual track",
            ));
        }
        if !path
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("ncm"))
        {
            return Ok(path);
        }
        let (_, source_sha) = digest(file)?;
        let _lock = OperationLock::acquire(
            self.store.directory(),
            OWNER,
            &format!("decoded:{source_sha}"),
        )?;
        let reader = NcmReader::open(&path, HeaderPolicy::Compatible)?;
        let format = reader
            .info()
            .audio_format
            .ok_or(Error::Media("unrecognized decoded NCM audio"))?;
        let suffix = match format {
            AudioFormat::Flac => "flac",
            AudioFormat::Mp3 => "mp3",
            AudioFormat::M4a => "m4a",
        };
        let output = self
            .state_root
            .join("decoded-ncm")
            .join(format!("{source_sha}.{suffix}"));
        let saved: Option<Decoded> = self
            .store
            .get(OWNER, KIND, &source_sha)?
            .map(serde_json::from_value)
            .transpose()?;
        let exists = match fs::symlink_metadata(&output) {
            Ok(meta) if meta.is_file() => true,
            Ok(_) => return Err(Error::Media("decoded cache path is not a regular file")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if exists {
            let expected = match saved {
                Some(record) => {
                    if record.format != format || record.audio_bytes != reader.info().audio_bytes {
                        return Err(Error::Media("decoded cache record disagrees with source"));
                    }
                    (record.audio_bytes, record.audio_sha256)
                }
                // A process can die after publication but before the manifest.
                // Recompute from the NCM before adopting that orphaned file.
                None => digest(reader)?,
            };
            if digest(crate::media::open_regular(&output)?)? == expected {
                if before != fingerprint(&path)? {
                    return Err(Error::State("NCM source changed during cache validation"));
                }
                self.store.put(
                    OWNER,
                    KIND,
                    &source_sha,
                    &serde_json::to_value(Decoded {
                        audio_bytes: expected.0,
                        audio_sha256: expected.1,
                        format,
                    })?,
                )?;
                return Ok(output);
            }
            // This name is generated inside the private managed cache. Never
            // repair/overwrite a user's original NCM or arbitrary output path.
            fs::remove_file(&output)?;
        }
        let decoded = extract(&path, &output, HeaderPolicy::Compatible)?;
        if before != fingerprint(&path)? {
            return Err(Error::State("NCM source changed during extraction"));
        }
        self.store.put(
            OWNER,
            KIND,
            &source_sha,
            &serde_json::to_value(Decoded {
                audio_bytes: decoded.info.audio_bytes,
                audio_sha256: decoded.audio_sha256,
                format,
            })?,
        )?;
        Ok(output)
    }

    pub fn resolve(&self, resource: &LocalResource) -> Result<LocalSource> {
        let item = self.item(&resource.local_id)?;
        Self::check_current(&item)?;
        if item.revision != resource.local_revision {
            return Err(Error::State(
                "queued local track belongs to an earlier scan",
            ));
        }
        let source = self.playable_path(&item.path)?;
        Self::check_current(&item)?;
        let segment = match item.revision {
            LocalRevision::File { .. } => None,
            LocalRevision::Cue {
                offset, duration, ..
            } => Some(LocalSegment {
                start: offset,
                duration,
            }),
        };
        Ok(LocalSource {
            source,
            metadata: item.metadata,
            segment,
        })
    }
}
