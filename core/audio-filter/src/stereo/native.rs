//! Explicit variable-frame API. LADSPA alone has no EOF/output-count channel.
use super::StereoProcessor;
use crate::control::contain;
use cumulus_core::effects::stereo::abi;
use std::ffi::c_void;

/// # Safety
/// out spans one writable pointer. Returned handle has exclusive serialized
/// access until exactly one destroy call; no callback may outlive destroy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_se1_instantiate_v1(
    id: u32,
    rate: u32,
    out: *mut *mut c_void,
) -> i32 {
    contain(-1, || {
        if out.is_null() {
            return -2;
        }
        let Some(mut processor) = StereoProcessor::new(u64::from(rate)) else {
            return -2;
        };
        if !processor.bind(id) {
            return -4;
        }
        // SAFETY: caller's writable scalar output; ownership transferred once.
        unsafe { out.write(Box::into_raw(Box::new(processor)).cast()) };
        0
    })
}
/// # Safety
/// handle is the live exclusive handle returned by instantiate.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_se1_reset_v1(handle: *mut c_void) -> i32 {
    contain(-1, || {
        if handle.is_null() {
            return -2;
        }
        // SAFETY: live serialized native instance by caller contract.
        unsafe {
            (*handle.cast::<StereoProcessor>())
                .reset()
                .map_or(-4, |()| 0)
        }
    })
}
/// # Safety
/// handle is live/exclusive. planar spans exactly 2*CAPACITY initialized
/// readable/writable floats even if frames=0; result is disjoint writable
/// BlockResult. Neither pointer may outlive or alias the processor object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_se1_process_v1(
    handle: *mut c_void,
    planar: *mut f32,
    frames: u32,
    end: u32,
    result: *mut abi::BlockResult,
) -> i32 {
    contain(-1, || {
        if handle.is_null()
            || planar.is_null()
            || result.is_null()
            || frames > abi::CAPACITY
            || end > 1
        {
            return -2;
        }
        // SAFETY: complete initialized allocation and exclusive handle.
        let (processor, pcm) = unsafe {
            (
                &mut *handle.cast::<StereoProcessor>(),
                std::slice::from_raw_parts_mut(planar, 2 * abi::CAPACITY as usize),
            )
        };
        match processor.process(pcm, frames as usize, end == 1) {
            Ok(timing) => {
                // SAFETY: disjoint writable result according to ABI contract.
                unsafe {
                    result.write(abi::BlockResult {
                        frames: timing.valid_frames as u32,
                        end: timing.end.into(),
                    })
                };
                0
            }
            Err(_) => {
                pcm.fill(0.0);
                -4
            }
        }
    })
}
/// # Safety
/// handle is returned by instantiate, called exactly once after all callbacks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_se1_destroy_v1(handle: *mut c_void) {
    contain((), || {
        if !handle.is_null() {
            // SAFETY: sole handle ownership returns to Box after final callback.
            unsafe { drop(Box::from_raw(handle.cast::<StereoProcessor>())) };
        }
    })
}
