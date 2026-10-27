//! Bounded native frame mailbox: one latest frame, no unbounded consumer queue.
use crate::{Error, Result};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Clone)]
pub struct VideoFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    /// Packed RGBA8, with every alpha byte initialized to 255.
    pub rgba: Arc<[u8]>,
}
pub(super) struct Surface {
    pub size: AtomicU64,
    pub stop: AtomicBool,
    pub latest: Mutex<Option<Arc<VideoFrame>>>,
    pub error: Mutex<Option<Arc<Error>>>,
}
#[derive(Clone)]
pub struct VideoFrames {
    pub(super) surface: Arc<Surface>,
    pub(super) worker: std::thread::Thread,
}
pub(super) fn size(width: u32, height: u32) -> Result<u64> {
    if !(16..=1920).contains(&width) || !(16..=1080).contains(&height) {
        return Err(Error::InvalidInput(
            "video surface must fit 16..1920 by 16..1080",
        ));
    }
    Ok((u64::from(width) << 32) | u64::from(height))
}
impl VideoFrames {
    pub fn resize(&self, width: u32, height: u32) -> Result<()> {
        let size = size(width, height)?;
        if self.surface.stop.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        self.surface.size.store(size, Ordering::Release);
        self.worker.unpark();
        Ok(())
    }
    pub fn after(&self, sequence: u64) -> Result<Option<Arc<VideoFrame>>> {
        if let Some(error) = self
            .surface
            .error
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
        {
            return Err(Error::Shared(error.clone()));
        }
        if self.surface.stop.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let frame = self
            .surface
            .latest
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|v| v.sequence > sequence)
            .cloned();
        Ok(frame)
    }
}
