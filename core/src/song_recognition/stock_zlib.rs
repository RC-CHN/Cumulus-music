use crate::{Error, Result};
use std::ffi::CStr;

const EXPECTED_VERSION: &CStr = c"1.3.2";
// This is a local memory budget for the offline codec, not a server limit.
const MAX_PACKED_BYTES: usize = 128 * 1024;
const MAX_OUTPUT_BYTES: usize = MAX_PACKED_BYTES + 4 * 1024;

/// Compresses an already packed fingerprint with the pinned stock zlib.
///
/// The upstream PC wrapper D0D770 returns no stream for empty input. For
/// nonempty input it uses the default level and zlib framing, with no
/// dictionary. The optional dependency is stock zlib, independently of the
/// application's flate2 backend. Byte parity is frozen in the synthetic tests.
pub fn compress_packed_packet(packed: &[u8]) -> Result<Option<Vec<u8>>> {
    if packed.is_empty() {
        return Ok(None);
    }
    if packed.len() > MAX_PACKED_BYTES {
        return Err(Error::InvalidInput("packed recognition input is too large"));
    }

    // SAFETY: zlibVersion returns a static, NUL-terminated string owned by the
    // linked library. Check for null before constructing the borrowed CStr.
    let version = unsafe { libz_sys::zlibVersion() };
    if version.is_null() || unsafe { CStr::from_ptr(version) } != EXPECTED_VERSION {
        return Err(Error::State("unexpected song recognition zlib version"));
    }
    let source_len = libz_sys::uLong::try_from(packed.len())
        .map_err(|_| Error::InvalidInput("packed recognition length is not representable"))?;

    // SAFETY: compressBound consumes a plain bounded integer and no pointers.
    let bound = unsafe { libz_sys::compressBound(source_len) };
    let capacity = usize::try_from(bound)
        .ok()
        .filter(|&size| size >= packed.len() && size <= MAX_OUTPUT_BYTES)
        .ok_or(Error::Protocol(
            "invalid song recognition compression bound",
        ))?;
    let mut output = vec![0u8; capacity];
    let mut written = bound;

    // SAFETY: the non-overlapping Rust allocations remain alive and stable for
    // this synchronous call. source_len equals packed.len(), and written is
    // initialized to the writable output capacity in the binding's C type.
    let status = unsafe {
        libz_sys::compress2(
            output.as_mut_ptr(),
            &mut written,
            packed.as_ptr(),
            source_len,
            libz_sys::Z_DEFAULT_COMPRESSION,
        )
    };
    if status != libz_sys::Z_OK {
        return Err(Error::Protocol("song recognition compression failed"));
    }
    let actual = usize::try_from(written)
        .ok()
        .filter(|&size| size > 0 && size <= capacity)
        .ok_or(Error::Protocol(
            "invalid song recognition compressed length",
        ))?;
    output.truncate(actual);
    Ok(Some(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use sha2::{Digest, Sha256};

    #[derive(Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Case {
        input_bytes: usize,
        shape: Shape,
        input_sha256: String,
        compressed_bytes: usize,
        compressed_sha256: String,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Shape {
        Repeated,
        Mixed,
        Noise,
    }

    impl Case {
        fn synthetic_input(&self) -> Vec<u8> {
            let mut x = 0x8812_34fe_u32;
            (0..self.input_bytes)
                .map(|i| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    match self.shape {
                        Shape::Repeated => ((i * 7 + i / 563) % 253) as u8,
                        Shape::Mixed if i % 771 < 391 => ((i / 29) % 31) as u8,
                        Shape::Mixed | Shape::Noise => (x >> 24) as u8,
                    }
                })
                .collect()
        }
    }

    #[test]
    fn compression_matches_native_streams_across_buffer_boundaries() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/song-recognition-zlib-official.json"
        ))
        .unwrap();

        for case in fixture.cases {
            let input = case.synthetic_input();
            assert_eq!(hex::encode(Sha256::digest(&input)), case.input_sha256);
            let compressed = compress_packed_packet(&input).unwrap().unwrap();
            assert_eq!(
                compressed.len(),
                case.compressed_bytes,
                "{}",
                case.input_bytes
            );
            assert_eq!(
                hex::encode(Sha256::digest(&compressed)),
                case.compressed_sha256,
                "{}",
                case.input_bytes
            );
        }
    }

    #[test]
    fn absent_and_oversized_packets_do_not_enter_compression() {
        assert!(compress_packed_packet(&[]).unwrap().is_none());
        assert!(matches!(
            compress_packed_packet(&vec![0; MAX_PACKED_BYTES + 1]),
            Err(Error::InvalidInput(_))
        ));
    }
}
