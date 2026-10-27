use crate::{Error, Result};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

// M/3127 selectRootFileOrDir -> listFilesByEntries(recursive=true).
// Enumeration only; create() subsequently probes and hashes each selected file.
pub fn select_audio_sources(roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    if roots.len() > 500 {
        return Err(Error::InvalidInput("too many cloud selection roots"));
    }
    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::new();
    let mut inspected = 0;
    for root in roots {
        visit(root, 0, &mut inspected, &mut files, &mut directories)?;
    }
    Ok(files.into_iter().collect())
}
fn visit(
    path: &Path,
    depth: usize,
    inspected: &mut usize,
    files: &mut BTreeSet<PathBuf>,
    directories: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    *inspected += 1;
    if depth > 64 || *inspected > 100_000 {
        return Err(Error::InvalidInput(
            "cloud directory selection exceeds traversal limit",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    // Skip links and nonregular entries: do not follow cycles or silently scan
    // outside the chosen directory. Direct file selection remains available.
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        if !directories.insert(path.canonicalize()?) {
            return Ok(());
        }
        let mut children = fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        children.sort();
        for child in children {
            visit(&child, depth + 1, inspected, files, directories)?;
        }
    } else if metadata.is_file()
        && path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
            matches!(
                s.to_ascii_lowercase().as_str(),
                "mp3" | "flac" | "wav" | "m4a" | "aac" | "ogg" | "opus" | "aiff" | "ape" | "wma"
            )
        })
    {
        let path = path.canonicalize()?;
        if path.to_str().is_none() {
            return Err(Error::InvalidInput("selected cloud path must be UTF8"));
        }
        files.insert(path);
        if files.len() > 500 {
            return Err(Error::InvalidInput("cloud selection exceeds 500 files"));
        }
    }
    Ok(())
}
