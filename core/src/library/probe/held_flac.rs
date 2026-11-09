use super::{Ffprobe, parse_ffprobe_response, process};
use crate::{Error, Result, library::AudioMetadata};
use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

impl Ffprobe {
    /// Opt-in metadata probe for a previously checked, read-only FLAC file.
    /// The label is a single filename for the existing title/extension fallback;
    /// it is never passed to the subprocess or opened. This does not validate
    /// the caller's path or authorize a file for the local index or playback.
    ///
    /// The child receives a duplicate of the same file as standard input and
    /// shares its cursor. The caller must not use the file through any other
    /// handle sharing that cursor until the probe returns. The original offset
    /// is restored after the child has exited, including on probe failure.
    /// Missing fd support and other probe failures never retry a pathname.
    pub fn inspect_held_flac(
        &self,
        file: &mut File,
        logical_filename: &OsStr,
    ) -> Result<AudioMetadata> {
        let name = Path::new(logical_filename);
        if name.file_name() != Some(logical_filename)
            || !name
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("flac"))
        {
            return Err(Error::InvalidInput("held FLAC requires a logical filename"));
        }
        let executable = self
            .executable
            .as_deref()
            .ok_or(Error::State("audio metadata probe unavailable"))?;
        if !file.metadata()?.is_file() {
            return Err(Error::Media("held FLAC is not a regular file"));
        }
        let offset = file.stream_position()?;
        let probe = (|| {
            file.rewind()?;
            let mut marker = [0; 4];
            file.read_exact(&mut marker)?;
            if &marker != b"fLaC" {
                return Err(Error::Media("held audio has no plain FLAC marker"));
            }
            file.rewind()?;
            let value = process::run(
                executable,
                process::Input::HeldFlac(file.try_clone()?),
                self.timeout,
            )?;
            let metadata = parse_ffprobe_response(name, &value)?;
            if metadata.format.as_deref() != Some("flac") {
                return Err(Error::Media("held audio was not probed as FLAC"));
            }
            Ok(metadata)
        })();
        file.seek(SeekFrom::Start(offset))?;
        probe
    }
}
