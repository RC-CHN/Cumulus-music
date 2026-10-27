use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Format {
    rate: f32,
    channels: u32,
    capacity: u32,
}

impl Format {
    pub fn new(rate: f32, channels: u32, capacity: u32) -> Result<Self> {
        if !rate.is_finite()
            || !(8000.0..=384000.0).contains(&rate)
            || !(1..=8).contains(&channels)
            || !(1..=4096).contains(&capacity)
        {
            return Err(Error::InvalidInput("unsupported DSP block format"));
        }
        Ok(Self {
            rate,
            channels,
            capacity,
        })
    }
    pub fn rate(self) -> f32 {
        self.rate
    }
    pub fn channels(self) -> usize {
        self.channels as usize
    }
    pub fn capacity(self) -> usize {
        self.capacity as usize
    }
    pub fn sample_count(self) -> usize {
        self.channels() * self.capacity()
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Aligned([f32; 4]);

/// Full-capacity planar storage, including the padding touched by native
/// bypass transitions. Each channel's stride equals capacity, not valid_frames.
pub struct PlanarBlock {
    format: Format,
    data: Vec<Aligned>,
}

impl PlanarBlock {
    pub fn new(format: Format) -> Self {
        Self {
            format,
            data: vec![Aligned([0.0; 4]); format.sample_count().div_ceil(4)],
        }
    }
    pub fn format(&self) -> Format {
        self.format
    }
    pub fn samples(&self) -> &[f32] {
        // SAFETY: Aligned is exactly four initialized contiguous floats, with
        // no tail padding. Format's checked bounds fit in the allocation.
        unsafe { std::slice::from_raw_parts(self.data.as_ptr().cast(), self.format.sample_count()) }
    }
    pub fn samples_mut(&mut self) -> &mut [f32] {
        // SAFETY: same layout guarantee, exclusive borrow of all storage.
        unsafe {
            std::slice::from_raw_parts_mut(
                self.data.as_mut_ptr().cast(),
                self.format.sample_count(),
            )
        }
    }
    pub fn load_interleaved(&mut self, data: &[f32]) -> Result<usize> {
        let channels = self.format.channels();
        if !data.len().is_multiple_of(channels)
            || data.len() > self.format.sample_count()
            || data.iter().any(|value| !value.is_finite())
        {
            return Err(Error::InvalidInput("invalid interleaved DSP input"));
        }
        let stride = self.format.capacity();
        self.samples_mut().fill(0.0);
        for (frame, values) in data.chunks_exact(channels).enumerate() {
            for (channel, value) in values.iter().enumerate() {
                self.samples_mut()[channel * stride + frame] = *value;
            }
        }
        Ok(data.len() / channels)
    }
    pub fn copy_interleaved(&self, frames: usize, target: &mut [f32]) -> Result<()> {
        if frames > self.format.capacity() || target.len() != frames * self.format.channels() {
            return Err(Error::InvalidInput("invalid interleaved DSP output size"));
        }
        for (frame, values) in target.chunks_exact_mut(self.format.channels()).enumerate() {
            for (channel, value) in values.iter_mut().enumerate() {
                *value = self.samples()[channel * self.format.capacity() + frame];
            }
        }
        Ok(())
    }
}
