use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use tauri::Manager;

static MODE: AtomicU8 = AtomicU8::new(0);
static READS: AtomicUsize = AtomicUsize::new(0);
static SPATIAL_READS: AtomicUsize = AtomicUsize::new(0);
static VOLUME_BEFORE_SPATIAL: AtomicUsize = AtomicUsize::new(0);

pub fn running() -> bool {
    std::env::var_os("CUMULUS_SYSTEM_MASTER_VOLUME_PROOF").is_some()
}

pub fn enabled() -> bool {
    std::env::var("CUMULUS_SYSTEM_MASTER_VOLUME_PROOF").as_deref() == Ok("synthetic")
}

pub fn install<R: tauri::Runtime>(app: &tauri::App<R>) {
    match std::env::var("CUMULUS_SYSTEM_MASTER_VOLUME_PROOF") {
        Ok(mode) if mode == "synthetic" => {
            assert!(
                app.manage(cumulus_desktop::audio_diagnostics::SystemMasterVolumeProbe::new(read)),
                "the sealed synthetic endpoint must be installed before opening a proof WebView"
            );
            assert!(app.manage(
                cumulus_desktop::audio_diagnostics::SystemSpatialSoundProbe::new(read_spatial)
            ));
        }
        Ok(mode) if mode == "unsupported" && cfg!(target_os = "linux") => {}
        Err(std::env::VarError::NotPresent) => {}
        _ => panic!("invalid system master volume probe mode for this platform"),
    }
}

fn read() -> Option<(f32, bool)> {
    READS.fetch_add(1, Ordering::Relaxed);
    match MODE.load(Ordering::Relaxed) {
        0 => Some((0.374, false)),
        1 => Some((0.81, true)),
        2 => None,
        3 => Some((0.84, false)),
        4 => Some((0.2999, false)),
        5 => Some((0.3, false)),
        6 => Some((0.81, true)),
        7 => Some((0.84, false)),
        _ => unreachable!("fixture selects a named input"),
    }
}

fn read_spatial() -> Option<bool> {
    VOLUME_BEFORE_SPATIAL.store(READS.load(Ordering::Relaxed), Ordering::Relaxed);
    SPATIAL_READS.fetch_add(1, Ordering::Relaxed);
    match MODE.load(Ordering::Relaxed) {
        0 | 4 | 5 | 6 => Some(false),
        1 | 3 => Some(true),
        2 | 7 => None,
        _ => unreachable!("fixture selects a named input"),
    }
}

pub fn set(mode: &str) {
    MODE.store(
        match mode {
            "muted" => 1,
            "missing" => 2,
            "recovered" => 3,
            "almost_thirty" => 4,
            "thirty" => 5,
            "muted_spatial_off" => 6,
            "spatial_unavailable" => 7,
            _ => panic!("unknown sealed volume input"),
        },
        Ordering::Relaxed,
    );
}

pub fn reads() -> usize {
    READS.load(Ordering::Relaxed)
}

pub fn spatial_reads() -> usize {
    SPATIAL_READS.load(Ordering::Relaxed)
}

pub fn volume_before_spatial() -> usize {
    VOLUME_BEFORE_SPATIAL.load(Ordering::Relaxed)
}
