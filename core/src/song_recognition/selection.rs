use super::PeakRecord;
use crate::{Error, Result};
use std::ops::Range;

const FIRST_BIN: i32 = 25;
const MAX_ROWS: usize = 999;
// Up to 120,000 samples of the observed 8 kHz input, with no tail padding.
const MAX_FRAMES: usize = (120_000 - 2_048) / 160 + 1;

/// Finite, already centered f32 values, stored by frequency row and then time.
/// Row zero maps to native bin 25. This type cannot construct a matrix from
/// audio, power values, or uncentered logarithms.
pub struct CenteredPeakMatrix<'a> {
    rows: usize,
    frames: usize,
    values: &'a [f32],
}

impl<'a> CenteredPeakMatrix<'a> {
    /// Accepts a bounded matrix for the isolated peak selector. Smaller
    /// rectangles permit clipping tests; the native upstream framing stage
    /// separately requires at least ten frames before calling the selector.
    pub fn new(rows: usize, frames: usize, values: &'a [f32]) -> Result<Self> {
        if !(1..=MAX_ROWS).contains(&rows)
            || !(1..=MAX_FRAMES).contains(&frames)
            || values.len() != rows * frames
        {
            return Err(Error::InvalidInput(
                "recognition centered matrix dimensions are invalid",
            ));
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Err(Error::InvalidInput(
                "recognition centered matrix requires finite values",
            ));
        }
        Ok(Self {
            rows,
            frames,
            values,
        })
    }

    fn window(
        &self,
        row: usize,
        frame: usize,
        row_radius: usize,
        frame_radius: usize,
    ) -> impl Iterator<Item = &'a [f32]> {
        let frames = clipped(frame, frame_radius, self.frames);
        let stride = self.frames;
        let values = self.values;
        clipped(row, row_radius, self.rows)
            .map(move |row| &values[row * stride + frames.start..row * stride + frames.end])
    }

    fn has_greater(
        &self,
        row: usize,
        frame: usize,
        row_radius: usize,
        frame_radius: usize,
        center: f32,
    ) -> bool {
        self.window(row, frame, row_radius, frame_radius)
            .any(|window_row| window_row.iter().any(|&value| value > center))
    }

    fn local_maxima(&self) -> impl Iterator<Item = (usize, usize, f32)> + '_ {
        (0..self.rows).flat_map(move |row| {
            let mut next = 0;
            std::iter::from_fn(move || {
                while next < self.frames {
                    let frame = next;
                    let center = self.values[row * self.frames + frame];
                    next += 1;
                    if !self.has_greater(row, frame, 1, 1, center) {
                        // D14570 advances an extra column only after accepting.
                        next += 1;
                        return Some((row, frame, center));
                    }
                }
                None
            })
        })
    }

    fn local_mean(&self, row: usize, frame: usize) -> f32 {
        // D14910: sum each clipped row in memory order with f32 rounding.
        let mut sum = 0.0_f32;
        let mut count = 0;
        for window_row in self.window(row, frame, 10, 5) {
            count += window_row.len();
            for &value in window_row {
                sum += value;
            }
        }
        sum / count as f32
    }
}

fn clipped(center: usize, radius: usize, len: usize) -> Range<usize> {
    center.saturating_sub(radius)..(center + radius + 1).min(len)
}

/// Runs the default native selector on a centered matrix. Output preserves
/// the input f32 bit patterns and is sorted by time, then frequency.
pub fn select_default_peaks(matrix: &CenteredPeakMatrix<'_>) -> Vec<PeakRecord> {
    let mut peaks = Vec::new();
    for (row, frame, center) in matrix.local_maxima() {
        if center <= 0.0 {
            continue;
        }
        let mean = matrix.local_mean(row, frame);
        if mean <= 2.0 && center - mean <= 1.0 {
            continue;
        }
        // D14750 compares the entire centered neighborhood, including points
        // that did not pass D14570 or D14910.
        if !matrix.has_greater(row, frame, 30, 8, center) {
            peaks.push(PeakRecord {
                bin: row as i32 + FIRST_BIN,
                frame: frame as i32,
                value_bits: center.to_bits(),
            });
        }
    }
    peaks.sort_unstable_by_key(|peak| (peak.frame, peak.bin));
    peaks
}

#[cfg(test)]
mod tests;
