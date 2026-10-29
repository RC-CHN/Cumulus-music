use super::*;
use cumulus_core::effects::dsp::{LoudnessParameters, LoudnessStage};

struct Loudness(u32);
impl Loudness {
    fn new(enabled: bool, gain: f32) -> Self {
        let mut id = 0;
        assert_eq!(
            unsafe { cumulus_loudness_create_v1(gain, enabled.into(), &mut id) },
            0
        );
        Self(id)
    }
    fn set(&self, enabled: bool, gain: f32) {
        assert_eq!(cumulus_loudness_update_v1(self.0, gain, enabled.into()), 0);
    }
    fn stats(&self) -> EqualizerStats {
        let mut value = EqualizerStats::default();
        assert_eq!(unsafe { cumulus_loudness_stats_v1(self.0, &mut value) }, 0);
        value
    }
}
impl Drop for Loudness {
    fn drop(&mut self) {
        cumulus_loudness_release_v1(self.0);
    }
}

#[test]
fn loudness_native_callbacks_match_transitions_for_every_block_and_seek_reset() {
    let config = Loudness::new(false, -18.0);
    let mut instance = Instance::new_kind(config.0, 48000, 1);
    let format = Format::new(48000.0, 1, CAPACITY).unwrap();
    let mut stage = LoudnessStage::new(format);
    stage.set_bypass(true).unwrap();
    stage.reset();
    let mut block = PlanarBlock::new(format);
    for (enabled, gain, count) in [
        (false, -18.0, 13),
        (true, -18.0, 8197),
        (false, -18.0, 13),
        (true, -18.0, 2048),
        (true, 6.0, 1),
        (false, 6.0, 0),
        (false, 6.0, 8193),
    ] {
        config.set(enabled, gain);
        // No callback reads configuration when its frame count is zero.
        if count == 0 {
            assert!(instance.process(&[], false).is_empty());
            continue;
        }
        stage.set_bypass(!enabled).unwrap();
        if enabled {
            stage
                .set_parameters(LoudnessParameters {
                    gain_db: gain,
                    peak: 0.0,
                })
                .unwrap();
        }
        let pcm = input(count);
        let mut expected = Vec::new();
        for part in pcm.chunks(CAPACITY as usize) {
            block.load_interleaved(part).unwrap();
            stage.process(&mut block, part.len()).unwrap();
            expected.extend_from_slice(&block.samples()[..part.len()]);
        }
        assert_eq!(instance.process(&pcm, count % 2 == 1), expected);
        assert_eq!(config.stats().revision, config.stats().observed_revision);
    }
    instance.reset();
    assert_eq!(instance.process(&input(13), true), input(13));
    let stats = config.stats();
    assert_eq!(stats.failed_blocks, 0);
    assert_eq!(stats.active_instances, 1);
    assert_eq!(stats.instances_created, 1);
}

#[test]
fn loudness_handles_reject_other_filter_settings_and_bad_updates_atomically() {
    let loud = Loudness::new(true, -12.0);
    let geq = Config::new(true, [3.0; 10]);
    let initial = loud.stats().revision;
    assert_eq!(cumulus_loudness_update_v1(geq.0, 0.0, 1), -2);
    assert_eq!(
        unsafe { cumulus_geq_update_v1(loud.0, [1.0; 10].as_ptr(), 1) },
        -2
    );
    assert_eq!(cumulus_loudness_update_v1(loud.0, f32::NAN, 1), -2);
    assert_eq!(cumulus_loudness_update_v1(loud.0, f32::MAX, 1), -2);
    assert_eq!(cumulus_loudness_update_v1(loud.0, 0.0, 2), -2);
    assert_eq!(loud.stats().revision, initial);
    let mut wrong = Instance::new_kind(geq.0, 48000, 1);
    assert_eq!(wrong.process(&input(5), true), [0.0; 5]);
    assert_eq!(geq.stats().failed_blocks, 1);
    let mut right = Instance::new_kind(loud.0, 48000, 1);
    assert!(right.process(&input(13), false).iter().any(|v| *v != 0.0));
    assert_eq!(loud.stats().failed_blocks, 0);
    drop(loud);
    assert_eq!(right.process(&input(5), false), [0.0; 5]);
    assert_eq!(
        unsafe { cumulus_loudness_create_v1(0.0, 1, std::ptr::null_mut()) },
        -2
    );
}
