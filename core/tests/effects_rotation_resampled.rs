use base64::{Engine, engine::general_purpose::STANDARD};
use cumulus_core::{Error, Result, effects::dsp::*};
use serde::Deserialize;
use std::{io::Read, sync::OnceLock};

#[derive(Deserialize)]
struct Tables {
    responses_b64: String,
}
fn response_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let stream = flate2::read::GzDecoder::new(
            &include_bytes!("fixtures/effects-spatial-official.json.gz")[..],
        );
        let mut raw = Vec::new();
        stream.take(32 * 1024 * 1024).read_to_end(&mut raw).unwrap();
        let tables: Tables = serde_json::from_slice(&raw).unwrap();
        STANDARD.decode(&tables.responses_b64).unwrap()
    })
}
fn responses() -> RotationResponses {
    RotationResponses::from_le_bytes(response_bytes()).unwrap()
}

struct InvalidConverter;
impl RotationRateConverter for InvalidConverter {
    fn process(&mut self, _: Option<[&[f32]; 2]>, output: [&mut [f32]; 2]) -> Result<usize> {
        Ok(output[0].len() + 1)
    }
    fn clear(&mut self) -> Result<()> {
        Ok(())
    }
}

#[test]
fn rejects_invalid_buffers_before_touching_the_converter_and_quarantines_bad_backend() {
    let format = Format::new(48000.0, 2, 256).unwrap();
    let mut stage =
        ResampledRotationStage::new(format, responses(), |_| Ok(InvalidConverter)).unwrap();
    let mut block = PlanarBlock::new(format);
    block.samples_mut()[0] = 0.5;
    let dry = stage.process(&mut block, 1, false).unwrap();
    assert_eq!(dry.valid_frames, 1);
    assert_eq!(block.samples()[0], 0.5);
    stage.set_bypass(false);
    assert!(matches!(
        stage.process(&mut block, 257, false),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        stage.process(&mut block, 1, false),
        Err(Error::Media(_))
    ));
    assert_eq!(block.samples()[0], 0.5);
    assert!(matches!(stage.reset(), Err(Error::State(_))));
    assert!(matches!(
        stage.process(&mut block, 1, false),
        Err(Error::State(_))
    ));
}

#[test]
fn a_disable_edge_caps_drain_and_forwards_buffered_input_before_new_dry_frames() {
    use std::{cell::RefCell, rc::Rc};
    type Call = (u32, Option<usize>, usize);
    struct Trace {
        rate: RotationRateConversion,
        calls: Rc<RefCell<Vec<Call>>>,
    }
    impl RotationRateConverter for Trace {
        fn process(
            &mut self,
            input: Option<[&[f32]; 2]>,
            [l, r]: [&mut [f32]; 2],
        ) -> Result<usize> {
            self.calls.borrow_mut().push((
                self.rate.input_rate(),
                input.map(|p| p[0].len()),
                l.len(),
            ));
            let count = input.map_or(1, |p| p[0].len()).min(l.len());
            l[..count].fill(0.25);
            r[..count].fill(-0.25);
            Ok(count)
        }
        fn clear(&mut self) -> Result<()> {
            self.calls
                .borrow_mut()
                .push((self.rate.input_rate(), None, 0));
            Ok(())
        }
    }
    let format = Format::new(48_000.0, 2, 256).unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut stage = ResampledRotationStage::new(format, responses(), |rate| {
        Ok(Trace {
            rate,
            calls: calls.clone(),
        })
    })
    .unwrap();
    let mut block = PlanarBlock::new(format);
    stage.set_bypass(false);
    assert_eq!(
        stage.process(&mut block, 256, false).unwrap().valid_frames,
        0
    );
    block.samples_mut().fill(0.5);
    stage.set_bypass(true);
    assert_eq!(
        stage.process(&mut block, 20, false).unwrap().valid_frames,
        256
    );
    assert_eq!(&block.samples()[..238], &[0.25; 238]);
    assert_eq!(&block.samples()[238..256], &[0.5; 18]);
    let end = stage.process(&mut block, 0, true).unwrap();
    assert_eq!((end.valid_frames, end.end), (2, true));
    assert_eq!(&block.samples()[..2], &[0.5; 2]);
    assert_eq!(
        *calls.borrow(),
        [
            (48_000, Some(256), 236),
            (48_000, Some(0), 0),
            (48_000, None, 236),
            (48_000, None, 0),
            (44_100, Some(236), 256),
            (44_100, Some(1), 256),
            (44_100, Some(0), 0),
            (44_100, None, 256),
            (44_100, None, 0),
        ]
    );
}

#[cfg(target_os = "linux")]
#[path = "../examples/support/effects_rotation_material.rs"]
mod material;
#[cfg(target_os = "linux")]
#[path = "../examples/support/effects_soxr_linux.rs"]
mod soxr;

#[cfg(target_os = "linux")]
mod host_proof {
    use super::soxr::{Converter, Host};
    use super::*;
    use std::{path::Path, sync::Arc};

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Row {
        rate: u32,
        capacity: u32,
        case: String,
        valid: Vec<usize>,
        end: Vec<bool>,
        phases: Vec<f32>,
        input_b64: String,
        pcm_b64: String,
    }
    #[derive(Deserialize)]
    struct Corpus {
        rows: Vec<Row>,
    }
    fn word(raw: &[u8]) -> u32 {
        u32::from_le_bytes(raw.try_into().unwrap())
    }
    #[test]
    #[ignore = "requires explicit trusted Linux libsoxr 0.1.3"]
    fn material_renderer_matches_frozen_48000_and_96000_pcm() {
        let library =
            std::env::var_os("CUMULUS_PROOF_SOXR_LIBRARY").expect("trusted library path required");
        // SAFETY: opt-in fixture path explicitly selects a trusted SoXR library.
        let host = Arc::new(unsafe { Host::open(Path::new(&library)) }.unwrap());
        let stream = flate2::read::GzDecoder::new(
            &include_bytes!("fixtures/effects-rotation-soxr-official.json.gz")[..],
        );
        let mut raw = Vec::new();
        stream.take(4 * 1024 * 1024).read_to_end(&mut raw).unwrap();
        let corpus: Corpus = serde_json::from_slice(&raw).unwrap();
        let enabled = serde_json::json!({"aev2":{"rotate":{"on":true,"velocity":40.0}}});
        let bypassed = serde_json::json!({"aev2":{"rotate":{"on":false}}});
        let mut counts = Vec::new();
        for row in corpus
            .rows
            .into_iter()
            .filter(|row| row.case == "continuous")
        {
            let input = STANDARD.decode(row.input_b64).unwrap();
            let cap = row.capacity as usize;
            let mut interleaved = Vec::new();
            for block in input[12..].chunks_exact(12 + cap * 8) {
                let count = word(&block[..4]) as usize;
                for i in 0..count {
                    interleaved.extend_from_slice(&block[12 + i * 4..16 + i * 4]);
                    interleaved.extend_from_slice(&block[12 + (cap + i) * 4..16 + (cap + i) * 4]);
                }
            }
            let output = material::render_resampled(
                &enabled,
                response_bytes(),
                &interleaved,
                row.rate,
                host.clone(),
            )
            .unwrap();
            let expected = STANDARD.decode(row.pcm_b64).unwrap();
            assert_eq!(output.len(), expected.len());
            let mut offset = 0;
            for frames in row.valid {
                for frame in 0..frames {
                    for channel in 0..2 {
                        let source = offset + (channel * frames + frame) * 4;
                        let index = offset + (frame * 2 + channel) * 4;
                        let diff = (f32::from_bits(word(&expected[source..source + 4]))
                            - f32::from_bits(word(&output[index..index + 4])))
                        .abs();
                        assert!(
                            diff <= 3e-6,
                            "{} Hz output sample {}: {}",
                            row.rate,
                            index / 4,
                            diff
                        );
                    }
                }
                offset += frames * 8;
            }
            counts.push((row.rate, output.len() / 8));
            assert_eq!(
                material::render_resampled(
                    &bypassed,
                    response_bytes(),
                    &interleaved,
                    row.rate,
                    host.clone()
                )
                .unwrap(),
                interleaved
            );
            assert_eq!(
                material::render(&bypassed, response_bytes(), &interleaved).unwrap(),
                interleaved
            );
        }
        assert_eq!(counts, [(48_000, 4109), (96_000, 3825)]);
    }

    #[test]
    #[ignore = "requires explicit trusted Linux libsoxr 0.1.3"]
    fn host_soxr_stage_matches_frozen_original_frames_end_phase_and_pcm() {
        let library =
            std::env::var_os("CUMULUS_PROOF_SOXR_LIBRARY").expect("trusted library path required");
        // SAFETY: opt-in fixture path explicitly selects a trusted SoXR library.
        let host = Arc::new(unsafe { Host::open(Path::new(&library)) }.unwrap());
        let stream = flate2::read::GzDecoder::new(
            &include_bytes!("fixtures/effects-rotation-soxr-official.json.gz")[..],
        );
        let mut raw = Vec::new();
        stream.take(4 * 1024 * 1024).read_to_end(&mut raw).unwrap();
        let corpus: Corpus = serde_json::from_slice(&raw).unwrap();
        assert_eq!(corpus.rows.len(), 5);
        let mut total = 0;
        let mut peak = 0.0f32;
        for row in corpus.rows {
            let format = Format::new(row.rate as f32, 2, row.capacity).unwrap();
            let mut stage = ResampledRotationStage::new(format, responses(), |rate| {
                Converter::new(host.clone(), rate)
            })
            .unwrap();
            let input = STANDARD.decode(row.input_b64).unwrap();
            let expected = STANDARD.decode(row.pcm_b64).unwrap();
            let stride = 12 + format.sample_count() * 4;
            assert_eq!(input.len(), 12 + stride * row.valid.len());
            let mut block = PlanarBlock::new(format);
            let mut sample = 0;
            for (i, chunk) in input[12..].chunks_exact(stride).enumerate() {
                let frames = word(&chunk[..4]) as usize;
                let period = f32::from_bits(word(&chunk[4..8]));
                let flags = word(&chunk[8..12]);
                for (value, bytes) in block
                    .samples_mut()
                    .iter_mut()
                    .zip(chunk[12..].chunks_exact(4))
                {
                    *value = f32::from_bits(word(bytes));
                }
                if flags & 4 != 0 {
                    stage.reset().unwrap();
                }
                stage.set_period(period).unwrap();
                stage.set_bypass(flags & 1 != 0);
                let got = stage.process(&mut block, frames, flags & 2 != 0).unwrap();
                let context = (row.rate, row.capacity, &row.case, i);
                assert_eq!(
                    (got.valid_frames, got.end),
                    (row.valid[i], row.end[i]),
                    "{context:?}"
                );
                assert_eq!(got.phase.to_bits(), row.phases[i].to_bits(), "{context:?}");
                for c in 0..2 {
                    for &value in &block.samples()
                        [c * format.capacity()..c * format.capacity() + got.valid_frames]
                    {
                        let target = f32::from_bits(word(&expected[sample * 4..(sample + 1) * 4]));
                        let diff = (value - target).abs();
                        assert!(
                            diff <= 3e-6,
                            "{context:?} sample {sample}: {value} vs {target}"
                        );
                        peak = peak.max(diff);
                        sample += 1;
                    }
                }
            }
            assert_eq!(sample * 4, expected.len());
            total += sample;
        }
        assert_eq!(total, 18_454);
        println!("{total} PCM samples, maximum absolute delta {peak}");
    }
}
