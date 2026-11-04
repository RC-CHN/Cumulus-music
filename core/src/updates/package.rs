use super::{Asset, Version};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::Read,
    path::Path,
};

const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const FORMATS: [PackageFormat; 4] = [
    PackageFormat::LinuxDeb,
    PackageFormat::LinuxPortable,
    PackageFormat::WindowsZip,
    PackageFormat::WindowsSetup,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageFormat {
    LinuxDeb,
    LinuxPortable,
    WindowsZip,
    WindowsSetup,
}

impl PackageFormat {
    fn name(self, version: Version) -> String {
        let suffix = match self {
            Self::LinuxDeb => "linux-x64.deb",
            Self::LinuxPortable => "linux-x64.tar.gz",
            Self::WindowsZip => "windows-x64.zip",
            Self::WindowsSetup => "windows-x64-setup.exe",
        };
        format!("Cumulus-{version}-{suffix}")
    }
    fn magic(self) -> &'static [u8] {
        match self {
            Self::LinuxDeb => b"!<arch>\n",
            Self::LinuxPortable => b"\x1f\x8b\x08",
            Self::WindowsZip => b"PK\x03\x04",
            Self::WindowsSetup => b"MZ",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageFailure {
    NoMetadata,
    InvalidFile,
    WrongName,
    WrongSize,
    WrongFormat,
    DigestMismatch,
    Changed,
    TooLarge,
    ReadFailed,
    Busy,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageVerification {
    version: String,
    filename: String,
    format: PackageFormat,
}

struct Package {
    name: String,
    format: PackageFormat,
    size: u64,
    digest: [u8; 32],
}

/// Constructible only from the validated public release response. No package,
/// path, digest or artifact URL is accepted from the renderer.
pub struct PublishedPackages {
    version: Version,
    entries: Vec<Package>,
}

impl PublishedPackages {
    pub(super) fn from_assets(assets: &[Asset], version: Version) -> Option<Self> {
        if assets.len() > 64 {
            return None;
        }
        let entries = FORMATS
            .into_iter()
            .map(|format| {
                let name = format.name(version);
                let (size, digest) = asset(assets, &name)?;
                Some(Package {
                    name,
                    format,
                    size,
                    digest,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        for name in [
            "manifest-linux-x64.json",
            "manifest-windows-x64.json",
            "SHA256SUMS",
            "release-provenance.json",
        ] {
            asset(assets, name)?;
        }
        Some(Self { version, entries })
    }

    /// Hash only the selected regular file using metadata from this same
    /// release. Do not extract, execute, upload, or return its full path.
    pub fn verify(&self, path: &Path) -> Result<PackageVerification, PackageFailure> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(PackageFailure::WrongName)?;
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or(PackageFailure::WrongName)?;
        if entry.size > MAX_PACKAGE_BYTES {
            return Err(PackageFailure::TooLarge);
        }
        if !platform_path(path) {
            return Err(PackageFailure::InvalidFile);
        }
        let before = selected_metadata(path)?;
        if before.len() != entry.size {
            return Err(PackageFailure::WrongSize);
        }
        let mut file = open(path).map_err(|_| PackageFailure::ReadFailed)?;
        let opened = file.metadata().map_err(|_| PackageFailure::ReadFailed)?;
        if !regular(&opened) || !unchanged(&before, &opened) {
            return Err(PackageFailure::Changed);
        }

        let magic = entry.format.magic();
        let mut prefix = [0_u8; 8];
        if entry.size < magic.len() as u64 {
            return Err(PackageFailure::WrongFormat);
        }
        file.read_exact(&mut prefix[..magic.len()])
            .map_err(|_| PackageFailure::ReadFailed)?;
        if &prefix[..magic.len()] != magic {
            return Err(PackageFailure::WrongFormat);
        }
        let mut hasher = Sha256::new();
        hasher.update(magic);
        let remaining = entry.size - magic.len() as u64;
        // A file that grows while reading cannot prolong the hash past the
        // expected bytes plus one, or yield a digest for only the prefix.
        let mut bounded = (&mut file).take(remaining + 1);
        let mut buffer = vec![0; 128 * 1024];
        let mut read = 0;
        loop {
            let count = bounded
                .read(&mut buffer)
                .map_err(|_| PackageFailure::ReadFailed)?;
            if count == 0 {
                break;
            }
            read += count as u64;
            hasher.update(&buffer[..count]);
        }
        let after = file.metadata().map_err(|_| PackageFailure::ReadFailed)?;
        if read != remaining
            || !unchanged(&opened, &after)
            || !unchanged(
                &opened,
                &selected_metadata(path).map_err(|_| PackageFailure::Changed)?,
            )
        {
            return Err(PackageFailure::Changed);
        }
        let digest: [u8; 32] = hasher.finalize().into();
        if digest != entry.digest {
            return Err(PackageFailure::DigestMismatch);
        }
        Ok(PackageVerification {
            version: self.version.to_string(),
            filename: entry.name.clone(),
            format: entry.format,
        })
    }
}

fn asset(rows: &[Asset], name: &str) -> Option<(u64, [u8; 32])> {
    let mut matching = rows.iter().filter(|asset| asset.name == name);
    let row = matching.next()?;
    if matching.next().is_some() || row.state != "uploaded" || row.size == 0 {
        return None;
    }
    let digest = row.digest.as_deref()?.strip_prefix("sha256:")?;
    let mut bytes = [0; 32];
    hex::decode_to_slice(digest, &mut bytes).ok()?;
    Some((row.size, bytes))
}

fn selected_metadata(path: &Path) -> Result<Metadata, PackageFailure> {
    let value = fs::symlink_metadata(path).map_err(|_| PackageFailure::ReadFailed)?;
    if regular(&value) {
        Ok(value)
    } else {
        Err(PackageFailure::InvalidFile)
    }
}

fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    before.len() == after.len()
        && before
            .modified()
            .is_ok_and(|time| after.modified().is_ok_and(|new| new == time))
        && same_file(before, after)
}

#[cfg(unix)]
fn same_file(before: &Metadata, after: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

#[cfg(windows)]
fn same_file(before: &Metadata, after: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    before.creation_time() == after.creation_time()
        && before.last_write_time() == after.last_write_time()
        && before.file_attributes() == after.file_attributes()
}

#[cfg(not(any(unix, windows)))]
fn same_file(_: &Metadata, _: &Metadata) -> bool {
    false
}

fn platform_path(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        matches!(path.components().next(), Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
            && path.is_absolute()
    }
    #[cfg(not(windows))]
    {
        path.is_absolute()
    }
}

fn regular(value: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Reject reparse, offline and recall-on-open/data-access placeholders.
        const DISALLOWED: u32 = 0x0000_0400 | 0x0000_1000 | 0x0004_0000 | 0x0040_0000;
        value.is_file() && value.file_attributes() & DISALLOWED == 0
    }
    #[cfg(not(windows))]
    {
        value.is_file()
    }
}

fn open(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // OPEN_REPARSE_POINT | OPEN_NO_RECALL; readers only while hashing.
        options
            .custom_flags(0x0020_0000 | 0x0010_0000)
            .share_mode(1);
    }
    options.open(path)
}
