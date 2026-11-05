//! Private subset of RIFF/WAVE for explicitly opened native stereo PCM.
use crate::{Error, Result};
use std::io::{Read, Seek, SeekFrom};

pub(crate) const RATE: u32 = 44_100;
pub(crate) const BYTES_PER_FRAME: usize = 4;
pub(crate) const BYTE_RATE: u32 = RATE * BYTES_PER_FRAME as u32;
const MAX_FRAMES: u64 = RATE as u64 * 3600;
const MAX_FILE_BYTES: u64 = MAX_FRAMES * BYTES_PER_FRAME as u64 + 1024 * 1024;
const MAX_CHUNKS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PcmWave {
    data_start: u64,
    pub(crate) frames: u64,
}

fn invalid() -> Error {
    Error::InvalidInput("unsupported native PCM WAV layout")
}

impl PcmWave {
    /// Accept only one ordinary signed-16 stereo stream. In particular, the
    /// stock demuxer must not see alternate audio in LIST/wavl or another data
    /// chunk when we yield this same handle to an independent filter.
    pub(crate) fn inspect(reader: &mut (impl Read + Seek)) -> Result<Self> {
        let length = reader.seek(SeekFrom::End(0))?;
        if !(44..=MAX_FILE_BYTES).contains(&length) {
            return Err(invalid());
        }
        reader.rewind()?;
        let mut header = [0; 12];
        reader.read_exact(&mut header)?;
        if &header[..4] != b"RIFF"
            || &header[8..] != b"WAVE"
            || u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap())) + 8 != length
        {
            return Err(invalid());
        }
        let mut cursor = 12;
        let mut have_format = false;
        let mut audio = None;
        for _ in 0..MAX_CHUNKS {
            if cursor == length {
                break;
            }
            if length - cursor < 8 {
                return Err(invalid());
            }
            reader.seek(SeekFrom::Start(cursor))?;
            let mut chunk = [0; 8];
            reader.read_exact(&mut chunk)?;
            let size = u64::from(u32::from_le_bytes(chunk[4..].try_into().unwrap()));
            let data_start = cursor + 8;
            cursor = data_start + size + size % 2;
            if cursor > length {
                return Err(invalid());
            }
            match &chunk[..4] {
                b"fmt " if !have_format && matches!(size, 16 | 18) => {
                    let mut fmt = [0; 18];
                    reader.read_exact(&mut fmt[..size as usize])?;
                    if fmt[..4] != [1, 0, 2, 0]
                        || fmt[4..8] != RATE.to_le_bytes()
                        || fmt[8..12] != BYTE_RATE.to_le_bytes()
                        || fmt[12..] != [4, 0, 16, 0, 0, 0]
                    {
                        return Err(invalid());
                    }
                    have_format = true;
                }
                b"data" if have_format && audio.is_none() => {
                    let frames = size / BYTES_PER_FRAME as u64;
                    if !size.is_multiple_of(BYTES_PER_FRAME as u64)
                        || !(1..=MAX_FRAMES).contains(&frames)
                    {
                        return Err(invalid());
                    }
                    audio = Some(Self { data_start, frames });
                }
                b"JUNK" | b"PAD " => (),
                b"LIST" if (4..=65_536).contains(&size) => {
                    let mut kind = [0; 4];
                    reader.read_exact(&mut kind)?;
                    if &kind != b"INFO" {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
        }
        if cursor != length {
            return Err(invalid());
        }
        reader.rewind()?;
        audio.ok_or_else(invalid)
    }

    pub(super) fn byte_offset(self, frame: u64) -> Result<u64> {
        if frame >= self.frames {
            return Err(Error::InvalidInput("PCM WAV seek outside source"));
        }
        // The inspected source fits an ordinary 32-bit RIFF extent.
        Ok(self.data_start + frame * BYTES_PER_FRAME as u64)
    }
}

#[cfg(test)]
pub(crate) mod tests;
