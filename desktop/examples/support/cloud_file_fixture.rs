//! Bind generated proof files using the same canonical path namespace on each OS.
use cumulus_core::{Error, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn original_path(directory: &Path, record: &Path, target: Option<&Path>) -> Result<PathBuf> {
    let boundary = directory.canonicalize()?;
    let saved = record.try_exists()?;
    let path: PathBuf = if saved {
        serde_json::from_slice(&fs::read(record)?)?
    } else {
        target
            .ok_or(Error::State("seeded synthetic original is missing"))?
            .canonicalize()?
    };
    if !path.starts_with(&boundary) {
        return Err(Error::InvalidInput(
            "proof original is outside its generated media directory",
        ));
    }
    if path.try_exists()? && path.canonicalize()? != path {
        return Err(Error::InvalidInput("proof original path was replaced"));
    }
    if !saved {
        fs::write(record, serde_json::to_vec(&path)?)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_native_target_is_bound_to_normal_root_and_survives_file_deletion() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("generated");
        fs::create_dir(&directory).unwrap();
        let file = directory.join("original.flac");
        fs::write(&file, b"synthetic").unwrap();
        let target = file.canonicalize().unwrap();
        let record = temp.path().join("record.json");
        let original = original_path(&directory, &record, Some(&target)).unwrap();
        assert_eq!(original, target);
        assert!(original.starts_with(directory.canonicalize().unwrap()));
        fs::remove_file(&file).unwrap();
        assert_eq!(original_path(&directory, &record, None).unwrap(), original);
    }
    #[test]
    fn missing_or_escaping_original_never_creates_a_record() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("generated");
        fs::create_dir(&directory).unwrap();
        let outside = temp.path().join("outside.flac");
        fs::write(&outside, b"untouched").unwrap();
        let record = temp.path().join("record.json");
        assert!(original_path(&directory, &record, None).is_err());
        assert!(original_path(&directory, &record, Some(&outside)).is_err());
        assert!(!record.exists());
        assert_eq!(fs::read(outside).unwrap(), b"untouched");
    }
    #[cfg(unix)]
    #[test]
    fn recorded_file_replaced_with_a_symlink_is_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("generated");
        fs::create_dir(&directory).unwrap();
        let file = directory.join("original.flac");
        fs::write(&file, b"synthetic").unwrap();
        let record = temp.path().join("record.json");
        original_path(&directory, &record, Some(&file)).unwrap();
        let outside = temp.path().join("outside.flac");
        fs::write(&outside, b"untouched").unwrap();
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(&outside, &file).unwrap();
        assert!(original_path(&directory, &record, None).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"untouched");
    }
}
