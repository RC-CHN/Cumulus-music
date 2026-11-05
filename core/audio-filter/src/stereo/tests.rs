use super::*;
use cumulus_core::effects::{dsp::*, stereo::abi};
use std::ffi::c_void;

fn config(enabled: bool) -> u32 {
    let mut id = 0;
    // SAFETY: writable scalar output, no retained pointer.
    assert_eq!(
        unsafe { cumulus_se1_create_v1(0.0, 5.0, 1, enabled.into(), &mut id) },
        0
    );
    id
}
fn stats(id: u32) -> abi::StereoStats {
    let mut value = abi::StereoStats::default();
    // SAFETY: exact writable layout.
    assert_eq!(unsafe { cumulus_se1_stats_v1(id, &mut value) }, 0);
    value
}
struct Instance(*mut c_void);
impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: unique owned handle, after all test calls finished.
        unsafe { cumulus_se1_destroy_v1(self.0) };
    }
}
fn instance(id: u32) -> Instance {
    let mut raw = std::ptr::null_mut();
    // SAFETY: exact writable scalar, synthetic fixed stereo format.
    assert_eq!(
        unsafe { cumulus_se1_instantiate_v1(id, 48000, &mut raw) },
        0
    );
    assert!(!raw.is_null());
    Instance(raw)
}
fn pcm(format: Format, index: usize) -> PlanarBlock {
    let mut pcm = PlanarBlock::new(format);
    for (i, x) in pcm.samples_mut().iter_mut().enumerate() {
        *x = ((i + index) % 31) as f32 * 0.0078125 - 0.125;
    }
    pcm
}
#[test]
fn variable_frame_abi_matches_real_core_and_keeps_initial_bypass_reset_and_tail() {
    let id = config(false);
    let native = instance(id);
    let initial = stats(id);
    assert_eq!(initial.input_frames, 0);
    assert_eq!(initial.output_frames, 0);
    assert_eq!(initial.observed_revision, 0);
    let format = Format::new(48000.0, 2, abi::CAPACITY).unwrap();
    let mut reference = StereoAmbienceOneStage::new(format).unwrap();
    reference
        .set_controls(StereoAmbienceOneControls {
            presence: 0.0,
            stereoizer: 5.0,
            spectrum_shaper: true,
        })
        .unwrap();
    reference.set_bypass(true).unwrap();
    let mut nonzero_tail = false;
    for index in 0..46 {
        let frames = if index >= 20 {
            0
        } else {
            [2048, 2047, 1, 0, 3, 7][index % 6]
        };
        let end = index >= 20;
        let enabled = index >= 2 && index != 6 && index != 7;
        assert_eq!(cumulus_se1_update_v1(id, 0.0, 5.0, 1, enabled.into()), 0);
        reference.set_bypass(!enabled).unwrap();
        if index == 11 {
            // SAFETY: live unique handle, original reset, no fake samples.
            assert_eq!(unsafe { cumulus_se1_reset_v1(native.0) }, 0);
            reference.reset().unwrap();
        }
        let mut a = pcm(format, index);
        if end {
            a.samples_mut().fill(0.0);
        }
        let mut b = PlanarBlock::new(format);
        b.samples_mut().copy_from_slice(a.samples());
        let expected = reference.process(&mut b, frames, end).unwrap();
        let mut result = abi::BlockResult::default();
        // SAFETY: complete initialized exclusive 2*CAPACITY planar allocation
        // and disjoint writable result, even for zero input audio frames.
        assert_eq!(
            unsafe {
                cumulus_se1_process_v1(
                    native.0,
                    a.samples_mut().as_mut_ptr(),
                    frames as u32,
                    end.into(),
                    &mut result,
                )
            },
            0
        );
        assert_eq!(
            (result.frames as usize, result.end != 0),
            (expected.valid_frames, expected.end)
        );
        assert_eq!(a.samples(), b.samples(), "native ABI block{index}");
        nonzero_tail |= end
            && a.samples()[..result.frames as usize]
                .iter()
                .any(|x| x.abs() > 0.000001);
    }
    assert!(nonzero_tail);
    assert_eq!(stats(id).failed_blocks, 0);
    assert_eq!(stats(id).instances_created, 1);
    assert_eq!(stats(id).resets, 1);
    assert_eq!(cumulus_se1_release_v1(id), 0);
    let mut block = PlanarBlock::new(format);
    block.samples_mut().fill(0.125);
    let mut result = abi::BlockResult::default();
    // SAFETY: same complete live buffers; revoked configuration must mute.
    assert_ne!(
        unsafe {
            cumulus_se1_process_v1(
                native.0,
                block.samples_mut().as_mut_ptr(),
                64,
                0,
                &mut result,
            )
        },
        0
    );
    assert!(block.samples().iter().all(|x| *x == 0.0));
    // SAFETY: live but quarantined instance; reset must not revive it.
    assert_ne!(unsafe { cumulus_se1_reset_v1(native.0) }, 0);
}
#[test]
fn five_ports_keep_stereo_linked_and_reactivation_is_original_reset() {
    let id = config(true);
    let mut control = id as f32;
    // SAFETY: immutable static descriptor from this same library.
    let descriptor = unsafe { &*crate::ladspa_descriptor(3) };
    assert_eq!(descriptor.port_count, 5);
    // SAFETY: immutable descriptor owns exactly its declared static port array.
    assert_eq!(
        unsafe { std::slice::from_raw_parts(descriptor.port_descriptors, 5) },
        [9, 9, 10, 10, 5]
    );
    // SAFETY: valid descriptor/rate, one owned handle until cleanup below.
    let handle = unsafe { descriptor.instantiate.unwrap()(descriptor, 48000) };
    assert!(!handle.is_null());
    let cap = abi::CAPACITY as usize;
    let mut left = vec![0.0; cap];
    let mut right = vec![0.0; cap];
    for i in 0..cap {
        left[i] = (i % 17) as f32 * 0.0078125;
        right[i] = -((i % 7) as f32) * 0.015625;
    }
    let format = Format::new(48000.0, 2, abi::CAPACITY).unwrap();
    let mut expected = StereoAmbienceOneStage::new(format).unwrap();
    expected
        .set_controls(StereoAmbienceOneControls {
            presence: 0.0,
            stereoizer: 5.0,
            spectrum_shaper: true,
        })
        .unwrap();
    expected.reset().unwrap();
    // SAFETY: full-capacity audio ports, exact channelwise in-place alias;
    // control remains readable, all callbacks are serialized.
    unsafe {
        for (port, pointer) in [
            left.as_mut_ptr(),
            right.as_mut_ptr(),
            left.as_mut_ptr(),
            right.as_mut_ptr(),
            &mut control,
        ]
        .into_iter()
        .enumerate()
        {
            descriptor.connect_port.unwrap()(handle, port as _, pointer);
        }
        descriptor.activate.unwrap()(handle);
        for index in 0..4 {
            if index == 2 {
                descriptor.activate.unwrap()(handle);
                expected.reset().unwrap();
            }
            let mut pcm = PlanarBlock::new(format);
            pcm.samples_mut()[..cap].copy_from_slice(&left);
            pcm.samples_mut()[cap..].copy_from_slice(&right);
            expected.process(&mut pcm, cap, false).unwrap();
            descriptor.run.unwrap()(handle, cap as _);
            assert_eq!(&left, &pcm.samples()[..cap]);
            assert_eq!(&right, &pcm.samples()[cap..]);
        }
        descriptor.cleanup.unwrap()(handle);
    }
    assert_eq!(stats(id).instances_created, 1);
    assert_eq!(stats(id).active_instances, 0);
    assert_eq!(stats(id).resets, 2);
    assert_eq!(cumulus_se1_release_v1(id), 0);
}
#[test]
fn invalid_configuration_and_wrong_zero_allocation_shape_are_not_permissions() {
    let id = config(true);
    let revision = stats(id).revision;
    assert_ne!(cumulus_se1_update_v1(id, f32::NAN, 5.0, 1, 1), 0);
    assert_ne!(cumulus_se1_update_v1(id, 0.0, 5.0, 2, 1), 0);
    assert_eq!(stats(id).revision, revision);
    let native = instance(id);
    let mut value = abi::BlockResult::default();
    // SAFETY: null input rejected before any slice is made, including frames0.
    assert_ne!(
        unsafe { cumulus_se1_process_v1(native.0, std::ptr::null_mut(), 0, 1, &mut value) },
        0
    );
    assert_eq!(stats(id).input_frames, 0);
    assert_eq!(stats(id).observed_revision, 0);
    assert_eq!(cumulus_se1_release_v1(id), 0);
}
