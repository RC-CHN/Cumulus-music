use super::{Shared, StereoPcmControl, control::QUANTUM};
use std::{
    io::{self, Read},
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

/// Opaque source-bound, nonseekable float32-le stereo PCM. No header, signed
/// URL or file path is exposed. Only a real producer terminal state yields EOF.
pub struct StereoPcmReader {
    shared: Arc<Shared>,
    window: [u8; QUANTUM * 8],
    offset: usize,
    length: usize,
}
impl StereoPcmReader {
    pub(super) fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            window: [0; QUANTUM * 8],
            offset: 0,
            length: 0,
        }
    }
    pub(crate) fn control(&self) -> StereoPcmControl {
        StereoPcmControl(self.shared.clone())
    }
    pub(crate) fn rate(&self) -> u32 {
        self.control().rate()
    }

    fn fill(&mut self) -> io::Result<()> {
        loop {
            self.shared.check().map_err(io::Error::other)?;
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| io::Error::other("stereo PCM lock poisoned"))?;
            if state.failed {
                return Err(io::Error::other("stereo PCM producer failed"));
            }
            let samples = state.pcm.len().min(QUANTUM * 2);
            if samples > 0 {
                for bytes in self.window[..samples * 4].chunks_exact_mut(4) {
                    bytes.copy_from_slice(
                        &state
                            .pcm
                            .pop_front()
                            .expect("bounded PCM queue")
                            .to_le_bytes(),
                    );
                }
                state.delivered += (samples / 2) as u64;
                self.offset = 0;
                self.length = samples * 4;
                self.shared.wake.notify_all();
                return Ok(());
            }
            if state.end {
                self.offset = 0;
                self.length = 0;
                return Ok(());
            }
            // Account invalidation may have no Condvar notifier; bounded
            // waits recheck its actual fast source predicate without polling
            // the native engine or producing fake silence/early EOF.
            let _guard = self
                .shared
                .wake
                .wait_timeout(state, Duration::from_millis(20))
                .map_err(|_| io::Error::other("stereo PCM lock poisoned"))?;
        }
    }
}
impl Read for StereoPcmReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let result = (|| {
            self.shared.check().map_err(io::Error::other)?;
            if output.is_empty() {
                return Ok(0);
            }
            if self.offset == self.length {
                self.fill()?;
            }
            let count = output.len().min(self.length - self.offset);
            output[..count].copy_from_slice(&self.window[self.offset..self.offset + count]);
            self.shared.check().map_err(io::Error::other)?;
            self.offset += count;
            Ok(count)
        })();
        if result.is_err() {
            output.fill(0);
        }
        result
    }
}
impl Drop for StereoPcmReader {
    fn drop(&mut self) {
        let clean_end = self.offset == self.length
            && self
                .shared
                .state
                .lock()
                .is_ok_and(|state| state.end && !state.failed && state.pcm.is_empty());
        if !clean_end {
            self.control().cancel();
        }
        self.shared.reader_closed.store(true, Ordering::Release);
        self.shared.wake.notify_all();
    }
}
