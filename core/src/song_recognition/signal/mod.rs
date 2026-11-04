use super::{
    CenteredPeakMatrix, PeakRecord, select_default_peaks,
    selection::{FIRST_BIN, SPECTRUM_ROWS},
};
use crate::{Error, Result};

mod fft;
mod logarithm;

#[cfg(test)]
mod tests;

const MAX_SAMPLES: usize = 120_000;
const WINDOW_SAMPLES: usize = 2_048;
const FRAME_HOP: usize = 160;
const MIN_FRAMES: usize = 10;
pub(super) const MAX_FRAMES: usize = (MAX_SAMPLES - WINDOW_SAMPLES) / FRAME_HOP + 1;
const MIN_LOG_INPUT: f32 = f32::from_bits(0x3596_37bd);

/// The rounding mode of the two observed native positive-normal log routines.
///
/// This choice is explicit and independent of the host CPU's instruction set.
#[derive(Clone, Copy)]
pub enum FingerprintLogMode {
    Sse,
    Fma,
}

/// Already converted 8 kHz mono signed samples, bounded to 15 seconds.
/// This type does not acquire, resample, or downmix device audio.
pub struct FingerprintPcm8k<'a> {
    samples: &'a [i16],
}

impl<'a> FingerprintPcm8k<'a> {
    pub fn new(samples: &'a [i16]) -> Result<Self> {
        if samples.len() > MAX_SAMPLES {
            return Err(Error::InvalidInput(
                "recognition audio exceeds 15 seconds at 8 kHz",
            ));
        }
        Ok(Self { samples })
    }

    fn complete_frames(&self) -> usize {
        self.samples
            .len()
            .checked_sub(WINDOW_SAMPLES)
            .map_or(0, |remaining| remaining / FRAME_HOP + 1)
    }
}

/// An owned, centered frequency-by-time matrix from the default signal plan.
/// No debug or serialization representation is provided for derived audio data.
pub struct FingerprintSpectrum {
    frames: usize,
    values: Vec<f32>,
}

impl FingerprintSpectrum {
    pub fn matrix(&self) -> CenteredPeakMatrix<'_> {
        CenteredPeakMatrix::from_default_spectrum(self.frames, &self.values)
    }

    pub fn peaks(&self) -> Vec<PeakRecord> {
        select_default_peaks(&self.matrix())
    }
}

/// Runs the default recovered offline signal path. The last partial window is
/// dropped, and fewer than ten complete frames yields no spectrum.
pub fn centered_default_spectrum(
    pcm: &FingerprintPcm8k<'_>,
    log_mode: FingerprintLogMode,
) -> Option<FingerprintSpectrum> {
    let frames = pcm.complete_frames();
    if frames < MIN_FRAMES {
        return None;
    }
    let mut values = logarithmic_spectrum(pcm, frames, log_mode);
    center(&mut values);
    Some(FingerprintSpectrum { frames, values })
}

fn logarithmic_spectrum(
    pcm: &FingerprintPcm8k<'_>,
    frames: usize,
    log_mode: FingerprintLogMode,
) -> Vec<f32> {
    let mut values = vec![0.0; SPECTRUM_ROWS * frames];
    fft::for_each_power_frame(pcm.samples, |frame, power| {
        for (row, &value) in power[FIRST_BIN..FIRST_BIN + SPECTRUM_ROWS]
            .iter()
            .enumerate()
        {
            let input = value.sqrt().max(MIN_LOG_INPUT);
            values[row * frames + frame] = logarithm::positive_normal(input, log_mode);
        }
    });
    values
}

fn center(values: &mut [f32]) {
    let mut sum = 0.0_f64;
    for &value in values.iter() {
        sum += f64::from(value);
    }
    let mean = (sum / values.len() as f64) as f32;
    for value in values {
        *value -= mean;
    }
}
