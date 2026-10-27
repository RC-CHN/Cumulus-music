use super::{Format, PlanarBlock, ffi};
use crate::{Error, Result};
use std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    cell::Cell,
    ffi::c_void,
    marker::PhantomData,
    ptr::NonNull,
};

pub(super) struct State {
    pointer: NonNull<u8>,
    layout: Layout,
    _not_sync: PhantomData<Cell<()>>,
}

// SAFETY: native states have no callbacks or thread affinity; mutation requires
// &mut. Rotation's sole external pointer targets immutable response data owned
// by the containing Rust wrapper for the complete lifetime of this allocation.
unsafe impl Send for State {}

impl State {
    pub fn new(kind: u32) -> Result<Self> {
        // SAFETY: C returns layout constants; no pointer arguments or side effects.
        let (size, alignment) = unsafe {
            (
                ffi::cumulus_dsp_size(kind),
                ffi::cumulus_dsp_alignment(kind),
            )
        };
        if size == 0 || alignment == 0 {
            return Err(Error::Media("invalid DSP state layout"));
        }
        let layout = Layout::from_size_align(size, alignment.max(16))
            .map_err(|_| Error::Media("invalid DSP state layout"))?;
        // SAFETY: valid nonzero layout, retained unchanged for Drop.
        let pointer = NonNull::new(unsafe { alloc_zeroed(layout) })
            .ok_or(Error::Media("DSP state allocation failed"))?;
        Ok(Self {
            pointer,
            layout,
            _not_sync: PhantomData,
        })
    }

    pub fn pointer(&mut self) -> *mut c_void {
        self.pointer.as_ptr().cast()
    }

    pub fn const_pointer(&self) -> *const c_void {
        self.pointer.as_ptr().cast()
    }
}

impl Drop for State {
    fn drop(&mut self) {
        // SAFETY: uniquely owned allocation from alloc_zeroed with this layout.
        // Recovered states contain no heap allocations or native destructors.
        unsafe { dealloc(self.pointer.as_ptr(), self.layout) };
    }
}

pub(super) fn status(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error::Media("invalid DSP parameters or state"))
    }
}

pub(super) struct Stage {
    pub state: State,
    pub format: Format,
    dry: PlanarBlock,
    failed: bool,
}

pub(super) type Process = unsafe extern "C" fn(*mut c_void, *mut f32, usize, *mut f32) -> i32;

impl Stage {
    pub fn new(kind: u32, format: Format) -> Result<Self> {
        Ok(Self {
            state: State::new(kind)?,
            format,
            dry: PlanarBlock::new(format),
            failed: false,
        })
    }

    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        process: Process,
    ) -> Result<()> {
        if self.failed {
            return Err(Error::State(
                "DSP produced non-finite output; recreate the stage",
            ));
        }
        if self.format != block.format() || frames > self.format.capacity() {
            return Err(Error::InvalidInput(
                "DSP block format or frame count mismatch",
            ));
        }
        if block.samples().iter().any(|value| !value.is_finite()) {
            return Err(Error::InvalidInput("non-finite DSP input"));
        }
        // SAFETY: initialized matching state, full channels*capacity planar
        // allocations, separate dry storage, valid_frames <= capacity. Neither
        // pointer can escape and &mut prevents concurrent access or overlap.
        status(unsafe {
            process(
                self.state.pointer(),
                block.samples_mut().as_mut_ptr(),
                frames,
                self.dry.samples_mut().as_mut_ptr(),
            )
        })?;
        if block.samples().iter().any(|value| !value.is_finite()) {
            block.samples_mut().fill(0.0);
            self.failed = true;
            return Err(Error::Media("non-finite DSP output"));
        }
        Ok(())
    }
}
