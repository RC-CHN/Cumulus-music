//! LADSPA 1.1 ABI. `unsigned long` is deliberately c_ulong (LLP64 != LP64).
use crate::{control::contain, processor::Processor};
use std::ffi::{c_char, c_int, c_ulong, c_void};

#[repr(C)]
pub struct PortRange {
    hint: c_int,
    lower: f32,
    upper: f32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Descriptor {
    unique_id: c_ulong,
    label: *const c_char,
    properties: c_int,
    name: *const c_char,
    maker: *const c_char,
    copyright: *const c_char,
    port_count: c_ulong,
    port_descriptors: *const c_int,
    port_names: *const *const c_char,
    port_range_hints: *const PortRange,
    implementation_data: *mut c_void,
    pub instantiate: Option<unsafe extern "C" fn(*const Descriptor, c_ulong) -> *mut c_void>,
    pub connect_port: Option<unsafe extern "C" fn(*mut c_void, c_ulong, *mut f32)>,
    pub activate: Option<unsafe extern "C" fn(*mut c_void)>,
    pub run: Option<unsafe extern "C" fn(*mut c_void, c_ulong)>,
    run_adding: Option<unsafe extern "C" fn(*mut c_void, c_ulong)>,
    set_run_adding_gain: Option<unsafe extern "C" fn(*mut c_void, f32)>,
    deactivate: Option<unsafe extern "C" fn(*mut c_void)>,
    pub cleanup: Option<unsafe extern "C" fn(*mut c_void)>,
}
// SAFETY: descriptor and all referenced arrays/strings are immutable statics.
unsafe impl Sync for Descriptor {}
struct Names([*const c_char; 3]);
// SAFETY: contains only pointers to immutable static C strings.
unsafe impl Sync for Names {}
static NAMES: Names = Names([
    c"Input".as_ptr(),
    c"Output".as_ptr(),
    c"Configuration".as_ptr(),
]);
static PORTS: [c_int; 3] = [1 | 8, 2 | 8, 1 | 4];
static HINTS: [PortRange; 3] = [
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
static DESCRIPTOR: Descriptor = Descriptor {
    // Internal plugin identity; no claim of public LADSPA-ID registration.
    unique_id: 0x43554d,
    label: c"cumulus_geq".as_ptr(),
    properties: 0,
    name: c"Cumulus recovered ten-band GEQ".as_ptr(),
    maker: c"Cumulus community".as_ptr(),
    copyright: c"See Cumulus LICENSE and native source provenance".as_ptr(),
    port_count: 3,
    port_descriptors: PORTS.as_ptr(),
    port_names: NAMES.0.as_ptr(),
    port_range_hints: HINTS.as_ptr(),
    implementation_data: std::ptr::null_mut(),
    instantiate: Some(instantiate),
    connect_port: Some(connect),
    activate: Some(activate),
    run: Some(run),
    run_adding: None,
    set_run_adding_gain: None,
    deactivate: None,
    cleanup: Some(cleanup),
};
static LOUDNESS_DESCRIPTOR: Descriptor = Descriptor {
    unique_id: 0x43554e,
    label: c"cumulus_loudness".as_ptr(),
    name: c"Cumulus recovered loudness balance".as_ptr(),
    instantiate: Some(instantiate_loudness),
    ..DESCRIPTOR
};
#[unsafe(no_mangle)]
pub extern "C" fn ladspa_descriptor(index: c_ulong) -> *const Descriptor {
    match index {
        0 => &DESCRIPTOR,
        1 => &LOUDNESS_DESCRIPTOR,
        _ => std::ptr::null(),
    }
}
unsafe extern "C" fn instantiate_loudness(_: *const Descriptor, rate: c_ulong) -> *mut c_void {
    #[allow(clippy::unnecessary_cast)]
    contain(std::ptr::null_mut(), || {
        Processor::new_loudness(rate as u64)
            .map_or(std::ptr::null_mut(), |p| Box::into_raw(Box::new(p)).cast())
    })
}
unsafe extern "C" fn instantiate(_: *const Descriptor, rate: c_ulong) -> *mut c_void {
    #[allow(clippy::unnecessary_cast)]
    contain(std::ptr::null_mut(), || {
        Processor::new(rate as u64)
            .map_or(std::ptr::null_mut(), |p| Box::into_raw(Box::new(p)).cast())
    })
}
unsafe extern "C" fn connect(handle: *mut c_void, port: c_ulong, data: *mut f32) {
    contain((), || {
        if handle.is_null() || port >= 3 {
            return;
        }
        // SAFETY: host owns a live instance and serializes callbacks.
        unsafe {
            (*handle.cast::<Processor>()).ports[port as usize] = data;
        }
    });
}
unsafe extern "C" fn activate(handle: *mut c_void) {
    contain((), || {
        if handle.is_null() {
            return;
        }
        // SAFETY: live, exclusively accessed instance.
        unsafe {
            (*handle.cast::<Processor>()).reset();
        }
    });
}
unsafe extern "C" fn run(handle: *mut c_void, frames: c_ulong) {
    contain((), || {
        if handle.is_null() || frames as u128 > isize::MAX as u128 / 4 {
            return;
        }
        // SAFETY: LADSPA guarantees valid connected port arrays for sample_count.
        unsafe {
            (*handle.cast::<Processor>()).run(frames as usize);
        }
    });
}
unsafe extern "C" fn cleanup(handle: *mut c_void) {
    contain((), || {
        if handle.is_null() {
            return;
        }
        // SAFETY: host calls cleanup exactly once after the final callback.
        unsafe {
            drop(Box::from_raw(handle.cast::<Processor>()));
        }
    });
}
