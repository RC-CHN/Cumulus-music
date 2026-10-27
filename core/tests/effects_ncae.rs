use cumulus_core::effects::ncae::{self, Limits};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{self, Read};

#[derive(Deserialize)]
struct Valid {
    name: String,
    encoded_hex: String,
    plain_bytes: usize,
    plain_sha256: String,
    key_bytes: usize,
}

#[derive(Deserialize)]
struct Malformed {
    name: String,
    encoded_hex: String,
}

#[derive(Deserialize)]
struct Corpus {
    valid_containers: Vec<Valid>,
    malformed_containers: Vec<Malformed>,
}

fn corpus() -> Corpus {
    serde_json::from_str(include_str!("fixtures/ncae-official.json")).unwrap()
}

#[test]
fn native_wrapped_empty_binary_multiblock_and_extreme_key_containers() {
    let samples = corpus().valid_containers;
    assert_eq!(samples.len(), 6);
    for sample in samples {
        let bytes = hex::decode(sample.encoded_hex).unwrap();
        let decoded = ncae::decode(&bytes, Limits::default()).unwrap();
        assert_eq!(decoded.type_id, 65535);
        assert_eq!(decoded.opaque_header, *b"ABCDEF");
        assert_eq!(decoded.key_bytes, sample.key_bytes);
        assert_eq!(decoded.plain.len(), sample.plain_bytes, "{}", sample.name);
        assert_eq!(
            hex::encode(Sha256::digest(&decoded.plain)),
            sample.plain_sha256
        );
        assert_eq!(decoded.summary().plain_sha256, sample.plain_sha256);
        assert_eq!(
            decoded.compressed_bytes as usize + 18 + sample.key_bytes,
            bytes.len()
        );
    }
}

#[test]
fn truncated_header_length_overflow_key_bounds_and_extra_bytes_fail() {
    let bytes = hex::decode(&corpus().valid_containers[1].encoded_hex).unwrap();
    for end in 0..bytes.len() {
        assert!(
            ncae::decode(&bytes[..end], Limits::default()).is_err(),
            "{end}"
        );
    }
    for size in [0u32, 1, u32::MAX] {
        let mut bad = bytes.clone();
        bad[4..8].copy_from_slice(&size.to_le_bytes());
        assert!(ncae::decode(&bad, Limits::default()).is_err());
    }
    for count in [0, 1, 2, 3, 4, 6, 255] {
        let mut bad = bytes.clone();
        bad[16] = count;
        assert!(ncae::decode(&bad, Limits::default()).is_err());
    }
    let mut wrong_magic = bytes.clone();
    wrong_magic[3] = b'X';
    assert!(ncae::decode(&wrong_magic, Limits::default()).is_err());
    let mut trailing = bytes;
    trailing.push(0);
    assert!(ncae::decode(&trailing, Limits::default()).is_err());
}

#[test]
fn invalid_incomplete_concatenated_and_wrapped_deflate_are_rejected() {
    let samples = corpus().malformed_containers;
    assert_eq!(samples.len(), 6);
    for sample in samples {
        assert!(
            ncae::decode(&hex::decode(sample.encoded_hex).unwrap(), Limits::default()).is_err(),
            "{}",
            sample.name
        );
    }
}

#[test]
fn inclusive_limits_cover_zero_exact_output_and_decompression_bomb() {
    for sample in corpus().valid_containers {
        let bytes = hex::decode(sample.encoded_hex).unwrap();
        let exact = Limits {
            container_bytes: bytes.len(),
            plain_bytes: sample.plain_bytes,
        };
        assert!(ncae::decode(&bytes, exact).is_ok());
        assert!(
            ncae::decode(
                &bytes,
                Limits {
                    container_bytes: bytes.len() - 1,
                    ..exact
                }
            )
            .is_err()
        );
        if sample.plain_bytes > 0 {
            for plain_bytes in [0, sample.plain_bytes - 1] {
                assert!(
                    ncae::decode(
                        &bytes,
                        Limits {
                            plain_bytes,
                            ..exact
                        }
                    )
                    .is_err()
                );
            }
        }
    }
}

struct SlowReader<'a> {
    bytes: &'a [u8],
    read: usize,
}

impl Read for SlowReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = output.len().min(self.bytes.len()).min(3);
        output[..count].copy_from_slice(&self.bytes[..count]);
        self.bytes = &self.bytes[count..];
        self.read += count;
        Ok(count)
    }
}

#[test]
fn bounded_reader_accepts_short_reads_and_stops_after_limit_plus_one() {
    let sample = corpus().valid_containers.remove(2);
    let bytes = hex::decode(&sample.encoded_hex).unwrap();
    let mut reader = SlowReader {
        bytes: &bytes,
        read: 0,
    };
    let result = ncae::read(&mut reader, Limits::default()).unwrap();
    assert_eq!(result.summary().plain_sha256, sample.plain_sha256);
    assert_eq!(reader.read, bytes.len());
    let mut reader = SlowReader {
        bytes: &bytes,
        read: 0,
    };
    assert!(
        ncae::read(
            &mut reader,
            Limits {
                container_bytes: 25,
                plain_bytes: 1024
            }
        )
        .is_err()
    );
    assert_eq!(reader.read, 26);
    let directory = tempfile::tempdir().unwrap();
    assert!(ncae::open(directory.path(), Limits::default()).is_err());
    let path = directory.path().join("material.ncae");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        ncae::open(path, Limits::default())
            .unwrap()
            .summary()
            .plain_sha256,
        sample.plain_sha256
    );
}
