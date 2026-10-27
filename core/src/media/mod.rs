//! Local media codecs and bounded cover validation; no account or network I/O.
mod cover;
pub mod integrity;
pub mod ncm;
pub mod tags;
pub use cover::{CoverInfo, validate_cover};

pub(crate) fn open_regular(path: impl AsRef<std::path::Path>) -> crate::Result<std::fs::File> {
    let path = path.as_ref();
    if !std::fs::metadata(path)?.is_file() {
        return Err(crate::Error::InvalidInput(
            "media source must be a regular file",
        ));
    }
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(crate::Error::InvalidInput(
            "media source must be a regular file",
        ));
    }
    Ok(file)
}
