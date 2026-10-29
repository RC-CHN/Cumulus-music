use cumulus_core::effects::equalizer::{
    EqualizerSettings,
    abi::{self, EqualizerStats},
};
use cumulus_core::effects::loudness::LoudnessSettings;
use std::{
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[derive(Clone, Copy)]
pub(crate) enum Settings {
    Equalizer(EqualizerSettings),
    Loudness(LoudnessSettings),
}
impl Settings {
    pub fn enabled(self) -> bool {
        match self {
            Self::Equalizer(settings) => settings.enabled(),
            Self::Loudness(settings) => settings.enabled(),
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Configuration {
    pub settings: Settings,
    pub revision: u64,
}
pub(crate) struct Control {
    pub config: Mutex<Configuration>,
    pub revoked: AtomicBool,
    pub observed: AtomicU64,
    pub created: AtomicU64,
    pub active: AtomicU64,
    pub processed: AtomicU64,
    pub failed: AtomicU64,
}
#[derive(Default)]
pub(crate) struct Registry {
    next: u32,
    pub entries: HashMap<u32, Arc<Control>>,
}
pub(crate) fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}
pub(crate) fn contain<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}
fn settings(gains: *const f32, enabled: u32) -> Option<EqualizerSettings> {
    if gains.is_null() || enabled > 1 {
        return None;
    }
    let mut values = [0.0; 10];
    // SAFETY: ABI caller provides ten aligned readable floats for this call.
    unsafe {
        std::ptr::copy_nonoverlapping(gains, values.as_mut_ptr(), 10);
    }
    EqualizerSettings::new(enabled == 1, values).ok()
}
#[unsafe(no_mangle)]
pub extern "C" fn cumulus_geq_version() -> u32 {
    abi::VERSION
}

/// # Safety
/// `gains` points to ten readable floats; `out` points to one writable u32.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_geq_create_v1(
    gains: *const f32,
    enabled: u32,
    out: *mut u32,
) -> i32 {
    contain(-1, || {
        let Some(settings) = settings(gains, enabled) else {
            return -2;
        };
        // SAFETY: forwarded output pointer has the same ABI contract.
        unsafe { create_configuration(Settings::Equalizer(settings), out) }
    })
}

/// # Safety
/// `out` points to one writable u32 for the duration of this call.
pub(crate) unsafe fn create_configuration(settings: Settings, out: *mut u32) -> i32 {
    if out.is_null() {
        return -2;
    }
    let Ok(mut registry) = registry().lock() else {
        return -3;
    };
    if registry.next == abi::MAX_ID {
        return -4;
    }
    registry.next += 1;
    let id = registry.next;
    registry.entries.insert(
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
            processed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        }),
    );
    // SAFETY: validated nonnull output, ABI requires writable storage.
    unsafe {
        out.write(id);
    }
    0
}
fn get(id: u32) -> Option<Arc<Control>> {
    registry().lock().ok()?.entries.get(&id).cloned()
}

/// # Safety
/// `gains` points to ten readable floats; no pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_geq_update_v1(id: u32, gains: *const f32, enabled: u32) -> i32 {
    contain(-1, || {
        let Some(settings) = settings(gains, enabled) else {
            return -2;
        };
        update_configuration(id, Settings::Equalizer(settings))
    })
}
pub(crate) fn update_configuration(id: u32, settings: Settings) -> i32 {
    let Some(control) = get(id) else {
        return -4;
    };
    let Ok(mut config) = control.config.lock() else {
        return -3;
    };
    if control.revoked.load(Ordering::Acquire) {
        return -4;
    }
    if std::mem::discriminant(&config.settings) != std::mem::discriminant(&settings) {
        return -2;
    }
    let Some(revision) = config.revision.checked_add(1) else {
        return -4;
    };
    *config = Configuration { settings, revision };
    0
}
/// # Safety
/// `out` points to a writable ABI-v1 EqualizerStats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cumulus_geq_stats_v1(id: u32, out: *mut EqualizerStats) -> i32 {
    contain(-1, || {
        if out.is_null() {
            return -2;
        }
        let Some(control) = get(id) else {
            return -4;
        };
        let Ok(config) = control.config.lock() else {
            return -3;
        };
        let value = EqualizerStats {
            revision: config.revision,
            observed_revision: control.observed.load(Ordering::Relaxed),
            instances_created: control.created.load(Ordering::Relaxed),
            active_instances: control.active.load(Ordering::Relaxed),
            processed_samples: control.processed.load(Ordering::Relaxed),
            failed_blocks: control.failed.load(Ordering::Relaxed),
        };
        // SAFETY: checked nonnull, caller owns appropriately sized/aligned storage.
        unsafe {
            out.write(value);
        }
        0
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn cumulus_geq_release_v1(id: u32) -> i32 {
    contain(-1, || {
        let Ok(mut registry) = registry().lock() else {
            return -3;
        };
        let Some(control) = registry.entries.remove(&id) else {
            return -4;
        };
        control.revoked.store(true, Ordering::Release);
        0
    })
}
