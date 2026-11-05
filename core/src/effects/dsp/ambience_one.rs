//! Fixed SE bank-1 environment child. This is not the complete SE/material
//! graph, arbitrary reverb, a live renderer, or an entitlement receipt.
use super::{
    Format, PlanarBlock, ffi,
    state::{State, status},
};
use crate::{Error, Result};
use serde::Serialize;

const STORAGE_LIMIT: usize = 4_194_304;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbienceOneResult {
    pub valid_frames: usize,
    pub end: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbienceOneSnapshot {
    /// None is the original sentinel outside an active EOF drain.
    pub remaining_tail: Option<u32>,
    pub last_tail: u32,
    pub total_tail: u32,
    pub diffusion_positions: [u32; 4],
    /// Four feedback rings and their preceding delay for each actual node.
    pub feedback_positions: [[u32; 5]; 16],
    /// Prior input, prior output, pole for each output channel.
    pub damping: [[f32; 3]; 2],
    pub feedback_history: [[f32; 4]; 16],
}

impl AmbienceOneSnapshot {
    pub(super) fn from_arrays(indices: [u32; 87], history: [f32; 70]) -> Result<Self> {
        if history.iter().any(|value| !value.is_finite()) {
            return Err(Error::Media("non-finite ambience1 history"));
        }
        Ok(Self {
            remaining_tail: (indices[0] != u32::MAX).then_some(indices[0]),
            last_tail: indices[1],
            total_tail: indices[2],
            diffusion_positions: indices[3..7].try_into().expect("four positions"),
            feedback_positions: std::array::from_fn(|i| {
                indices[7 + 5 * i..12 + 5 * i]
                    .try_into()
                    .expect("five positions")
            }),
            damping: std::array::from_fn(|i| {
                history[3 * i..3 * i + 3].try_into().expect("three values")
            }),
            feedback_history: std::array::from_fn(|i| {
                history[6 + 4 * i..10 + 4 * i]
                    .try_into()
                    .expect("four values")
            }),
        })
    }
}

pub(super) fn allocate_storage(rate: u32) -> Result<Box<[f32]>> {
    // SAFETY: pure bounded geometry query for the validated sample rate.
    let count = unsafe { ffi::cumulus_ambience_one_storage(rate) };
    if count == 0 || count > STORAGE_LIMIT {
        return Err(Error::Media("unsupported ambience1 delay geometry"));
    }
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| Error::Media("ambience1 storage allocation failed"))?;
    values.resize(count, 0.0);
    Ok(values.into_boxed_slice())
}

pub struct AmbienceOneEnvironment {
    state: State,
    format: Format,
    // C only borrows this stable owned allocation. State drops before storage;
    // no pointer escapes, and process/reset cannot allocate or resize it.
    _storage: Box<[f32]>,
    failed: bool,
}

impl AmbienceOneEnvironment {
    pub fn new(format: Format) -> Result<Self> {
        if format.channels() != 2
            || format.rate().fract() != 0.0
            || format.capacity() < 4
            || !format.capacity().is_multiple_of(4)
        {
            return Err(Error::InvalidInput(
                "ambience1 requires integer-rate aligned stereo",
            ));
        }
        let rate = format.rate() as u32;
        let mut storage = allocate_storage(rate)?;
        let mut state = State::new(8)?;
        // SAFETY: exclusive aligned C layout and exact owned zeroed arena.
        // The arena remains stable for the full state's lifetime.
        status(unsafe {
            ffi::cumulus_ambience_one_init(
                state.pointer(),
                rate,
                format.capacity() as u32,
                storage.as_mut_ptr(),
                storage.len(),
            )
        })?;
        Ok(Self {
            state,
            format,
            _storage: storage,
            failed: false,
        })
    }

    pub fn format(&self) -> Format {
        self.format
    }

    fn available(&self) -> Result<()> {
        if self.failed {
            Err(Error::State("ambience1 failed; recreate the environment"))
        } else {
            Ok(())
        }
    }

    /// Original reset clears ring/filter contents, preserving their cursors
    /// and current EOF budget. It does not reconstruct a new environment.
    pub fn reset(&mut self) -> Result<()> {
        self.available()?;
        // SAFETY: exclusive initialized state and its still-owned arena.
        status(unsafe { ffi::netease_ambience_one_reset(self.state.pointer()) })
    }

    pub fn snapshot(&self) -> Result<AmbienceOneSnapshot> {
        let mut indices = [0; 87];
        let mut history = [0.0; 70];
        // SAFETY: both fixed arrays exactly match the private C output shape.
        status(unsafe {
            ffi::netease_ambience_one_snapshot(
                self.state.const_pointer(),
                indices.as_mut_ptr(),
                history.as_mut_ptr(),
            )
        })?;
        AmbienceOneSnapshot::from_arrays(indices, history)
    }

    /// May append frames and clear EOF while draining the original tail.
    /// Supply zero valid frames with end=true until the returned end is true.
    /// Only returned valid frames are audio; padding is not an equality claim.
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        self.available()?;
        if block.format() != self.format
            || frames > self.format.capacity()
            || block.samples().iter().any(|value| !value.is_finite())
        {
            return Err(Error::InvalidInput("invalid ambience1 planar block"));
        }
        let mut valid = frames as u32;
        let mut ended = u32::from(end);
        // SAFETY: exclusive matching full-capacity stereo allocation, exact
        // scalar outputs and initialized state. Zero frames is a verified drain.
        let result = status(unsafe {
            ffi::netease_ambience_one_process(
                self.state.pointer(),
                block.samples_mut().as_mut_ptr(),
                &mut valid,
                &mut ended,
            )
        });
        if result.is_err()
            || valid as usize > self.format.capacity()
            || ended > 1
            || block.samples().iter().any(|value| !value.is_finite())
        {
            block.samples_mut().fill(0.0);
            self.failed = true;
            return Err(Error::Media("ambience1 processing failed"));
        }
        Ok(AmbienceOneResult {
            valid_frames: valid as usize,
            end: ended != 0,
        })
    }
}
