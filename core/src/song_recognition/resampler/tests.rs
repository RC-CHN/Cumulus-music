use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{env, fmt::Write, ops::Range, path::PathBuf};

#[derive(Deserialize)]
struct Fixtures {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    case: String,
    auto: Expected,
    scalar: Expected,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    frames: usize,
    packets: usize,
    zero_packets: usize,
    produced: usize,
    produced_at_3_seconds: isize,
    input_delay_at_stop: i64,
    tail: usize,
    input_sha256: String,
    mono_sha256: String,
    output_sha256: String,
    diagnostic_tail_sha256: String,
    packets_sha256: String,
}

enum Input {
    I16(Vec<[i16; 2]>),
    F32(Vec<[f32; 2]>),
}

impl Input {
    fn new(frames: usize, format: StereoPcmFormat, clipped: bool) -> Self {
        let mut seed = 0x2a23_a904_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            ((seed >> 16) as i32 - 32_768) as i16
        };
        let mut noise = |i| {
            let left = next();
            let right = next();
            [
                if i == 0 {
                    32767
                } else if i + 1 == frames {
                    -32768
                } else {
                    left
                },
                right,
            ]
        };
        match format {
            StereoPcmFormat::I16 => {
                let levels = [32767, -32768, 32767, -32768, 0, 16384, -16384, 1, -1, 0];
                Self::I16(
                    (0..frames)
                        .map(|i| {
                            if clipped {
                                let j = i / 233 % 10;
                                [levels[j], levels[(j + usize::from(i / 233 % 3 == 0)) % 10]]
                            } else {
                                noise(i)
                            }
                        })
                        .collect(),
                )
            }
            StereoPcmFormat::F32 => {
                let levels = [
                    1.5,
                    -1.5,
                    1.0,
                    -1.0,
                    0.0,
                    32767.0 / 32768.0,
                    -32769.0 / 32768.0,
                    0.5,
                    -0.5,
                    0.0,
                ];
                Self::F32(
                    (0..frames)
                        .map(|i| {
                            if clipped {
                                let j = i / 233 % 10;
                                [levels[j], levels[(j + usize::from(i / 233 % 3 == 0)) % 10]]
                            } else {
                                noise(i).map(|v| f32::from(v) * (1.0 / 32768.0))
                            }
                        })
                        .collect(),
                )
            }
        }
    }

    fn packet(&self, range: Range<usize>) -> StereoPcmPacket<'_> {
        match self {
            Self::I16(input) => StereoPcmPacket::I16(&input[range]),
            Self::F32(input) => StereoPcmPacket::F32(&input[range]),
        }
    }

    fn digest(&self) -> String {
        let mut hash = Sha256::new();
        match self {
            Self::I16(input) => input
                .iter()
                .flatten()
                .for_each(|value| hash.update(value.to_le_bytes())),
            Self::F32(input) => input
                .iter()
                .flatten()
                .for_each(|value| hash.update(value.to_le_bytes())),
        }
        hex::encode(hash.finalize())
    }
}

fn observe_pipeline<'b, 'a>(
    pipeline: &'b NativePipeline<'a>,
    mono: &mut Sha256,
) -> (&'b StageProbe, &'b Stage<'a, i16>) {
    match pipeline {
        NativePipeline::I16(p) => {
            for value in &p.mixer.buffer[..p.mixer.last.frames] {
                mono.update(value.to_le_bytes());
            }
            (&p.mixer.last, &p.resampler)
        }
        NativePipeline::F32(p) => {
            for value in &p.mixer.buffer[..p.mixer.last.frames] {
                mono.update(value.to_le_bytes());
            }
            (&p.mixer.last, &p.resampler)
        }
    }
}

fn verify_case(libraries: &ResamplerLibraries, name: &str, expected: &Expected) {
    let parts: Vec<_> = name.split('-').collect();
    let rate = match parts[0] {
        "44100" => StereoSampleRate::Hz44100,
        "48000" => StereoSampleRate::Hz48000,
        _ => panic!("unexpected rate"),
    };
    let format = match parts[1] {
        "i16" => StereoPcmFormat::I16,
        "f32" => StereoPcmFormat::F32,
        _ => panic!("unexpected sample format"),
    };
    let input = Input::new(expected.frames, format, parts[2] == "clip");
    assert_eq!(input.digest(), expected.input_sha256, "{name}");
    let mut stream = libraries.stream(rate, format).unwrap();
    assert!(stream.push(input.packet(0..0)).unwrap().is_empty());

    let mut packets = 0;
    let mut zeros = 0;
    let mut offset = 0;
    let mut samples = 0;
    let mut at_3s = -1;
    let mut csv = String::new();
    let mut mono_hash = Sha256::new();
    let mut output_hash = Sha256::new();
    while offset < expected.frames {
        let frames = match parts[3] {
            "single" => expected.frames,
            "10ms" => rate.hz() as usize / 100,
            "ragged" => {
                let chunks = [1, 7, 31, 17, 146, 2, 509, 11, 960, 3, 441, 113, 4];
                chunks[packets % chunks.len()]
            }
            _ => panic!("unexpected packet split"),
        }
        .min(expected.frames - offset);
        let out = stream.push(input.packet(offset..offset + frames)).unwrap();
        for value in out {
            output_hash.update(value.to_le_bytes());
        }
        samples += out.len();
        zeros += usize::from(out.is_empty());
        let (mix, swr) = observe_pipeline(&stream.pipeline, &mut mono_hash);
        writeln!(
            csv,
            "{packets},{offset},{frames},{},{},{},{},{},{}",
            mix.delay,
            mix.frames,
            swr.last.delay,
            swr.last.required,
            swr.last.frames,
            swr.context.delay().unwrap(),
        )
        .unwrap();
        offset += frames;
        packets += 1;
        if offset == 3 * rate.hz() as usize {
            at_3s = samples as isize;
        }
    }
    assert_eq!(packets, expected.packets, "{name} packets");
    assert_eq!(zeros, expected.zero_packets, "{name} empty output");
    assert_eq!(samples, expected.produced, "{name} sample count");
    assert_eq!(at_3s, expected.produced_at_3_seconds, "{name} at 3s");
    assert_eq!(
        hex::encode(mono_hash.finalize()),
        expected.mono_sha256,
        "{name} mono"
    );
    assert_eq!(
        hex::encode(output_hash.finalize()),
        expected.output_sha256,
        "{name} i16"
    );
    assert_eq!(
        hex::encode(Sha256::digest(csv)),
        expected.packets_sha256,
        "{name} packet delay"
    );

    // Diagnostic only: verify delayed samples separately; callers of push()
    // never receive these extra samples upon drop or empty input.
    let stage = match &mut stream.pipeline {
        NativePipeline::I16(p) => &mut p.resampler,
        NativePipeline::F32(p) => &mut p.resampler,
    };
    assert_eq!(
        stage.context.delay().unwrap(),
        expected.input_delay_at_stop,
        "{name} delay at stop"
    );
    let mut tail = [0_i16; 512];
    let mut tail_frames = 0;
    let mut tail_hash = Sha256::new();
    for iteration in 0..8 {
        // SAFETY: mono i16, 512 writable samples; no input means diagnostic drain.
        let produced = unsafe {
            stage
                .context
                .convert(tail.as_mut_ptr().cast(), 512, None, 0)
        }
        .unwrap();
        for value in &tail[..produced] {
            tail_hash.update(value.to_le_bytes());
        }
        tail_frames += produced;
        if produced == 0 {
            break;
        }
        assert_ne!(iteration, 7, "diagnostic drain must finish");
    }
    assert_eq!(tail_frames, expected.tail, "{name} drain length");
    assert_eq!(
        hex::encode(tail_hash.finalize()),
        expected.diagnostic_tail_sha256,
        "{name} tail"
    );
}

#[test]
#[ignore = "requires a caller-verified private stock library pair"]
fn exact_local_pair_matches_frozen_native_packet_corpus() {
    let directory = PathBuf::from(
        env::var_os("CUMULUS_RECOGNITION_SWR_TEST_DIR")
            .expect("provide the private stock pair with CUMULUS_RECOGNITION_SWR_TEST_DIR"),
    );
    let mode = env::var("CUMULUS_RECOGNITION_SWR_TEST_MODE").unwrap_or_else(|_| "auto".to_string());
    assert!(matches!(mode.as_str(), "auto" | "scalar"));
    // SAFETY: this opt-in test is explicitly given a previously verified local pair.
    let libraries = unsafe { ResamplerLibraries::load_local(&directory) }.unwrap();
    let fixtures: Fixtures = serde_json::from_str(include_str!(
        "../../../tests/fixtures/song-recognition-resampler-official.json"
    ))
    .unwrap();
    for case in &fixtures.cases {
        let expected = if mode == "scalar" {
            &case.scalar
        } else {
            &case.auto
        };
        verify_case(&libraries, &case.case, expected);
    }
    assert_eq!(fixtures.cases.len(), 24);

    let mut stream = libraries
        .stream(StereoSampleRate::Hz48000, StereoPcmFormat::F32)
        .unwrap();
    for invalid in [f32::NAN, f32::INFINITY, 4.01] {
        assert!(
            stream
                .push(StereoPcmPacket::F32(&[[invalid, 0.0]]))
                .is_err()
        );
    }
    assert!(stream.push(StereoPcmPacket::I16(&[[0, 0]])).is_err());
    assert!(
        stream
            .push(StereoPcmPacket::F32(&vec![[0.0; 2]; MAX_PACKET_FRAMES + 1]))
            .is_err()
    );
    assert_eq!(stream.consumed, 0);
    assert!(
        stream
            .push(StereoPcmPacket::F32(&[[0.0, 0.0]]))
            .unwrap()
            .is_empty()
    );
    drop(stream);
    drop(libraries);

    // New sessions and unloading the pair release loader and context state.
    for _ in 0..24 {
        // SAFETY: use the same opt-in local stock pair.
        let pair = unsafe { ResamplerLibraries::load_local(&directory) }.unwrap();
        let _fresh = pair
            .stream(StereoSampleRate::Hz44100, StereoPcmFormat::I16)
            .unwrap();
    }
}

#[test]
fn requires_an_explicit_existing_absolute_library_pair() {
    // SAFETY: the invalid inputs are rejected before native loading.
    unsafe {
        assert!(ResamplerLibraries::load_local(Path::new(".")).is_err());
        let empty = tempfile::tempdir().unwrap();
        assert!(ResamplerLibraries::load_local(empty.path()).is_err());
    }
}
