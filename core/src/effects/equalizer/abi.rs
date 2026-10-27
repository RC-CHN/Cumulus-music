//! C ABI shared with the trusted `cumulus-audio-filter` dynamic library.
//! All functions are synchronous and copy inputs; no borrowed pointer is retained.
use serde::Serialize;
pub const VERSION: u32 = 1;
pub const CAPACITY: u32 = 2048;
/// Exactly representable as the single LADSPA float control port.
pub const MAX_ID: u32 = 16_777_215;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct EqualizerStats {
    pub revision: u64,
    /// Highest revision observed by any mono instance, not an all-channel fence.
    pub observed_revision: u64,
    pub instances_created: u64,
    pub active_instances: u64,
    /// Sum over channels. Counts only successfully processed samples.
    pub processed_samples: u64,
    pub failed_blocks: u64,
}
pub type Version = unsafe extern "C" fn() -> u32;
pub type Create = unsafe extern "C" fn(*const f32, u32, *mut u32) -> i32;
pub type Update = unsafe extern "C" fn(u32, *const f32, u32) -> i32;
pub type Stats = unsafe extern "C" fn(u32, *mut EqualizerStats) -> i32;
pub type Release = unsafe extern "C" fn(u32) -> i32;
