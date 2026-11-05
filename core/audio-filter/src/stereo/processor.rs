use super::configuration::{Control, get};
use cumulus_core::{
    Error, Result,
    effects::{
        dsp::{AmbienceOneResult, Format, PlanarBlock, StereoAmbienceOneStage},
        stereo::abi,
    },
};
use std::sync::{Arc, TryLockError, atomic::Ordering};

pub(crate) struct StereoProcessor {
    pub ports: [*mut f32; 5],
    stage: StereoAmbienceOneStage,
    block: PlanarBlock,
    control: Option<Arc<Control>>,
    revision: u64,
    faulted: bool,
}
impl StereoProcessor {
    pub fn new(rate: u64) -> Option<Self> {
        let rate = u32::try_from(rate).ok()?;
        let format = Format::new(rate as f32, 2, abi::CAPACITY).ok()?;
        Some(Self {
            ports: [std::ptr::null_mut(); 5],
            stage: StereoAmbienceOneStage::new(format).ok()?,
            block: PlanarBlock::new(format),
            control: None,
            revision: 0,
            faulted: false,
        })
    }
    pub fn bind(&mut self, id: u32) -> bool {
        if self.control.is_some() {
            return false;
        }
        let Some(c) = get(id) else {
            return false;
        };
        if c.revoked.load(Ordering::Acquire) {
            return false;
        }
        c.created.fetch_add(1, Ordering::Relaxed);
        c.active.fetch_add(1, Ordering::Relaxed);
        self.control = Some(c);
        self.synchronize(true)
    }
    fn synchronize(&mut self, blocking: bool) -> bool {
        if self.faulted {
            return false;
        }
        if self.control.is_none() {
            if !blocking {
                return false;
            }
            if self.ports[4].is_null() {
                return false;
            }
            // SAFETY: connected control port remains readable in the callback.
            let id = unsafe { *self.ports[4] };
            if !id.is_finite() || id.fract() != 0.0 || !(1.0..=abi::MAX_ID as f32).contains(&id) {
                return false;
            }
            return self.bind(id as u32);
        }
        let Some(c) = &self.control else {
            return false;
        };
        if c.revoked.load(Ordering::Acquire) {
            return false;
        }
        let config = if blocking {
            let Ok(v) = c.config.lock() else {
                return false;
            };
            *v
        } else {
            match c.config.try_lock() {
                Ok(v) => *v,
                Err(TryLockError::WouldBlock) => return self.revision != 0,
                Err(TryLockError::Poisoned(_)) => return false,
            }
        };
        if config.revision != self.revision {
            if self.stage.set_controls(config.settings.controls()).is_err()
                || self.stage.set_bypass(!config.settings.enabled()).is_err()
            {
                return false;
            }
            // No zero-sample or fake one-frame settle. The first real block
            // observes the original A80D0 bypass edge and parent gain ramp.
            self.revision = config.revision;
        }
        true
    }
    pub fn reset(&mut self) -> Result<()> {
        if !self.synchronize(true) {
            return Err(Error::State("stereo configuration unavailable"));
        }
        self.stage.reset()?;
        if let Some(c) = &self.control {
            c.resets.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
    fn fault(&mut self) -> Error {
        self.faulted = true;
        if let Some(c) = &self.control {
            c.failed.fetch_add(1, Ordering::Relaxed);
        }
        Error::Media("stereo processor quarantined")
    }
    /// Owned full-capacity planar input/output, with variable EOF semantics.
    pub fn process(
        &mut self,
        pcm: &mut [f32],
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        if pcm.len() != 2 * abi::CAPACITY as usize
            || frames > abi::CAPACITY as usize
            || pcm.iter().any(|x| !x.is_finite())
        {
            return Err(Error::InvalidInput("invalid stereo processor PCM"));
        }
        if !self.synchronize(false) {
            pcm.fill(0.0);
            return Err(self.fault());
        }
        self.block.samples_mut().copy_from_slice(pcm);
        let result = match self.stage.process(&mut self.block, frames, end) {
            Ok(v) => v,
            Err(_) => {
                pcm.fill(0.0);
                return Err(self.fault());
            }
        };
        if self
            .control
            .as_ref()
            .is_none_or(|c| c.revoked.load(Ordering::Acquire))
        {
            pcm.fill(0.0);
            return Err(self.fault());
        }
        pcm.copy_from_slice(self.block.samples());
        if let Some(c) = &self.control {
            c.input.fetch_add(frames as u64, Ordering::Relaxed);
            c.output
                .fetch_add(result.valid_frames as u64, Ordering::Relaxed);
            c.observed.fetch_max(self.revision, Ordering::Release);
        }
        Ok(result)
    }
    /// # Safety
    /// Each audio port spans frames readable/writable floats; exact in-place
    /// alias is allowed, partial cross-block overlap and aliased outputs aren't.
    pub unsafe fn run(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        let cap = abi::CAPACITY as usize;
        let Some(bytes) = frames.checked_mul(4) else {
            return;
        };
        let range = |p: *mut f32| -> Option<(usize, usize)> {
            let a = p as usize;
            (!p.is_null()).then_some((a, a.checked_add(bytes)?))
        };
        let outputs = [range(self.ports[2]), range(self.ports[3])];
        let inputs = [range(self.ports[0]), range(self.ports[1])];
        let intersects = |(a, b): (usize, usize), (c, d): (usize, usize)| a < d && c < b;
        let valid = outputs.iter().all(Option::is_some)
            && inputs.iter().all(Option::is_some)
            && !intersects(outputs[0].unwrap(), outputs[1].unwrap())
            && outputs.iter().flatten().all(|&o| {
                inputs
                    .iter()
                    .flatten()
                    .all(|&i| o.0 == i.0 || !intersects(o, i))
            });
        if !valid {
            for (p, valid) in self.ports[2..4].iter().zip(outputs) {
                if valid.is_some() {
                    // SAFETY: connected output contract remains writable on error.
                    unsafe { std::ptr::write_bytes(*p, 0, frames) };
                }
            }
            let _ = self.fault();
            return;
        }
        for offset in (0..frames).step_by(cap) {
            let count = (frames - offset).min(cap);
            let mut local = [0.0; 2 * abi::CAPACITY as usize];
            for c in 0..2 {
                // SAFETY: validated port range and owned disjoint staging.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.ports[c].add(offset),
                        local.as_mut_ptr().add(c * cap),
                        count,
                    )
                };
            }
            let mut result = self.process(&mut local, count, false);
            if result
                .as_ref()
                .is_ok_and(|flow| flow.valid_frames != count || flow.end)
            {
                result = Err(self.fault());
            }
            for c in 0..2 {
                // SAFETY: all inputs copied before writing exact in-place
                // buffers; partial overlap and inter-output alias rejected.
                unsafe {
                    if result.is_err() {
                        std::ptr::write_bytes(self.ports[c + 2].add(offset), 0, frames - offset)
                    } else {
                        std::ptr::copy_nonoverlapping(
                            local.as_ptr().add(c * cap),
                            self.ports[c + 2].add(offset),
                            count,
                        )
                    }
                };
            }
            if result.is_err() {
                return;
            }
        }
    }
}
impl Drop for StereoProcessor {
    fn drop(&mut self) {
        if let Some(c) = &self.control {
            c.active.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
