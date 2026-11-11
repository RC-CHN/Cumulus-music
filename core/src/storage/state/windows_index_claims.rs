//! Owner-independent Windows exclusion for the optional indexed WAV lane.

use super::{INDEXED_FLAC_DOWNLOAD_CLAIMS, StateStore};
use crate::{Error, Result};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    os::windows::ffi::OsStrExt,
    path::{Component, Path, PathBuf, Prefix},
};
use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

const MAX_ROWS: usize = 8192;
const MAX_PATH_UNITS: usize = 32767;
const MAX_PATH_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct WindowsIndexedDownloadClaims {
    blocked: bool,
    offline_keys: HashSet<String>,
    paths: Vec<(bool, Vec<u16>)>,
}

impl WindowsIndexedDownloadClaims {
    pub(crate) fn excludes(&self, key: &str, path: &Path) -> bool {
        if self.blocked || self.offline_keys.contains(key) {
            return true;
        }
        let Some(path) = drive_units(path) else {
            return true;
        };
        self.paths
            .iter()
            .any(|(directory, claim)| overlaps(&path, claim, *directory))
    }
}

fn drive_units(path: &Path) -> Option<Vec<u16>> {
    let mut components = path.components();
    let Component::Prefix(prefix) = components.next()? else {
        return None;
    };
    let drive = match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive.to_ascii_uppercase(),
        _ => return None,
    };
    if components.next()? != Component::RootDir {
        return None;
    }
    let mut units = vec![u16::from(drive), b':' as u16, b'\\' as u16];
    for component in components {
        let Component::Normal(name) = component else {
            return None;
        };
        let name: Vec<_> = name.encode_wide().collect();
        if name.is_empty()
            || name.last().is_some_and(|last| matches!(*last, 32 | 46))
            || name
                .iter()
                .any(|unit| matches!(*unit, 0..=31 | 34 | 42 | 47 | 58 | 60 | 62 | 63 | 92 | 124))
            || units.len() + name.len() + 1 > MAX_PATH_UNITS
        {
            return None;
        }
        if units.len() > 3 {
            units.push(b'\\' as u16);
        }
        units.extend(name);
    }
    Some(units)
}

fn overlaps(path: &[u16], claim: &[u16], directory: bool) -> bool {
    if path.len() < claim.len()
        || path.len() > claim.len()
            && (!directory
                || claim.last() != Some(&(b'\\' as u16)) && path[claim.len()] != b'\\' as u16)
    {
        return false;
    }
    // SAFETY: Both slices have at least claim.len() initialized UTF-16
    // code units; drive_units bounds their lengths well within i32.
    let order = unsafe {
        CompareStringOrdinal(
            path.as_ptr(),
            claim.len() as i32,
            claim.as_ptr(),
            claim.len() as i32,
            1,
        )
    };
    // If Windows cannot compare, we cannot prove the claim is distinct.
    order == CSTR_EQUAL || order == 0
}

fn canonical_claim(path: &Path, directory: bool) -> Option<Vec<u16>> {
    drive_units(path)?;
    let resolved = match fs::canonicalize(path) {
        Ok(path) => path,
        // Newly admitted direct task targets can be absent, but their
        // already existing parent was fixed before the task was persisted.
        Err(error) if !directory && error.kind() == ErrorKind::NotFound => {
            fs::canonicalize(path.parent()?)
                .ok()?
                .join(path.file_name()?)
        }
        Err(_) => return None,
    };
    drive_units(&resolved)
}

fn capture_path(
    intents: &mut Vec<(bool, PathBuf)>,
    total_bytes: &mut usize,
    value: &Value,
    directory: bool,
) -> bool {
    let Some(path) = value.as_str() else {
        return false;
    };
    if path.is_empty()
        || path.len() > MAX_PATH_UNITS * 4
        || path.contains('\0')
        || intents.len() >= MAX_ROWS * 3
        || *total_bytes + path.len() > MAX_PATH_BYTES
    {
        return false;
    }
    *total_bytes += path.len();
    intents.push((directory, path.into()));
    true
}

impl StateStore {
    pub(crate) fn windows_indexed_download_claims(&self) -> Result<WindowsIndexedDownloadClaims> {
        let mut claims = WindowsIndexedDownloadClaims::default();
        let mut intents = Vec::new();
        let mut total_bytes = 0;
        {
            let connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
            let mut query = connection.prepare_cached(&format!(
                "SELECT kind,key,CASE WHEN length(CAST(value AS BLOB)) <= 262144 THEN value END
                 FROM state WHERE {INDEXED_FLAC_DOWNLOAD_CLAIMS} LIMIT {}",
                MAX_ROWS + 1
            ))?;
            let mut rows = query.query([])?;
            let mut count = 0;
            while let Some(row) = rows.next()? {
                count += 1;
                if count > MAX_ROWS {
                    claims.blocked = true;
                    break;
                }
                let kind: String = row.get(0)?;
                let key: String = row.get(1)?;
                if kind == "download_directory_v1" && !matches!(key.as_str(), "settings" | "active")
                {
                    continue;
                }
                let Ok(value): rusqlite::Result<Option<String>> = row.get(2) else {
                    claims.blocked = true;
                    break;
                };
                let Some(value) = value
                    .as_deref()
                    .and_then(|value| serde_json::from_str::<Value>(value).ok())
                    .filter(Value::is_object)
                else {
                    claims.blocked = true;
                    break;
                };
                let mut capture = |value, directory| {
                    capture_path(&mut intents, &mut total_bytes, value, directory)
                };
                let understood = match (kind.as_str(), key.as_str()) {
                    ("offline_music_v1", _) => {
                        claims.offline_keys.insert(key.clone());
                        capture(&value["path"], false)
                    }
                    ("download_directory_v1", "settings") => capture(&value["root"], true),
                    ("download_directory_v1", "active") => false,
                    ("download_file_operations_v1", _) => {
                        matches!(value["phase"].as_str(), Some("complete" | "cancelled"))
                    }
                    ("download_tasks_v1", _) => {
                        let destination = &value["destination"];
                        (value["target"].is_null() || capture(&value["target"], false))
                            && match destination["kind"].as_str() {
                                Some("path") => capture(&destination["path"], false),
                                Some("named") => {
                                    capture(&value["root"], true)
                                        && capture(&destination["root"], true)
                                }
                                _ => false,
                            }
                    }
                    _ => false,
                };
                if !understood {
                    claims.blocked = true;
                    break;
                }
            }
        }
        if !claims.blocked {
            // Drop the SQLite lock before asking the filesystem. Compare both
            // recorded and physical path spelling; an unresolvable alias
            // cannot establish that an audio file is outside managed state.
            let mut seen = HashSet::new();
            let mut remaining_units = MAX_PATH_BYTES / size_of::<u16>();
            for (directory, path) in intents {
                if !seen.insert((directory, path.clone())) {
                    continue;
                }
                let (Some(recorded), Some(canonical)) =
                    (drive_units(&path), canonical_claim(&path, directory))
                else {
                    claims.blocked = true;
                    break;
                };
                let distinct = recorded != canonical;
                let required = canonical.len() + if distinct { recorded.len() } else { 0 };
                let Some(remaining) = remaining_units.checked_sub(required) else {
                    claims.blocked = true;
                    break;
                };
                remaining_units = remaining;
                if distinct {
                    claims.paths.push((directory, recorded));
                }
                claims.paths.push((directory, canonical));
            }
        }
        Ok(claims)
    }

    pub(crate) fn download_claims_indexed_windows_path(
        &self,
        key: &str,
        path: &str,
    ) -> Result<bool> {
        Ok(self
            .windows_indexed_download_claims()?
            .excludes(key, Path::new(path)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(value: &str) -> Vec<u16> {
        drive_units(Path::new(value)).unwrap()
    }

    #[test]
    fn windows_claim_paths_are_case_and_drive_prefix_alias_aware_without_matching_siblings() {
        let root = path(r"\\?\C:\Música");
        let child = path(r"c:\mÚSICA\目录\Audio.wav");
        let same_file = path(r"\\?\C:\MÚSICA\目录\AUDIO.WAV");
        assert!(overlaps(&child, &root, true));
        assert!(overlaps(&child, &same_file, false));
        assert!(overlaps(&child, &path(r"c:\"), true));
        assert!(!overlaps(&child, &root, false));
        assert!(!overlaps(&path(r"c:\Música2\Audio.wav"), &root, true));
        assert!(!overlaps(&path(r"d:\Música\Audio.wav"), &root, true));
        for unsupported in [
            r"c:audio.wav",
            r"\\host\share\Audio.wav",
            r"c:\Música\Audio.wav:extra",
            r"c:\Música\Alias.\Audio.wav",
        ] {
            assert!(
                drive_units(Path::new(unsupported)).is_none(),
                "{unsupported}"
            );
        }
    }
}
