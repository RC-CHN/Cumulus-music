use crate::control::contain;
use cumulus_core::effects::stereo::{StereoSettings, abi};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
#[derive(Clone, Copy)]
pub(super) struct Configuration {
    pub settings: StereoSettings,
    pub revision: u64,
}
pub(super) struct Control {
    pub config: Mutex<Configuration>,
    pub revoked: AtomicBool,
    pub observed: AtomicU64,
    pub created: AtomicU64,
    pub active: AtomicU64,
    pub input: AtomicU64,
    pub output: AtomicU64,
    pub failed: AtomicU64,
    pub resets: AtomicU64,
}
#[derive(Default)]
struct Registry {
    next: u32,
    entries: HashMap<u32, Arc<Control>>,
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}
pub(super) fn get(id: u32) -> Option<Arc<Control>> {
    registry().lock().ok()?.entries.get(&id).cloned()
}
fn settings(presence: f32, stereoizer: f32, shaper: u32, enabled: u32) -> Option<StereoSettings> {
    if shaper > 1 || enabled > 1 {
        return None;
    }
    StereoSettings::new(enabled == 1, presence, stereoizer, shaper == 1).ok()
}
#[unsafe(no_mangle)]
pub extern "C" fn cumulus_se1_version() -> u32 {
    abi::VERSION
}
/// # Safety
/// out spans one writable aligned u32; no pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_se1_create_v1(
    presence: f32,
    stereoizer: f32,
    shaper: u32,
    enabled: u32,
    out: *mut u32,
) -> i32 {
    contain(-1, || {
        let Some(settings) = settings(presence, stereoizer, shaper, enabled) else {
            return -2;
        };
        if out.is_null() {
            return -2;
        }
        let Ok(mut r) = registry().lock() else {
            return -3;
        };
        if r.next == abi::MAX_ID {
            return -4;
        }
        r.next += 1;
        let id = r.next;
        r.entries.insert(
            id,
            Arc::new(Control {
                config: Mutex::new(Configuration {
                    settings,
                    revision: 1,
                }),
                revoked: AtomicBool::new(false),
                observed: AtomicU64::new(0),
                created: AtomicU64::new(0),
                active: AtomicU64::new(0),
                input: AtomicU64::new(0),
                output: AtomicU64::new(0),
                failed: AtomicU64::new(0),
                resets: AtomicU64::new(0),
            }),
        );
        // SAFETY: caller provides the checked writable scalar output.
        unsafe { out.write(id) };
        0
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn cumulus_se1_update_v1(
    id: u32,
    presence: f32,
    stereoizer: f32,
    shaper: u32,
    enabled: u32,
) -> i32 {
    contain(-1, || {
        let Some(settings) = settings(presence, stereoizer, shaper, enabled) else {
            return -2;
        };
        let Some(c) = get(id) else {
            return -4;
        };
        let Ok(mut config) = c.config.lock() else {
            return -3;
        };
        if c.revoked.load(Ordering::Acquire) {
            return -4;
        }
        if config.settings == settings {
            return 0;
        }
        let Some(revision) = config.revision.checked_add(1) else {
            return -4;
        };
        *config = Configuration { settings, revision };
        0
    })
}
/// # Safety
/// out spans one writable version-1 StereoStats; no pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_se1_stats_v1(id: u32, out: *mut abi::StereoStats) -> i32 {
    contain(-1, || {
        if out.is_null() {
            return -2;
        }
        let Some(c) = get(id) else {
            return -4;
        };
        let Ok(config) = c.config.lock() else {
            return -3;
        };
        let stats = abi::StereoStats {
            revision: config.revision,
            observed_revision: c.observed.load(Ordering::Acquire),
            instances_created: c.created.load(Ordering::Relaxed),
            active_instances: c.active.load(Ordering::Relaxed),
            input_frames: c.input.load(Ordering::Relaxed),
            output_frames: c.output.load(Ordering::Relaxed),
            failed_blocks: c.failed.load(Ordering::Relaxed),
            resets: c.resets.load(Ordering::Relaxed),
        };
        // SAFETY: checked scalar layout and caller's writable output contract.
        unsafe { out.write(stats) };
        0
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn cumulus_se1_release_v1(id: u32) -> i32 {
    contain(-1, || {
        let Ok(mut r) = registry().lock() else {
            return -3;
        };
        let Some(c) = r.entries.remove(&id) else {
            return -4;
        };
        c.revoked.store(true, Ordering::Release);
        0
    })
}
