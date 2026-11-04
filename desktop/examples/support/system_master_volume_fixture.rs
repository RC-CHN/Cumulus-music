use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

static MODE: AtomicU8 = AtomicU8::new(0);
static READS: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_os = "linux")]
pub fn running() -> bool {
    std::env::var_os("CUMULUS_SYSTEM_MASTER_VOLUME_PROOF").is_some()
}

pub fn enabled() -> bool {
    std::env::var("CUMULUS_SYSTEM_MASTER_VOLUME_PROOF").as_deref() == Ok("synthetic")
}

pub fn read() -> Option<(f32, bool)> {
    READS.fetch_add(1, Ordering::Relaxed);
    match MODE.load(Ordering::Relaxed) {
        0 => Some((0.374, false)),
        1 => Some((0.81, true)),
        2 => None,
        3 => Some((0.84, false)),
        _ => unreachable!("fixture selects a named input"),
    }
}

pub fn set(mode: &str) {
    MODE.store(
        match mode {
            "muted" => 1,
            "missing" => 2,
            "recovered" => 3,
            _ => panic!("unknown sealed volume input"),
        },
        Ordering::Relaxed,
    );
}

pub fn reads() -> usize {
    READS.load(Ordering::Relaxed)
}
