use super::*;
use cumulus_core::effects::{
    dsp::{Format, GraphicEq, PlanarBlock},
    equalizer::{
        EqualizerSettings,
        abi::{CAPACITY, EqualizerStats},
    },
};
use std::ffi::c_void;

struct Instance {
    handle: *mut c_void,
    token: Box<f32>,
}
impl Instance {
    fn new(id: u32, rate: u32) -> Self {
        let mut token = Box::new(id as f32);
        // SAFETY: valid descriptor; instance is uniquely owned until Drop.
        let handle = unsafe {
            let d = &*ladspa_descriptor(0);
            let h = d.instantiate.unwrap()(d, rate.into());
            assert!(!h.is_null());
            d.connect_port.unwrap()(h, 2, &mut *token);
            d.activate.unwrap()(h);
            h
        };
        Self { handle, token }
    }
    fn process(&mut self, input: &[f32], inplace: bool) -> Vec<f32> {
        // Canary at both ends checks bounds without making input/output aliases
        // into simultaneous Rust slices; callbacks only receive raw pointers.
        let mut output = vec![12345.0; input.len() + 2];
        let out = unsafe { output.as_mut_ptr().add(1) };
        if inplace {
            output[1..input.len() + 1].copy_from_slice(input);
        }
        unsafe {
            let d = &*ladspa_descriptor(0);
            d.connect_port.unwrap()(
                self.handle,
                0,
                if inplace {
                    out
                } else {
                    input.as_ptr().cast_mut()
                },
            );
            d.connect_port.unwrap()(self.handle, 1, out);
            d.run.unwrap()(self.handle, input.len() as _);
        }
        assert_eq!(output[0], 12345.0);
        assert_eq!(output[input.len() + 1], 12345.0);
        output[1..input.len() + 1].to_vec()
    }
    fn reset(&mut self) {
        unsafe { (&*ladspa_descriptor(0)).activate.unwrap()(self.handle) }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe { (&*ladspa_descriptor(0)).cleanup.unwrap()(self.handle) }
    }
}
struct Config(u32);
impl Config {
    fn new(enabled: bool, gains: [f32; 10]) -> Self {
        let mut id = 0;
        assert_eq!(
            unsafe { cumulus_geq_create_v1(gains.as_ptr(), enabled.into(), &mut id) },
            0
        );
        Self(id)
    }
    fn set(&self, enabled: bool, gains: [f32; 10]) {
        assert_eq!(
            unsafe { cumulus_geq_update_v1(self.0, gains.as_ptr(), enabled.into()) },
            0
        );
    }
    fn stats(&self) -> EqualizerStats {
        let mut value = EqualizerStats::default();
        assert_eq!(unsafe { cumulus_geq_stats_v1(self.0, &mut value) }, 0);
        value
    }
}
impl Drop for Config {
    fn drop(&mut self) {
        cumulus_geq_release_v1(self.0);
    }
}
fn input(count: usize) -> Vec<f32> {
    (0..count)
        .map(|n| ((n as f32 * 0.13).sin() + (n as f32 * 0.023).cos()) * 0.03)
        .collect()
}
fn expected(stage: &mut GraphicEq, input: &[f32]) -> Vec<f32> {
    let mut result = Vec::new();
    let mut block = PlanarBlock::new(Format::new(48000.0, 1, CAPACITY).unwrap());
    for part in input.chunks(CAPACITY as usize) {
        block.load_interleaved(part).unwrap();
        stage.process(&mut block, part.len()).unwrap();
        result.extend_from_slice(&block.samples()[..part.len()]);
    }
    result
}

#[test]
fn callback_matches_recovered_dsp_with_inplace_partial_and_large_blocks() {
    let c = Config::new(true, [3.0, -2.0, 1.0, 0.0, -4.0, 5.0, 1.0, 0.0, -2.0, 3.0]);
    let mut instance = Instance::new(c.0, 48000);
    let mut reference = GraphicEq::new(Format::new(48000.0, 1, CAPACITY).unwrap()).unwrap();
    reference
        .set_gains(&[3.0, -2.0, 1.0, 0.0, -4.0, 5.0, 1.0, 0.0, -2.0, 3.0])
        .unwrap();
    for size in [0, 1, 13, 2048, 8197, 5, 4096] {
        let pcm = input(size);
        assert_eq!(
            instance.process(&pcm, size % 2 == 1),
            expected(&mut reference, &pcm)
        );
    }
    let s = c.stats();
    assert_eq!(s.active_instances, 1);
    assert_eq!(s.failed_blocks, 0);
    assert_eq!(s.processed_samples, 14360);
    drop(instance);
    assert_eq!(c.stats().active_instances, 0);
}
#[test]
fn hot_changes_bypass_and_reactivation_preserve_recovered_transitions() {
    let c = Config::new(true, [0.0; 10]);
    let mut instance = Instance::new(c.0, 48000);
    let mut reference = GraphicEq::new(Format::new(48000.0, 1, CAPACITY).unwrap()).unwrap();
    for (enabled, gain) in [
        (true, 6.0),
        (false, 6.0),
        (false, 6.0),
        (true, -3.0),
        (true, 2.0),
    ] {
        let mut gains = [0.0; 10];
        gains[6] = gain;
        c.set(enabled, gains);
        reference.set_gains(&gains).unwrap();
        reference.set_bypass(!enabled).unwrap();
        let pcm = input(2053);
        assert_eq!(
            instance.process(&pcm, false),
            expected(&mut reference, &pcm)
        );
        assert_eq!(c.stats().observed_revision, c.stats().revision);
    }
    instance.reset();
    let mut fresh = Instance::new(c.0, 48000);
    assert_eq!(
        instance.process(&input(9000), false),
        fresh.process(&input(9000), true)
    );
}
#[test]
fn configuration_isolation_revocation_and_no_id_reuse() {
    let a = Config::new(true, [7.0; 10]);
    let b = Config::new(true, [-5.0; 10]);
    let mut ai = Instance::new(a.0, 48000);
    let mut bi = Instance::new(b.0, 48000);
    assert_ne!(
        ai.process(&input(3000), false),
        bi.process(&input(3000), false)
    );
    let before = b.stats().processed_samples;
    assert_eq!(cumulus_geq_release_v1(a.0), 0);
    assert_eq!(ai.process(&input(3000), false), vec![0.0; 3000]);
    assert!(bi.process(&input(3000), false).iter().any(|x| *x != 0.0));
    assert_eq!(b.stats().processed_samples, before + 3000);
    let next = Config::new(true, [0.0; 10]);
    assert!(next.0 > a.0);
    assert_ne!(next.0, b.0);
}
#[test]
fn busy_control_never_blocks_callback_and_defers_whole_settings() {
    let c = Config::new(true, [0.0; 10]);
    let mut instance = Instance::new(c.0, 48000);
    instance.process(&input(256), false);
    let control = control::registry().lock().unwrap().entries[&c.0].clone();
    let guard = control.config.lock().unwrap();
    let before = c.0;
    // If run ever blocks on the configuration mutex, this test deadlocks.
    assert!(
        instance
            .process(&input(256), true)
            .iter()
            .all(|x| x.is_finite())
    );
    assert_eq!(before, c.0);
    drop(guard);
    assert_eq!(c.stats().processed_samples, 512);
}
#[test]
fn first_callback_after_activation_uses_prepared_config_while_both_locks_are_busy() {
    let gains = [3.0, -2.0, 1.0, 0.0, -4.0, 5.0, 1.0, 0.0, -2.0, 3.0];
    let c = Config::new(true, gains);
    let mut instance = Instance::new(c.0, 48000);
    // Include reactivation after seek, which resets the DSP and its revision.
    for reactivate in [false, true] {
        if reactivate {
            instance.reset();
        }
        let mut reference = GraphicEq::new(Format::new(48000.0, 1, CAPACITY).unwrap()).unwrap();
        reference.set_gains(&gains).unwrap();
        let pcm = input(2053);
        let expected = expected(&mut reference, &pcm);
        let registry_guard = control::registry().lock().unwrap();
        let control = registry_guard.entries[&c.0].clone();
        let config_guard = control.config.lock().unwrap();
        // No preceding run primes the instance. The callback must neither
        // block on either mutex nor replace valid audio with silence.
        let actual = instance.process(&pcm, true);
        drop(config_guard);
        drop(registry_guard);
        assert_eq!(actual, expected);
    }
    assert_eq!(c.stats().failed_blocks, 0);
    assert_eq!(c.stats().processed_samples, 4106);
}

#[test]
fn nonfinite_input_quarantines_until_activation_and_bad_config_is_atomic() {
    let c = Config::new(true, [0.0; 10]);
    let mut instance = Instance::new(c.0, 48000);
    assert_eq!(instance.process(&[f32::NAN; 16], true), vec![0.0; 16]);
    assert_eq!(instance.process(&input(16), false), vec![0.0; 16]);
    instance.reset();
    assert!(
        instance
            .process(&input(16), false)
            .iter()
            .any(|x| *x != 0.0)
    );
    let old = c.stats().revision;
    for gain in [f32::NAN, f32::INFINITY, 12.001, -12.001] {
        assert_ne!(
            unsafe { cumulus_geq_update_v1(c.0, [gain; 10].as_ptr(), 1) },
            0
        );
    }
    assert_ne!(
        unsafe { cumulus_geq_update_v1(c.0, [0.0; 10].as_ptr(), 2) },
        0
    );
    assert_eq!(c.stats().revision, old);
}
#[test]
fn abi_rejects_unsupported_rate_invalid_token_and_null_arguments() {
    unsafe {
        let d = &*ladspa_descriptor(0);
        assert!(ladspa_descriptor(1).is_null());
        assert!(d.instantiate.unwrap()(d, 7999).is_null());
        assert!(d.instantiate.unwrap()(d, 384001).is_null());
        assert_ne!(
            cumulus_geq_create_v1(std::ptr::null(), 1, std::ptr::null_mut()),
            0
        );
        assert_ne!(cumulus_geq_stats_v1(0, std::ptr::null_mut()), 0);
    }
    let mut instance = Instance::new(0, 48000);
    for value in [0.0, -1.0, f32::NAN, 1.5, 16_777_216.0] {
        *instance.token = value;
        assert_eq!(instance.process(&input(16), false), vec![0.0; 16]);
    }
}
#[test]
fn settings_deserialization_cannot_bypass_official_slider_bounds() {
    for gains in [[0.0; 10], [12.0; 10], [-12.0; 10]] {
        let s = EqualizerSettings::new(true, gains).unwrap();
        assert_eq!(
            serde_json::from_str::<EqualizerSettings>(&serde_json::to_string(&s).unwrap()).unwrap(),
            s
        );
    }
    assert!(
        serde_json::from_str::<EqualizerSettings>(
            r#"{"enabled":true,"gains":[13,0,0,0,0,0,0,0,0,0]}"#
        )
        .is_err()
    );
    assert!(serde_json::from_str::<EqualizerSettings>(r#"{"enabled":true,"gains":[0]}"#).is_err());
}

#[test]
fn disabled_start_and_seek_reset_are_dry_from_the_first_sample() {
    let c = Config::new(false, [12.0; 10]);
    let mut instance = Instance::new(c.0, 48000);
    for size in [1, 13, 2048, 8197] {
        let pcm = input(size);
        assert_eq!(instance.process(&pcm, true), pcm);
    }
    instance.reset();
    let pcm = input(3000);
    assert_eq!(instance.process(&pcm, false), pcm);
    let mut new_instance = Instance::new(c.0, 48000);
    assert_eq!(new_instance.process(&pcm, false), pcm);
    assert_eq!(c.stats().failed_blocks, 0);
}
