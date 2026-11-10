use super::*;
use crate::effects::dsp::{LimiterStream, LimiterStreamQuantum, LimiterStreamWrite};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const QUANTUM: usize = 2048;

fn legacy(gain: f32, bass: i32, presence: i32) -> LocalEqualizerMaterialPlan {
    LocalEqualizerMaterialPlan::from_equalizer_json(&json!({
        "eq": {"on":gain != 0.0,"eqs":[0.0,gain,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0]},
        "bt": {"on":bass != 0,"bass":bass,"treble":0},
        "se": {"on":presence != 0,"presence":presence,"stereoizer":0,"sshaper":true,"ambience":1},
        "rvb": {"on":false}
    }))
    .unwrap()
}

fn modern() -> ModernLimiterPlan {
    let vector: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/effects-modern-limiter-graph-official.json"
    )))
    .unwrap();
    ModernLimiterPlan::from_material(&vector["profiles"]["neutralTone"]).unwrap()
}

fn synth(first: usize, count: usize) -> Vec<f32> {
    (first..first + count)
        .flat_map(|i| {
            [
                if i.is_multiple_of(27) {
                    0.875
                } else {
                    (((i * 37) % 256) as f32 - 128.0) / 512.0
                },
                if i.is_multiple_of(29) {
                    -0.625
                } else {
                    (((i * 53) % 128) as f32 - 64.0) / 512.0
                },
            ]
        })
        .collect()
}

fn hash(pcm: &[f32]) -> String {
    let mut digest = Sha256::new();
    for sample in pcm {
        digest.update(sample.to_le_bytes());
    }
    hex::encode(digest.finalize())
}

fn push(
    stream: &mut LimiterStream,
    stage: &mut LocalFixedStage,
    first: usize,
    count: usize,
    timestamp: Option<u32>,
) -> LimiterStreamWrite {
    push_observed(stream, stage, first, count, timestamp, |_| {})
}

fn push_observed(
    stream: &mut LimiterStream,
    stage: &mut LocalFixedStage,
    first: usize,
    count: usize,
    timestamp: Option<u32>,
    observe: impl FnMut(LimiterStreamQuantum),
) -> LimiterStreamWrite {
    stream
        .push_interleaved_graph(
            &synth(first, count),
            timestamp,
            |limiter, block, frames, end| {
                let cap = block.format().capacity();
                for channel in block.samples_mut().chunks_exact_mut(cap) {
                    channel[frames as usize..].fill(0.0);
                }
                let routed = stage.process(block, frames as usize, end)?;
                assert_eq!(
                    (routed.valid_frames, routed.end),
                    (frames as usize, end),
                    "settled dry spatial scope"
                );
                limiter.process(block, frames, end)
            },
            observe,
        )
        .unwrap()
}

fn pull(stream: &mut LimiterStream, max: usize) -> Vec<f32> {
    pull_with_end(stream, max, false)
}

fn pull_with_end(stream: &mut LimiterStream, max: usize, end: bool) -> Vec<f32> {
    let mut pcm = vec![0.0; max * 2];
    let receipt = stream.pull_interleaved(&mut pcm).unwrap();
    assert_eq!(receipt.end, Some(end));
    pcm.truncate(receipt.frames as usize * 2);
    pcm
}

#[test]
fn initial_modern_matches_original_hot_pcm_and_preserves_both_fifos() {
    let oracle: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/effects-modern-limiter-initial-transition-official.json"
    )))
    .unwrap();
    let plan = modern();
    for case in oracle["cases"].as_array().unwrap() {
        let rate = case["rate"].as_u64().unwrap() as f32;
        let format = Format::new(rate, 2, QUANTUM as u32).unwrap();
        let local = LocalEqualizerStereoMaterial::new(format, legacy(3.0, 0, 0)).unwrap();
        let LocalFixedStereoMaterial {
            mut stage, limiter, ..
        } = local.graph;
        let mut stream = limiter.into_stream(8192).unwrap();
        push(&mut stream, &mut stage, 0, QUANTUM, Some(1000));
        let mut old = pull(&mut stream, 31);
        let queued = push(&mut stream, &mut stage, QUANTUM, 511, None);
        assert_eq!(queued.input_ready, 511);
        assert!(queued.clock_ms.unwrap() > 1000);
        let new_limiter = plan
            .install_on_settled_local(&mut stage, format)
            .unwrap()
            .unwrap();
        stream.replace_stage_for_setup(new_limiter);
        let more_old = pull(&mut stream, QUANTUM);
        assert_eq!(more_old.len() as u32 / 2, queued.output_ready);
        old.extend(more_old);
        assert_eq!(
            old.len() as u64 / 2,
            case["alreadyProducedFrames"].as_u64().unwrap()
        );
        assert_eq!(
            hash(&old),
            case["queuedOldPcmSha256"].as_str().unwrap(),
            "{rate} Hz old PCM"
        );

        let accepted = push(&mut stream, &mut stage, QUANTUM + 511, QUANTUM - 511, None);
        assert_eq!(accepted.input_ready, 0);
        assert!(accepted.clock_ms > queued.clock_ms);
        let hot = pull(&mut stream, QUANTUM);
        assert_eq!(
            hot.len() as u64 / 2,
            case["newFramesBoth"].as_u64().unwrap()
        );
        assert_eq!(
            hash(&hot),
            case["hotNewPcmSha256"].as_str().unwrap(),
            "{rate} Hz hot PCM"
        );

        // The held worker owns the input quanta and FIFO itself. Its direct
        // block API must make the same change as the independent FIFO oracle.
        let mut held = LocalEqualizerStereoMaterial::new(format, legacy(3.0, 0, 0)).unwrap();
        let mut first_block = block(format, 0);
        let first = held.process(&mut first_block, QUANTUM, false).unwrap();
        assert_eq!(interleaved(&first_block, first.valid_frames), old);
        held.apply_initial_modern(&plan).unwrap();
        let mut next_block = block(format, QUANTUM);
        let next = held.process(&mut next_block, QUANTUM, false).unwrap();
        assert_eq!(interleaved(&next_block, next.valid_frames), hot);

        let mut fresh = plan.pcm_renderer(format, 8192).unwrap();
        fresh
            .push_interleaved(&synth(QUANTUM, QUANTUM), None)
            .unwrap();
        let mut pcm = vec![0.0; QUANTUM * 2];
        let read = fresh.pull_interleaved(&mut pcm).unwrap();
        assert_eq!(read.frames as usize * 2, hot.len());
        pcm.truncate(hot.len());
        assert_eq!(
            hash(&pcm),
            case["freshNewPcmSha256"].as_str().unwrap(),
            "{rate} Hz fresh control"
        );
        assert_eq!(
            hot.iter()
                .zip(&pcm)
                .filter(|(a, b)| a.to_bits() != b.to_bits())
                .count() as u64,
            case["sameFrameDifferentSampleWords"].as_u64().unwrap()
        );
    }
}

fn block(format: Format, first: usize) -> PlanarBlock {
    let mut block = PlanarBlock::new(format);
    for (index, pair) in synth(first, QUANTUM).chunks_exact(2).enumerate() {
        block.samples_mut()[index] = pair[0];
        block.samples_mut()[QUANTUM + index] = pair[1];
    }
    block
}

fn interleaved(block: &PlanarBlock, count: usize) -> Vec<f32> {
    let (left, right) = block.samples().split_at(QUANTUM);
    (0..count).flat_map(|i| [left[i], right[i]]).collect()
}

#[test]
fn initial_modern_rejects_unsupported_or_unsettled_legacy_without_mutating() {
    let plan = modern();
    let format = Format::new(48_000.0, 2, QUANTUM as u32).unwrap();
    let old = legacy(3.0, 0, 0);
    let mut local = LocalEqualizerStereoMaterial::new(format, old).unwrap();
    let mut unchanged = LocalEqualizerStereoMaterial::new(format, old).unwrap();
    assert!(
        local.apply_initial_modern(&plan).is_err(),
        "pending GEQ activation"
    );
    local.reset().unwrap();
    unchanged.reset().unwrap();
    assert!(
        local.apply_initial_modern(&plan).is_err(),
        "reset without PCM"
    );
    let mut sample = block(format, 0);
    let mut expected = block(format, 0);
    local.process(&mut sample, QUANTUM, false).unwrap();
    unchanged.process(&mut expected, QUANTUM, false).unwrap();
    assert_eq!(sample.samples(), expected.samples());

    local.apply_initial_modern(&plan).unwrap();
    assert!(local.apply_initial_modern(&plan).is_err());
    assert!(local.apply_material(old).is_err());
    assert!(local.disable_master().is_err());
    for old in [legacy(3.0, 1, 0), legacy(3.0, 0, 1), legacy(0.0, 0, 0)] {
        let mut local = LocalEqualizerStereoMaterial::new(format, old).unwrap();
        local
            .process(&mut block(format, 0), QUANTUM, false)
            .unwrap();
        assert!(local.apply_initial_modern(&plan).is_err());
        local.apply_material(legacy(3.0, 0, 0)).unwrap();
        local
            .process(&mut block(format, QUANTUM), QUANTUM, false)
            .unwrap();
        assert!(
            local.apply_initial_modern(&plan).is_err(),
            "previously outside the GEQ-only baseline"
        );
    }

    let mut ended = LocalEqualizerStereoMaterial::new(format, old).unwrap();
    ended.process(&mut block(format, 0), QUANTUM, true).unwrap();
    assert!(
        ended.apply_initial_modern(&plan).is_err(),
        "source already ending"
    );
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedCorpus {
    schema: String,
    sdk_sha256: String,
    cases: Vec<SharedCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SharedCase {
    profile: String,
    rate: u32,
    mode: SharedMode,
    inputs: Vec<u32>,
    outputs: Vec<u32>,
    frames: [usize; 4],
    sha256: [String; 4],
    native_b0_setups: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum SharedMode {
    Steady,
    SettledOff,
    Coalesced,
    InitialCoalesced,
    CloseDry,
    DirectLegacy,
    CloseThenLegacyCoalesced,
    CloseThenLegacySettled,
}

#[test]
fn shared_modern_controls_match_original_pcm_with_and_without_an_off_quantum() {
    compare_shared_corpus(
        include_str!(
            "../../../tests/fixtures/effects-modern-limiter-shared-controls-official.json"
        ),
        "cumulus.modern-limiter-x64-shared-controls.v1",
    );
}

#[test]
fn shared_modern_legacy_return_matches_original_direct_close_and_settled_cases() {
    compare_shared_corpus(
        include_str!("../../../tests/fixtures/effects-modern-limiter-legacy-return-official.json"),
        "cumulus.modern-limiter-x64-legacy-return.v1",
    );
}

enum Change {
    Off,
    Reapply,
    Legacy,
}

fn compare_shared_corpus(document: &str, schema: &str) {
    let corpus: SharedCorpus = serde_json::from_str(document).unwrap();
    let profiles: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/effects-modern-limiter-graph-official.json"
    )))
    .unwrap();
    assert_eq!(corpus.schema, schema);
    assert_eq!(corpus.sdk_sha256, profiles["sdkSha256"].as_str().unwrap());
    assert_eq!(corpus.cases.len(), 24);

    for case in corpus.cases {
        let label = (&case.profile, case.rate, case.mode);
        let mut profile = profiles["profiles"][&case.profile].clone();
        if case.profile == "defaultTiming" {
            profile = profiles["profiles"]["nonzeroTone"].clone();
            profile["aev2"]["limiter"] = json!({
                "on":true,"threshold":0,"attack":0.005,"release":0.1,
                "outputgain":0,"infiniteRatio":true,"ratio":4
            });
        }
        let plan = ModernLimiterPlan::from_material(&profile).unwrap();
        let format = Format::new(case.rate as f32, 2, QUANTUM as u32).unwrap();
        let local = LocalEqualizerStereoMaterial::new(format, legacy(3.0, 0, 0)).unwrap();
        let LocalFixedStereoMaterial {
            mut stage, limiter, ..
        } = local.graph;
        let mut stream = limiter.into_stream(8192).unwrap();
        let mut quanta = Vec::new();
        let mut push =
            |stream: &mut LimiterStream, stage: &mut LocalFixedStage, first, count, timestamp| {
                push_observed(stream, stage, first, count, timestamp, |q| quanta.push(q))
            };
        let mut setups = 1;
        let mut material: Option<ModernLocalMaterial> = None;
        let mut pcm_segments = Vec::new();
        push(&mut stream, &mut stage, 0, QUANTUM, Some(1000));
        for (i, pending) in [511, 257, 1023].into_iter().enumerate() {
            let first = (i + 1) * QUANTUM;
            let mut pcm = pull(&mut stream, 31);
            let queued = push(&mut stream, &mut stage, first, pending, None);
            assert_eq!(queued.input_ready as usize, pending);
            assert_eq!(queued.output_ready as usize, case.frames[i] - 31);
            let mut new_b0 = None;
            if i == 0 {
                let (new, b0) = ModernLocalMaterial::install(&plan, &mut stage, format).unwrap();
                material = Some(new);
                new_b0 = b0;
            }
            use Change::{Legacy, Off, Reapply};
            let switches: &[Change] = match (i, case.mode) {
                (0, SharedMode::InitialCoalesced) | (1, SharedMode::Coalesced) => &[Off, Reapply],
                (
                    1,
                    SharedMode::SettledOff
                    | SharedMode::CloseDry
                    | SharedMode::CloseThenLegacySettled,
                ) => &[Off],
                (2, SharedMode::SettledOff) => &[Reapply],
                (1, SharedMode::DirectLegacy) | (2, SharedMode::CloseThenLegacySettled) => {
                    &[Legacy]
                }
                (1, SharedMode::CloseThenLegacyCoalesced) => &[Off, Legacy],
                _ => &[],
            };
            for switch in switches {
                let replacement = match switch {
                    Off | Reapply => material.as_mut().unwrap().set_active(
                        &mut stage,
                        format,
                        matches!(switch, Reapply),
                    ),
                    Legacy => material.take().unwrap().return_legacy_geq(
                        &mut stage,
                        format,
                        &[0.0, 0.0, -4.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    ),
                }
                .unwrap();
                if replacement.is_some() {
                    new_b0 = replacement;
                }
            }
            // More than one setup request before the next quantum coalesces
            // inside B0. Its old pending PCM is already invalidated by the
            // first timing change; only the final settings reach Execute.
            if let Some(b0) = new_b0 {
                setups += 1;
                stream.replace_stage_for_setup(b0);
            }
            pcm.extend(pull(&mut stream, QUANTUM));
            pcm_segments.push(pcm);
            push(
                &mut stream,
                &mut stage,
                first + pending,
                QUANTUM - pending,
                None,
            );
        }
        let mut tail = pull(&mut stream, QUANTUM);
        push(&mut stream, &mut stage, 4 * QUANTUM, 7, None);
        push(&mut stream, &mut stage, 4 * QUANTUM + 7, 0, None);
        tail.extend(pull_with_end(&mut stream, QUANTUM, true));
        assert!(pull_with_end(&mut stream, QUANTUM, true).is_empty());
        pcm_segments.push(tail);

        assert_eq!(setups, case.native_b0_setups, "{label:?} B0 setups");
        assert_eq!(
            quanta.iter().map(|q| q.input_frames).collect::<Vec<_>>(),
            case.inputs,
            "{label:?} source quanta"
        );
        assert_eq!(
            quanta
                .iter()
                .map(|q| q.timing.output_frames)
                .collect::<Vec<_>>(),
            case.outputs,
            "{label:?} output quanta"
        );
        assert_eq!(
            quanta.iter().map(|q| q.timestamp_ms).collect::<Vec<_>>(),
            if case.rate == 44_100 {
                [1000, 1046, 1092, 1138, 1184]
            } else {
                [1000, 1042, 1084, 1126, 1168]
            },
            "{label:?} source clock"
        );
        for (index, pcm) in pcm_segments.into_iter().enumerate() {
            assert_eq!(
                pcm.len() / 2,
                case.frames[index],
                "{label:?} segment {index}"
            );
            assert_eq!(hash(&pcm), case.sha256[index], "{label:?} segment {index}");
        }
    }
}
