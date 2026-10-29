use crate::control::{self, Settings, contain};
use cumulus_core::effects::loudness::{LoudnessSettings, abi};

#[unsafe(no_mangle)]
pub extern "C" fn cumulus_loudness_version() -> u32 {
    abi::VERSION
}
/// # Safety
/// `out` points to one writable u32; no pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_loudness_create_v1(
    gain_db: f32,
    enabled: u32,
    out: *mut u32,
) -> i32 {
    contain(-1, || {
        if enabled > 1 {
            return -2;
        }
        let Ok(settings) = LoudnessSettings::new(enabled == 1, gain_db) else {
            return -2;
        };
        // SAFETY: forwarded pointer has the same synchronous output contract.
        unsafe { control::create_configuration(Settings::Loudness(settings), out) }
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn cumulus_loudness_update_v1(id: u32, gain_db: f32, enabled: u32) -> i32 {
    contain(-1, || {
        if enabled > 1 {
            return -2;
        }
        let Ok(settings) = LoudnessSettings::new(enabled == 1, gain_db) else {
            return -2;
        };
        control::update_configuration(id, Settings::Loudness(settings))
    })
}
/// # Safety
/// `out` points to one writable ABI-v1 statistics record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_loudness_stats_v1(id: u32, out: *mut abi::LoudnessStats) -> i32 {
    // SAFETY: both filters use the same explicitly shared statistics layout.
    unsafe { control::cumulus_geq_stats_v1(id, out) }
}
#[unsafe(no_mangle)]
pub extern "C" fn cumulus_loudness_release_v1(id: u32) -> i32 {
    control::cumulus_geq_release_v1(id)
}
