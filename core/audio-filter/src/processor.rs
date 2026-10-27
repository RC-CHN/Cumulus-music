use crate::control::{Control, registry};
use cumulus_core::effects::{
    dsp::{Format, GraphicEq, PlanarBlock},
    equalizer::abi,
};
use std::sync::{Arc, TryLockError, atomic::Ordering};

pub(crate) struct Processor {
    pub ports: [*mut f32; 3],
    stage: GraphicEq,
    block: PlanarBlock,
    control: Option<Arc<Control>>,
    revision: u64,
    faulted: bool,
}
impl Processor {
    pub fn new(rate: u64) -> Option<Self> {
        let format = Format::new(rate as f32, 1, abi::CAPACITY).ok()?;
        Some(Self {
            ports: [std::ptr::null_mut(); 3],
            stage: GraphicEq::new(format).ok()?,
            block: PlanarBlock::new(format),
            control: None,
            revision: 0,
            faulted: false,
        })
    }
    pub fn reset(&mut self) {
        // LADSPA activate is outside run, may allocate, and resets all histories.
        // The host connects the native-owned configuration ID before activation.
        // Bind and copy its initial settings here: registry/configuration lock
        // contention must not turn the first audio block into silence.
        if let Ok(stage) = GraphicEq::new(self.block.format()) {
            self.stage = stage;
            self.revision = 0;
            self.faulted = false;
            let _ = self.synchronize(true);
        } else {
            self.faulted = true;
        }
    }
    fn synchronize(&mut self, activating: bool) -> bool {
        if self.control.is_none() {
            if self.ports[2].is_null() {
                return false;
            }
            // SAFETY: the connected control is readable during activate/run.
            let id = unsafe { *self.ports[2] };
            if !id.is_finite() || id.fract() != 0.0 || !(1.0..=abi::MAX_ID as f32).contains(&id) {
                return false;
            }
            let registry = if activating {
                registry().lock().ok()
            } else {
                registry().try_lock().ok()
            };
            let Some(registry) = registry else {
                return false;
            };
            let Some(control) = registry.entries.get(&(id as u32)) else {
                return false;
            };
            control.created.fetch_add(1, Ordering::Relaxed);
            control.active.fetch_add(1, Ordering::Relaxed);
            self.control = Some(control.clone());
        }
        let Some(control) = &self.control else {
            return false;
        };
        if control.revoked.load(Ordering::Acquire) {
            return false;
        }
        let config = if activating {
            let Ok(value) = control.config.lock() else {
                return false;
            };
            *value
        } else {
            match control.config.try_lock() {
                Ok(value) => *value,
                Err(TryLockError::WouldBlock) => return self.revision != 0,
                Err(TryLockError::Poisoned(_)) => return false,
            }
        };
        if config.revision != self.revision {
            if self.stage.set_gains(config.settings.gains()).is_err()
                || self.stage.set_bypass(!config.settings.enabled()).is_err()
            {
                return false;
            }
            if self.revision == 0 && !config.settings.enabled() {
                // A new/seek-recreated instance starts disabled without a wet
                // first block. Settle the recovered bypass edge on empty owned
                // padding; later user toggles still use its native crossfade.
                self.block.samples_mut().fill(0.0);
                if self.stage.process(&mut self.block, 0).is_err() {
                    return false;
                }
            }
            self.revision = config.revision;
            control
                .observed
                .fetch_max(config.revision, Ordering::Relaxed);
        }
        true
    }
    /// # Safety
    /// Connected audio ports each span `frames` aligned floats; input/output may
    /// alias (in-place), controls do not overlap audio. Host serializes callbacks.
    pub unsafe fn run(&mut self, frames: usize) {
        if frames == 0 || self.ports[1].is_null() {
            return;
        }
        if self.faulted || self.ports[0].is_null() || !self.synchronize(false) {
            // SAFETY: output pointer is connected, with frames writable floats.
            unsafe {
                std::ptr::write_bytes(self.ports[1], 0, frames);
            }
            if let Some(c) = &self.control {
                c.failed.fetch_add(1, Ordering::Relaxed);
            }
            return;
        }
        for offset in (0..frames).step_by(abi::CAPACITY as usize) {
            let count = (frames - offset).min(abi::CAPACITY as usize);
            self.block.samples_mut().fill(0.0);
            // SAFETY: input is readable and copied into disjoint owned storage
            // before any output write, so in-place hosts are supported.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.ports[0].add(offset),
                    self.block.samples_mut().as_mut_ptr(),
                    count,
                );
            }
            if self.stage.process(&mut self.block, count).is_err() {
                self.faulted = true;
                if let Some(c) = &self.control {
                    c.failed.fetch_add(1, Ordering::Relaxed);
                }
                // SAFETY: remaining output is writable; never emit NaN/Inf.
                unsafe {
                    std::ptr::write_bytes(self.ports[1].add(offset), 0, frames - offset);
                }
                return;
            }
            // SAFETY: disjoint block and writable output, count bounded by frames.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    self.block.samples().as_ptr(),
                    self.ports[1].add(offset),
                    count,
                );
            }
            if let Some(c) = &self.control {
                c.processed.fetch_add(count as u64, Ordering::Relaxed);
            }
        }
    }
}
impl Drop for Processor {
    fn drop(&mut self) {
        if let Some(c) = &self.control {
            c.active.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
