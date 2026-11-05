//! Versioned stereo component adapter; separate from mono GEQ/PEQ ABI.
use serde::Serialize;
use std::ffi::c_void;
pub const VERSION: u32 = 1;
pub const CAPACITY: u32 = 2048;
pub const MAX_ID: u32 = 16_777_215;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct StereoStats {
    pub revision: u64,
    pub observed_revision: u64,
    pub instances_created: u64,
    pub active_instances: u64,
    pub input_frames: u64,
    pub output_frames: u64,
    pub failed_blocks: u64,
    pub resets: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BlockResult {
    pub frames: u32,
    pub end: u32,
}
pub type Version = unsafe extern "C" fn() -> u32;
pub type Create = unsafe extern "C" fn(f32, f32, u32, u32, *mut u32) -> i32;
pub type Update = unsafe extern "C" fn(u32, f32, f32, u32, u32) -> i32;
pub type Stats = unsafe extern "C" fn(u32, *mut StereoStats) -> i32;
pub type Release = unsafe extern "C" fn(u32) -> i32;
pub type Instantiate = unsafe extern "C" fn(u32, u32, *mut *mut c_void) -> i32;
pub type Reset = unsafe extern "C" fn(*mut c_void) -> i32;
/// The in-place block always spans 2*CAPACITY initialized planar floats,
/// including when frames=0. No pointer is retained after the call.
pub type Process = unsafe extern "C" fn(*mut c_void, *mut f32, u32, u32, *mut BlockResult) -> i32;
pub type Destroy = unsafe extern "C" fn(*mut c_void);
