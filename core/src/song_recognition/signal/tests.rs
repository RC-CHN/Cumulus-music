use super::*;
use crate::song_recognition::pack_default_packet;
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Read};

#[derive(Deserialize)]
struct OfficialPackets {
    cases: Vec<OfficialPacket>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfficialPacket {
    name: String,
    input_sha256: String,
    native_peaks: Vec<(i32, i32, u32)>,
    packed_hex: String,
    #[cfg(feature = "song-recognition-zlib")]
    fingerprint_base64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stages {
    name: String,
    power_sha256: String,
    sse_log_sha256: String,
    sse_centered_sha256: String,
    fma_log_sha256: String,
    fma_centered_sha256: String,
}

fn digest_floats(values: &[f32]) -> String {
    let mut digest = Sha256::new();
    for &value in values {
        digest.update(value.to_le_bytes());
    }
    hex::encode(digest.finalize())
}

fn synthetic_pcm(name: &str) -> Vec<i16> {
    match name {
        "noise" => {
            let mut x = 0x2a23_a904_u32;
            (0..24_000)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    ((i32::try_from(x >> 16).unwrap() - 32_768).div_euclid(2)) as i16
                })
                .collect()
        }
        "two-tone" => {
            let mut bytes = Vec::new();
            GzDecoder::new(
                &include_bytes!("../../../tests/fixtures/song-recognition-two-tone.i16le.gz")[..],
            )
            .take(48_001)
            .read_to_end(&mut bytes)
            .unwrap();
            assert_eq!(bytes.len(), 48_000);
            bytes
                .chunks_exact(2)
                .map(|sample| i16::from_le_bytes(sample.try_into().unwrap()))
                .collect()
        }
        "silence" => vec![0; 24_000],
        _ => panic!("unrecognized synthetic fixture"),
    }
}

#[test]
fn matches_native_power_log_center_peaks_and_packet_from_pcm() {
    let official: OfficialPackets = serde_json::from_str(include_str!(
        "../../../tests/fixtures/song-recognition-official.json"
    ))
    .unwrap();
    let fixtures: Vec<Stages> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/song-recognition-signal-official.json"
    ))
    .unwrap();

    for expected in fixtures {
        let name = &expected.name;
        let original = official
            .cases
            .iter()
            .find(|case| &case.name == name)
            .unwrap();
        let samples = synthetic_pcm(name);
        let mut raw_digest = Sha256::new();
        for &sample in &samples {
            raw_digest.update(sample.to_le_bytes());
        }
        assert_eq!(hex::encode(raw_digest.finalize()), original.input_sha256);

        let pcm = FingerprintPcm8k::new(&samples).unwrap();
        let frames = pcm.complete_frames();
        assert_eq!(frames, 138);
        let mut power_digest = Sha256::new();
        fft::for_each_power_frame(&samples, |_, power| {
            for value in power {
                power_digest.update(value.to_le_bytes());
            }
        });
        assert_eq!(
            hex::encode(power_digest.finalize()),
            expected.power_sha256,
            "{name} power",
        );

        for (mode, log_sha, center_sha) in [
            (
                FingerprintLogMode::Sse,
                &expected.sse_log_sha256,
                &expected.sse_centered_sha256,
            ),
            (
                FingerprintLogMode::Fma,
                &expected.fma_log_sha256,
                &expected.fma_centered_sha256,
            ),
        ] {
            let logs = logarithmic_spectrum(&pcm, frames, mode);
            assert_eq!(digest_floats(&logs), *log_sha, "{name} log");
            let output = centered_default_spectrum(&pcm, mode).unwrap();
            let matrix = output.matrix();
            assert_eq!(matrix.rows(), 999);
            assert_eq!(matrix.frames(), 138);
            assert_eq!(
                digest_floats(matrix.values()),
                *center_sha,
                "{name} centered"
            );

            let peaks = output.peaks();
            assert_eq!(
                peaks
                    .iter()
                    .map(|p| (p.bin, p.frame, p.value_bits))
                    .collect::<Vec<_>>(),
                original.native_peaks,
                "{name} peaks",
            );
            let packed = pack_default_packet(samples.len() as u32, &peaks).unwrap();
            assert_eq!(hex::encode(&packed), original.packed_hex, "{name} packet");

            #[cfg(feature = "song-recognition-zlib")]
            {
                use crate::song_recognition::{compress_packed_packet, seal_compressed_packet};
                let compressed = compress_packed_packet(&packed).unwrap().unwrap();
                let fingerprint = seal_compressed_packet(&compressed).unwrap().unwrap();
                assert_eq!(
                    fingerprint.rawdata(),
                    original.fingerprint_base64,
                    "{name} sealed"
                );
            }
        }
    }
}

#[test]
fn both_log_modes_match_native_boundary_records_including_rounding_difference() {
    let mut inputs = BTreeSet::from([
        0x3596_37bd,
        0x3596_37be,
        0x3596_37bf,
        0x3f80_0000,
        0x7f7f_ffff,
        0x0080_0000,
    ]);
    for exponent in [-126_i32, -31, -20, -1, 0, 1, 6, 13, 64, 127] {
        let exponent_bits = ((exponent + 127) as u32) << 23;
        for bucket in 0..128 {
            let start = bucket << 16;
            for offset in [0, 32_767, 32_768, 32_769] {
                inputs.insert(exponent_bits | (start + offset));
            }
        }
    }
    for anchor in [0.9375_f32, 1.0625_f32, 1.0_f32].map(f32::to_bits) {
        inputs.extend(anchor - 32..=anchor + 32);
    }
    assert_eq!(inputs.len(), 5_316);
    let different = 1_066_625_693;
    for (mode, value, expected) in [
        (
            FingerprintLogMode::Sse,
            1_041_276_660,
            "01f24dd6168cfa79fc32f152321968561ec424a9e60fd1ef85aec4db547a708e",
        ),
        (
            FingerprintLogMode::Fma,
            1_041_276_661,
            "de75bbcf2b50d3640d358693a7cbe481b68672e78599558b5437330386513a2c",
        ),
    ] {
        let mut digest = Sha256::new();
        for bits in inputs.iter().copied().chain([different]) {
            let result = logarithm::positive_normal(f32::from_bits(bits), mode);
            digest.update(bits.to_le_bytes());
            digest.update(result.to_le_bytes());
        }
        assert_eq!(hex::encode(digest.finalize()), expected);
        assert_eq!(
            logarithm::positive_normal(f32::from_bits(different), mode).to_bits(),
            value
        );
    }
}

#[test]
fn honors_complete_frame_gate_and_offline_input_limit() {
    let samples = vec![0; MAX_SAMPLES + 1];
    let first_supported = WINDOW_SAMPLES + (MIN_FRAMES - 1) * FRAME_HOP;
    for count in [0, WINDOW_SAMPLES - 1, first_supported - 1] {
        let pcm = FingerprintPcm8k::new(&samples[..count]).unwrap();
        assert!(centered_default_spectrum(&pcm, FingerprintLogMode::Sse).is_none());
    }
    let pcm = FingerprintPcm8k::new(&samples[..first_supported + FRAME_HOP - 1]).unwrap();
    let spectrum = centered_default_spectrum(&pcm, FingerprintLogMode::Sse).unwrap();
    assert_eq!(spectrum.matrix().frames(), MIN_FRAMES);
    assert!(spectrum.peaks().is_empty());
    assert_eq!(
        FingerprintPcm8k::new(&samples[..MAX_SAMPLES])
            .unwrap()
            .complete_frames(),
        MAX_FRAMES,
    );
    assert!(FingerprintPcm8k::new(&samples).is_err());
}
