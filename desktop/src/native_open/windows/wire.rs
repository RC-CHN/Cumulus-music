//! One bounded frame/receipt on an authenticated, single-use local channel.
use std::{fmt, io};

pub const MAGIC: &[u8; 4] = b"CFW1";
pub const MAX: usize = 32 * 1024;
#[cfg(windows)]
pub(super) const READY: u8 = 6;
#[cfg(windows)]
#[allow(
    dead_code,
    reason = "the older standalone lab does not consume product receipts"
)]
pub(super) const CONSUMED: u8 = 7;
const MAX_PATH: usize = 4096;
const MAX_COUNT: u8 = 8;

#[derive(Debug)]
struct OversizedHeader;

impl fmt::Display for OversizedHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("lab frame length exceeds limit")
    }
}
impl std::error::Error for OversizedHeader {}

#[allow(dead_code, reason = "exercised by the standalone Windows lab")]
pub fn is_oversized(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::InvalidData
        && error
            .get_ref()
            .is_some_and(|cause| cause.is::<OversizedHeader>())
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "malformed lab frame")
}

#[cfg(windows)]
pub(super) fn receipt(code: u8) -> [u8; 5] {
    [MAGIC[0], MAGIC[1], MAGIC[2], MAGIC[3], code]
}

#[cfg(windows)]
pub(super) fn receipt_code(bytes: [u8; 5]) -> io::Result<u8> {
    if bytes[..4] != MAGIC[..] {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(bytes[4])
}

fn checked(value: &[u8]) -> io::Result<&str> {
    if value.is_empty() || value.len() > MAX_PATH {
        return Err(invalid());
    }
    let text = std::str::from_utf8(value).map_err(|_| invalid())?;
    if text.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(text)
}

pub fn encode(paths: &[&str]) -> io::Result<Vec<u8>> {
    if paths.is_empty() || paths.len() > MAX_COUNT as usize {
        return Err(invalid());
    }
    let mut frame = MAGIC.to_vec();
    frame.extend([0; 4]);
    frame.push(paths.len() as u8);
    for path in paths {
        let bytes = path.as_bytes();
        checked(bytes)?;
        frame.extend((bytes.len() as u16).to_be_bytes());
        frame.extend(bytes);
    }
    let size = frame.len() - 8;
    if size > MAX {
        return Err(invalid());
    }
    frame[4..8].copy_from_slice(&(size as u32).to_be_bytes());
    Ok(frame)
}

pub fn size(head: [u8; 8]) -> io::Result<usize> {
    let length = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if &head[..4] != MAGIC || length == 0 {
        return Err(invalid());
    }
    if length > MAX {
        return Err(io::Error::new(io::ErrorKind::InvalidData, OversizedHeader));
    }
    Ok(length)
}

pub fn decode(frame: &[u8]) -> io::Result<Vec<String>> {
    let (&count, mut remaining) = frame.split_first().ok_or_else(invalid)?;
    if count == 0 || count > MAX_COUNT || frame.len() > MAX {
        return Err(invalid());
    }
    let mut paths = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (header, rest) = remaining.split_at_checked(2).ok_or_else(invalid)?;
        let len = usize::from(u16::from_be_bytes([header[0], header[1]]));
        let (path, rest) = rest.split_at_checked(len).ok_or_else(invalid)?;
        paths.push(checked(path)?.to_owned());
        remaining = rest;
    }
    if !remaining.is_empty() {
        return Err(invalid());
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_unicode_group_and_closed_framing() {
        let paths = [r#"C:\合成\01 % 空格.mp3"#, r#"C:\合成\02.flac"#];
        let frame = encode(&paths).unwrap();
        assert_eq!(
            size(frame[..8].try_into().unwrap()).unwrap(),
            frame.len() - 8
        );
        assert_eq!(decode(&frame[8..]).unwrap(), paths);
        assert!(decode(&frame[8..frame.len() - 1]).is_err());
        assert!(decode(&[1, 0, 1, 255]).is_err());
        assert!(decode(&[1, 0, 1, b'x', 0]).is_err());
        assert!(encode(&["x"; 9]).is_err());
        assert!(encode(&["\n"]).is_err());
        let mut head = [0; 8];
        head[..4].copy_from_slice(MAGIC);
        assert!(!is_oversized(&size(head).unwrap_err()));
        head[4..].copy_from_slice(&(MAX as u32).to_be_bytes());
        assert_eq!(size(head).unwrap(), MAX);
        head[4..].copy_from_slice(&((MAX + 1) as u32).to_be_bytes());
        assert!(is_oversized(&size(head).unwrap_err()));
        head[0] ^= 1;
        assert!(!is_oversized(&size(head).unwrap_err()));
    }
}
