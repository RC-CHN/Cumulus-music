//! A bounded MPEG Layer III metadata and leading-decoder check on a held file.
use super::{Ffprobe, parse_ffprobe_response, process};
use crate::{Error, Result, library::AudioMetadata};
use serde_json::Value;
use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TAG_BYTES: u64 = 16 * 1024 * 1024;

impl Ffprobe {
    /// Probe a held, already authorized MP3, never a filesystem path. The
    /// filename is a display label and does not cross the child boundary.
    /// This validates only the bounded leading audio, not the entire file.
    /// No other reader may use the shared file cursor until this returns.
    pub fn inspect_held_mp3(
        &self,
        file: &mut File,
        logical_filename: &OsStr,
    ) -> Result<AudioMetadata> {
        let name = Path::new(logical_filename);
        if name.file_name() != Some(logical_filename)
            || !name
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"))
        {
            return Err(Error::InvalidInput("held MP3 requires a logical filename"));
        }
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            return Err(Error::Media("held MP3 exceeds regular file bounds"));
        }
        let executable = self
            .executable
            .as_deref()
            .ok_or(Error::State("audio metadata probe unavailable"))?;
        let offset = file.stream_position()?;
        let checked = (|| {
            let first = leading_frames(file, metadata.len())?;
            file.rewind()?;
            let value = process::run(
                executable,
                process::Input::HeldMp3(file.try_clone()?),
                self.timeout,
            )?;
            decoded_metadata(name, &value, first, metadata.len())
        })();
        file.seek(SeekFrom::Start(offset))?;
        checked
    }
}

fn decoded_metadata(
    name: &Path,
    value: &Value,
    leading: Header,
    bytes: u64,
) -> Result<AudioMetadata> {
    let [stream] = value["streams"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    else {
        return Err(Error::Media("held MP3 requires one audio stream"));
    };
    if value["format"]["format_name"] != "mp3"
        || stream["codec_type"] != "audio"
        || stream["codec_name"] != "mp3"
    {
        return Err(Error::Media("held audio was not decoded as MP3"));
    }
    let stream_index = stream["index"]
        .as_u64()
        .ok_or(Error::Media("held MP3 stream index missing"))?;
    let frames = value["frames"]
        .as_array()
        .filter(|frames| !frames.is_empty() && frames.len() <= 8)
        .ok_or(Error::Media("held MP3 produced no bounded audio frames"))?;
    if !frames.iter().all(|frame| {
        frame["media_type"] == "audio"
            && frame["stream_index"].as_u64() == Some(stream_index)
            && frame["channels"].as_u64() == Some(u64::from(leading.channels))
            && frame["nb_samples"]
                .as_u64()
                .is_some_and(|samples| (1..=1152).contains(&samples))
    }) {
        return Err(Error::Media("invalid held MP3 decoded frames"));
    }
    let metadata = parse_ffprobe_response(name, value)?;
    // The smallest supported Layer III bit rate is 8 kbit/s. Allow
    // rounding below 1000 B/s and short-stream estimation error, but do
    // not project an unbounded duration from a forged Xing/Info header.
    if metadata.duration <= 0.0
        || metadata.duration > bytes as f64 / 900.0 + 1.0
        || metadata.sample_rate != Some(leading.rate)
        || metadata.channels != Some(leading.channels)
    {
        return Err(Error::Media("invalid held MP3 metadata"));
    }
    Ok(metadata)
}

#[derive(Clone, Copy)]
struct Header {
    bytes: u64,
    rate: u32,
    channels: u32,
}

fn leading_frames(file: &mut File, len: u64) -> Result<Header> {
    file.rewind()?;
    let mut prefix = [0; 10];
    file.read_exact(&mut prefix)?;
    let start = if prefix[..3] == *b"ID3" {
        let (tag_len, footer) =
            tag_span(prefix).ok_or(Error::Media("invalid or oversized held MP3 ID3 header"))?;
        if footer {
            let mut tail = [0; 10];
            file.seek(SeekFrom::Start(10 + tag_len))?;
            file.read_exact(&mut tail)?;
            if tail[..3] != *b"3DI" || tail[3..] != prefix[3..] {
                return Err(Error::Media("invalid held MP3 ID3 footer"));
            }
        }
        10 + tag_len + if footer { 10 } else { 0 }
    } else {
        0
    };
    let read_header = |file: &mut File, at: u64| -> Result<Header> {
        if at.checked_add(4).is_none_or(|end| end > len) {
            return Err(Error::Media("truncated held MP3 frame header"));
        }
        file.seek(SeekFrom::Start(at))?;
        let mut bytes = [0; 4];
        file.read_exact(&mut bytes)?;
        let header = mpeg_header(bytes).ok_or(Error::Media("invalid held MP3 frame header"))?;
        if at.checked_add(header.bytes).is_none_or(|end| end > len) {
            return Err(Error::Media("truncated held MP3 frame"));
        }
        Ok(header)
    };
    let first = read_header(file, start)?;
    let second = read_header(file, start + first.bytes)?;
    if first.rate != second.rate || first.channels != second.channels {
        return Err(Error::Media("held MP3 leading geometry changed"));
    }
    Ok(first)
}

fn tag_span(header: [u8; 10]) -> Option<(u64, bool)> {
    let valid_flags = match header[3] {
        2 => 0xc0,
        3 => 0xe0,
        4 => 0xf0,
        _ => return None,
    };
    if header[4] != 0 || header[5] & !valid_flags != 0 || header[6..].iter().any(|b| b & 0x80 != 0)
    {
        return None;
    }
    let length = header[6..]
        .iter()
        .fold(0, |size, &b| (size << 7) | u64::from(b));
    (length <= MAX_TAG_BYTES).then_some((length, header[3] == 4 && header[5] & 0x10 != 0))
}

// The two Layer III bit-rate tables and version rate divisions match
// FFmpeg libavcodec/mpegaudiodecheader.c and mpegaudiodata.c. Free-format
// (bit-rate index zero) cannot establish the next frame offset in this lane.
fn mpeg_header(bytes: [u8; 4]) -> Option<Header> {
    const MPEG1: [u64; 14] = [
        32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const MPEG2: [u64; 14] = [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let bits = u32::from_be_bytes(bytes);
    if bits >> 21 != 0x7ff || (bits >> 17) & 3 != 1 || bits & 3 == 2 {
        return None;
    }
    let (divisor, slots, table) = match (bits >> 19) & 3 {
        3 => (1, 144, MPEG1),
        2 => (2, 72, MPEG2),
        0 => (4, 72, MPEG2),
        _ => return None,
    };
    let index = usize::try_from((bits >> 12) & 15).ok()?.checked_sub(1)?;
    let rate = [44_100, 48_000, 32_000].get(((bits >> 10) & 3) as usize)? / divisor;
    let bytes = slots * table.get(index)? * 1000 / u64::from(rate) + u64::from((bits >> 9) & 1);
    let channels = if (bits >> 6) & 3 == 3 { 1 } else { 2 };
    Some(Header {
        bytes,
        rate,
        channels,
    })
}
