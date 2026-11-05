//! Default-off, sample-free metadata for an isolated real native proof.
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_EVENTS: usize = 2048;
const FIELDS: usize = 6;
const CREATED: u64 = 1;
const ACTIVATED: u64 = 2;
const RUN: u64 = 3;
const RETIRED: u64 = 4;
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);
static LOG: Log = Log::new();

struct Log {
    next: AtomicU64,
    events: [[AtomicU64; FIELDS]; MAX_EVENTS],
}
impl Log {
    const fn new() -> Self {
        Self {
            next: AtomicU64::new(0),
            events: [const { [const { AtomicU64::new(0) }; FIELDS] }; MAX_EVENTS],
        }
    }
    fn record(&self, instance: u64, rate: u64, kind: u64, frames: u64, revision: u64) {
        let sequence = self.next.fetch_add(1, Ordering::Relaxed);
        let Some(row) = usize::try_from(sequence)
            .ok()
            .and_then(|index| self.events.get(index))
        else {
            return;
        };
        for (slot, value) in row[1..]
            .iter()
            .zip([instance, kind, rate, frames, revision])
        {
            slot.store(value, Ordering::Relaxed);
        }
        // A slot is written once and becomes visible only as a complete row.
        row[0].store(sequence + 1, Ordering::Release);
    }
    fn cursor(&self) -> u64 {
        self.next.load(Ordering::Acquire)
    }
    fn snapshot(&self, output: &mut [[u64; FIELDS]]) -> Option<usize> {
        let cursor = self.cursor();
        if cursor > MAX_EVENTS as u64 {
            return None;
        }
        let mut count = 0;
        for (target, row) in output.iter_mut().zip(&self.events).take(cursor as usize) {
            let sequence = row[0].load(Ordering::Acquire);
            if sequence == 0 {
                break;
            }
            target[0] = sequence;
            for (target, slot) in target[1..].iter_mut().zip(&row[1..]) {
                *target = slot.load(Ordering::Relaxed);
            }
            count += 1;
        }
        Some(count)
    }
}

pub(crate) struct Instance {
    id: u64,
    rate: u64,
}
impl Instance {
    pub(crate) fn new(rate: u64) -> Self {
        let id = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed) + 1;
        LOG.record(id, rate, CREATED, 0, 0);
        Self { id, rate }
    }
    pub(crate) fn activate(&self, revision: u64) {
        LOG.record(self.id, self.rate, ACTIVATED, 0, revision);
    }
    pub(crate) fn run(&self, frames: usize, revision: u64) {
        LOG.record(self.id, self.rate, RUN, frames as u64, revision);
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        LOG.record(self.id, self.rate, RETIRED, 0, 0);
    }
}

#[unsafe(no_mangle)]
extern "C" fn cumulus_tone_geometry_cursor_v1() -> u64 {
    LOG.cursor()
}

/// # Safety
/// `out` spans `rows * 6` writable, aligned `u64` values. A negative status
/// denotes an invalid caller or overflow. A concurrent incomplete row ends
/// the returned prefix; no partial row or unpublished field is exposed.
#[unsafe(no_mangle)]
unsafe extern "C" fn cumulus_tone_geometry_snapshot_v1(out: *mut u64, rows: usize) -> isize {
    if out.is_null() || rows > MAX_EVENTS {
        return -1;
    }
    // SAFETY: the caller supplies valid storage for this bounded slice.
    let output = unsafe { std::slice::from_raw_parts_mut(out.cast::<[u64; FIELDS]>(), rows) };
    LOG.snapshot(output).map_or(-2, |count| count as isize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_never_skips_a_partially_published_callback_or_wraps() {
        let log = Log::new();
        log.record(1, 44100, CREATED, 0, 0);
        log.next.fetch_add(1, Ordering::Relaxed); // concurrent writer reserves row 2
        log.record(2, 48000, RUN, 7, 2);
        let mut rows = [[0; FIELDS]; 3];
        assert_eq!(log.snapshot(&mut rows), Some(1));
        assert_eq!(rows[1..], [[0; FIELDS]; 2]);
        for (field, value) in log.events[1][1..].iter().zip([1, RUN, 44100, 2048, 1]) {
            field.store(value, Ordering::Relaxed);
        }
        log.events[1][0].store(2, Ordering::Release);
        assert_eq!(log.snapshot(&mut rows), Some(3));
        assert_eq!(rows[1], [2, 1, RUN, 44100, 2048, 1]);
        assert_eq!(rows[2], [3, 2, RUN, 48000, 7, 2]);
        log.next.store(MAX_EVENTS as u64, Ordering::Relaxed);
        log.record(3, 48000, RETIRED, 0, 0);
        assert_eq!(log.snapshot(&mut rows), None);
        assert_eq!(log.events[0][0].load(Ordering::Acquire), 1);
    }
}
