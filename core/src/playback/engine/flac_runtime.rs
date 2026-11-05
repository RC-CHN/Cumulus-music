//! Installation identity for the optional held-source FLAC decoder.
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

const MAX_LIBRARY_BYTES: u64 = 8 * 1024 * 1024;

/// Native-only runtime configuration. The host supplies the staged path and
/// the digest recorded when it built its package; a track cannot supply either.
pub struct TrustedFlacRuntime {
    path: PathBuf,
    sha256: [u8; 32],
}

impl TrustedFlacRuntime {
    pub fn new(path: PathBuf, sha256: &str) -> Result<Self> {
        let mut digest = [0; 32];
        if !path.is_absolute()
            || sha256.len() != 64
            || !sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || hex::decode_to_slice(sha256, &mut digest).is_err()
        {
            return Err(Error::InvalidInput("invalid packaged FLAC identity"));
        }
        Ok(Self {
            path,
            sha256: digest,
        })
    }

    /// Verify again at use time. This verifies the installed bytes before
    /// opening the library by its absolute path; it is not an OS code-signing
    /// boundary against a writer with access to the installation directory.
    pub(super) fn verified_path(&self) -> Result<&Path> {
        let meta = self.path.symlink_metadata()?;
        if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_LIBRARY_BYTES {
            return Err(Error::Media("invalid packaged FLAC library"));
        }
        let mut file = File::open(&self.path)?;
        let mut hasher = Sha256::new();
        let bytes = io::copy(
            &mut Read::by_ref(&mut file).take(MAX_LIBRARY_BYTES + 1),
            &mut hasher,
        )?;
        let actual: [u8; 32] = hasher.finalize().into();
        if bytes != meta.len() || actual != self.sha256 {
            return Err(Error::Media("packaged FLAC library identity changed"));
        }
        Ok(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_identity_rechecks_installed_bytes_and_bounds_reads() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native-flac");
        let original = b"synthetic decoder bytes";
        let sha = hex::encode(Sha256::digest(original));
        let runtime = TrustedFlacRuntime::new(path.clone(), &sha).unwrap();
        assert!(runtime.verified_path().is_err());
        std::fs::write(&path, original).unwrap();
        assert_eq!(runtime.verified_path().unwrap(), path);
        std::fs::write(&path, b"different decoder file").unwrap();
        assert!(runtime.verified_path().is_err());
        File::create(&path)
            .unwrap()
            .set_len(MAX_LIBRARY_BYTES + 1)
            .unwrap();
        assert!(runtime.verified_path().is_err());
        assert!(TrustedFlacRuntime::new("native-flac".into(), &sha).is_err());
        assert!(TrustedFlacRuntime::new(path, &sha.to_uppercase()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn runtime_does_not_follow_a_redirected_library() {
        let directory = tempfile::tempdir().unwrap();
        let real = directory.path().join("native-flac");
        let link = directory.path().join("redirected");
        std::fs::write(&real, b"same bytes").unwrap();
        std::os::unix::fs::symlink(real, &link).unwrap();
        let runtime =
            TrustedFlacRuntime::new(link, &hex::encode(Sha256::digest(b"same bytes"))).unwrap();
        assert!(runtime.verified_path().is_err());
    }
}
