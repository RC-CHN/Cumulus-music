use super::*;
use crate::effects::dsp::{LimiterStream, LimiterStreamWrite};
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
            |_| {},
        )
        .unwrap()
}

fn pull(stream: &mut LimiterStream, max: usize) -> Vec<f32> {
    let mut pcm = vec![0.0; max * 2];
    let receipt = stream.pull_interleaved(&mut pcm).unwrap();
    assert_eq!(receipt.end, Some(false));
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
