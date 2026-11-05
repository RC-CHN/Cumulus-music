use super::*;
use crate::stereo::StereoProcessor;
struct Names([*const c_char; 5]);
// SAFETY: immutable static strings, no writable object behind the pointers.
unsafe impl Sync for Names {}
static NAMES: Names = Names([
    c"Input L".as_ptr(),
    c"Input R".as_ptr(),
    c"Output L".as_ptr(),
    c"Output R".as_ptr(),
    c"Configuration".as_ptr(),
]);
static PORTS: [c_int; 5] = [1 | 8, 1 | 8, 2 | 8, 2 | 8, 1 | 4];
static HINTS: [PortRange; 5] = [
    PortRange {
        hint: 0,
        lower: 0.0,
        upper: 0.0,
    },
    PortRange {
        hint: 0,
        lower: 0.0,
        upper: 0.0,
    },
    PortRange {
        hint: 0,
        lower: 0.0,
        upper: 0.0,
    },
    PortRange {
        hint: 0,
        lower: 0.0,
        upper: 0.0,
    },
    PortRange {
        hint: 1 | 2 | 32,
        lower: 1.0,
        upper: 16_777_215.0,
    },
];
pub(super) static DESCRIPTOR: Descriptor = Descriptor {
    unique_id: 0x435550,
    label: c"cumulus_se1".as_ptr(),
    name: c"Cumulus recovered stereo SE1 component".as_ptr(),
    port_count: 5,
    port_descriptors: PORTS.as_ptr(),
    port_names: NAMES.0.as_ptr(),
    port_range_hints: HINTS.as_ptr(),
    instantiate: Some(instantiate),
    connect_port: Some(connect),
    activate: Some(activate),
    run: Some(run),
    cleanup: Some(cleanup),
    ..super::DESCRIPTOR
};
unsafe extern "C" fn instantiate(_: *const Descriptor, rate: c_ulong) -> *mut c_void {
    #[allow(clippy::unnecessary_cast)]
    contain(std::ptr::null_mut(), || {
        StereoProcessor::new(rate as u64)
            .map_or(std::ptr::null_mut(), |p| Box::into_raw(Box::new(p)).cast())
    })
}
unsafe extern "C" fn connect(handle: *mut c_void, port: c_ulong, data: *mut f32) {
    contain((), || {
        if !handle.is_null() && port < 5 {
            // SAFETY: host serializes callbacks and retains connected port arrays.
            unsafe { (*handle.cast::<StereoProcessor>()).ports[port as usize] = data };
        }
    })
}
unsafe extern "C" fn activate(handle: *mut c_void) {
    contain((), || {
        if !handle.is_null() {
            // SAFETY: live exclusively accessed instance; preserves native gains.
            let _ = unsafe { (*handle.cast::<StereoProcessor>()).reset() };
        }
    })
}
unsafe extern "C" fn run(handle: *mut c_void, frames: c_ulong) {
    contain((), || {
        if !handle.is_null() && frames as u128 <= isize::MAX as u128 / 4 {
            // SAFETY: LADSPA supplies connected port arrays of sample_count floats.
            unsafe { (*handle.cast::<StereoProcessor>()).run(frames as usize) };
        }
    })
}
unsafe extern "C" fn cleanup(handle: *mut c_void) {
    contain((), || {
        if !handle.is_null() {
            // SAFETY: exactly once, after the last serialized host callback.
            unsafe { drop(Box::from_raw(handle.cast::<StereoProcessor>())) };
        }
    })
}
