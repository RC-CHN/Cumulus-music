//! Recovered offline stages for PC song recognition.
//!
//! Accepts already-centered matrices and supports peak selection, packet packing,
//! and sealing. It does not record audio, compute the signal matrix, or send
//! a recognition request. Stock zlib is an explicit optional feature.

mod packing;
mod selection;
#[cfg(feature = "song-recognition-zlib")]
mod stock_zlib;

pub use packing::{PeakRecord, pack_default_packet};
pub use selection::{CenteredPeakMatrix, select_default_peaks};
#[cfg(feature = "song-recognition-zlib")]
pub use stock_zlib::compress_packed_packet;

use crate::{Result, protocol::wire::crypto::encrypt_ecb};
use base64::{Engine, engine::general_purpose::STANDARD};

// D0ADE0 reads the 16 ASCII bytes 074B97221F27F029; the native default
// encryption branch rotates the first two bytes to the end.
const FINGERPRINT_KEY: &[u8; 16] = b"4B97221F27F02907";

/// The encrypted bytes used to produce the request's `rawdata` field.
///
/// Exposed explicitly to the caller; the type has no default debug or
/// serialization representation for audio-derived data.
pub struct SealedFingerprint(Vec<u8>);

impl SealedFingerprint {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn rawdata(&self) -> String {
        STANDARD.encode(&self.0)
    }
}

/// Seals the complete zlib stream produced by the fingerprint packet encoder.
///
/// The native D10280 stage returns an empty vector for absent input. For
/// nonempty input it pads with PKCS#7 and encrypts each 16-byte block
/// independently. This does not establish that the input is a valid packet,
/// that its deflate encoding matches the official compressor, or that a
/// service will accept the fingerprint.
pub fn seal_compressed_packet(compressed: &[u8]) -> Result<Option<SealedFingerprint>> {
    if compressed.is_empty() {
        return Ok(None);
    }
    encrypt_ecb(compressed, FINGERPRINT_KEY).map(|bytes| Some(SealedFingerprint(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use std::io::Read;

    #[derive(Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Case {
        name: String,
        packed_hex: String,
        native_packet_header: NativePacketHeader,
        native_peaks: Vec<(i32, i32, u32)>,
        compressed_hex: String,
        fingerprint_base64: String,
        fingerprint_sha256: String,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct NativePacketHeader {
        sample_count: u32,
    }

    #[test]
    fn packs_and_seals_original_synthetic_fingerprints_byte_for_byte() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/song-recognition-official.json"
        ))
        .unwrap();

        for case in fixture.cases {
            let packed = hex::decode(case.packed_hex).unwrap();
            let peaks = case
                .native_peaks
                .into_iter()
                .map(|(bin, frame, value_bits)| PeakRecord {
                    bin,
                    frame,
                    value_bits,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                pack_default_packet(case.native_packet_header.sample_count, &peaks).unwrap(),
                packed,
                "{}",
                case.name
            );
            let compressed = hex::decode(case.compressed_hex).unwrap();
            #[cfg(feature = "song-recognition-zlib")]
            assert_eq!(
                compress_packed_packet(&packed).unwrap().unwrap(),
                compressed,
                "{}",
                case.name
            );
            let mut original_packet = Vec::new();
            let mut stream = ZlibDecoder::new(compressed.as_slice());
            stream.read_to_end(&mut original_packet).unwrap();
            assert_eq!(original_packet, packed, "{}", case.name);
            assert_eq!(stream.total_in(), compressed.len() as u64, "{}", case.name);

            let sealed = seal_compressed_packet(&compressed).unwrap().unwrap();
            assert_eq!(sealed.rawdata(), case.fingerprint_base64, "{}", case.name);
            assert_eq!(
                hex::encode(Sha256::digest(sealed.as_bytes())),
                case.fingerprint_sha256,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn empty_packet_has_no_fingerprint() {
        assert!(seal_compressed_packet(&[]).unwrap().is_none());
    }
}
