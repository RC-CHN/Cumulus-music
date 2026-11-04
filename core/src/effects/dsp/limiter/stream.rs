//! AudioEffectsImpl 67C00/68120 framing around an isolated final node.
//! The owned adapter works on interleaved f32; it is not a desktop sink.
use super::{LimiterBlockFlow, LimiterStage, PlanarBlock};
use crate::{Error, Result};
use serde::Serialize;
use std::collections::VecDeque;

const MAX_FIFO_FRAMES: u32 = 65_536;

pub struct LimiterStream {
    stage: LimiterStage,
    block: PlanarBlock,
    input: VecDeque<f32>,
    output: VecDeque<f32>,
    capacity: usize,
    input_end: bool,
    clock_ms: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimiterStreamWrite {
    pub accepted_frames: u32,
    pub input_ready: u32,
    pub output_ready: u32,
    pub input_end: bool,
    pub clock_ms: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimiterStreamRead {
    pub frames: u32,
    /// A zero-capacity native read leaves the EOF output argument unchanged.
    pub end: Option<bool>,
    pub output_ready: u32,
}

/// Source clock and frame counts at one invocation of the recovered node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimiterStreamQuantum {
    pub input_frames: u32,
    pub input_end: bool,
    pub timestamp_ms: u32,
    pub timing: LimiterBlockFlow,
}

struct Admission {
    quanta: usize,
    clock_after: Option<u32>,
}

impl LimiterStage {
    /// Own the original independent input/output FIFO contract. The stage's
    /// format capacity is the processing quantum (normally 2048 in 3.1.42).
    pub(crate) fn into_stream(self, fifo_capacity: u32) -> Result<LimiterStream> {
        let format = self.format;
        let capacity = fifo_capacity as usize;
        if capacity < format.capacity() || fifo_capacity > MAX_FIFO_FRAMES {
            return Err(Error::InvalidInput(
                "unsupported limiter stream FIFO capacity",
            ));
        }
        let samples = capacity * format.channels();
        Ok(LimiterStream {
            stage: self,
            block: PlanarBlock::new(format),
            input: VecDeque::with_capacity(samples),
            output: VecDeque::with_capacity(samples),
            capacity,
            input_end: false,
            clock_ms: None,
        })
    }
}

impl LimiterStream {
    pub fn lookahead_frames(&self) -> u32 {
        self.stage.lookahead_frames()
    }

    fn input_ready(&self) -> usize {
        self.input.len() / self.stage.format.channels()
    }

    fn output_ready(&self) -> usize {
        self.output.len() / self.stage.format.channels()
    }

    fn write_status(&self, accepted_frames: usize) -> LimiterStreamWrite {
        LimiterStreamWrite {
            accepted_frames: accepted_frames as u32,
            input_ready: self.input_ready() as u32,
            output_ready: self.output_ready() as u32,
            input_end: self.input_end,
            clock_ms: self.clock_ms,
        }
    }

    fn clock_after(&self, timestamp: Option<u32>, frames: u32) -> Result<Option<u32>> {
        // 67DDE–67E22: use actual input frames before the node modifies the
        // count. Zero is passed to the graph but does not advance.
        match timestamp {
            Some(value) if value > 0 && frames > 0 => {
                let next = f64::from(frames) / f64::from(self.stage.format.rate()) * 1000.0
                    + f64::from(value);
                if next.trunc() > f64::from(i32::MAX) {
                    return Err(Error::InvalidInput("limiter stream timestamp overflow"));
                }
                Ok(Some(next as u32))
            }
            current => Ok(current),
        }
    }

    fn admit(&self, mut remaining: usize, end: bool, mut clock: Option<u32>) -> Result<Admission> {
        if remaining > self.capacity {
            return Err(Error::State("limiter stream input FIFO is full"));
        }
        let quantum = self.stage.format.capacity();
        let mut flow = self.stage.flow;
        let mut output_ready = self.output_ready();
        let mut quanta = 0;
        while remaining >= quantum || end {
            let frames = remaining.min(quantum);
            remaining -= frames;
            let timing = flow.next_block(frames as u32, end)?;
            clock = self.clock_after(clock, frames as u32)?;
            output_ready += timing.output_frames as usize;
            if output_ready > self.capacity {
                return Err(Error::State("limiter stream output FIFO is full"));
            }
            quanta += 1;
            if timing.output_end {
                break;
            }
        }
        Ok(Admission {
            quanta,
            clock_after: clock,
        })
    }

    /// An empty write ends input. Whole frames only; a write that cannot fit
    /// the bounded FIFOs fails before advancing the clock or sample history.
    pub fn push_interleaved(
        &mut self,
        samples: &[f32],
        timestamp_ms: Option<u32>,
    ) -> Result<LimiterStreamWrite> {
        self.push_interleaved_observed(samples, timestamp_ms, |_| {})
    }

    /// Like push_interleaved, with one observation after each processed quantum.
    pub fn push_interleaved_observed(
        &mut self,
        samples: &[f32],
        timestamp_ms: Option<u32>,
        mut observe: impl FnMut(LimiterStreamQuantum),
    ) -> Result<LimiterStreamWrite> {
        // The official outer function checks this latch before timestamps,
        // input pointers or byte counts; repeated writes are ignored.
        if self.input_end {
            return Ok(self.write_status(0));
        }
        let format = self.stage.format;
        let channels = format.channels();
        let quantum = format.capacity();
        if !samples.len().is_multiple_of(channels)
            || samples.iter().any(|sample| !sample.is_finite())
        {
            return Err(Error::InvalidInput(
                "invalid interleaved limiter stream input",
            ));
        }
        if timestamp_ms.is_some_and(|timestamp| timestamp > i32::MAX as u32) {
            return Err(Error::InvalidInput("invalid limiter stream timestamp"));
        }
        let accepted = samples.len() / channels;
        let end = samples.is_empty();
        let before = self.input_ready();
        // 67CE8–67D1C inserts one *input* silent frame only when EOF arrives
        // at an already empty outer FIFO. It is separate from B0's padding.
        let sentinel = end && before == 0;
        let available = before
            .checked_add(accepted)
            .and_then(|total| total.checked_add(usize::from(sentinel)))
            .ok_or(Error::InvalidInput("limiter stream frame count overflow"))?;
        let mut clock = timestamp_ms.or(self.clock_ms);
        let plan = self.admit(available, end, clock)?;
        if sentinel {
            self.input.extend(std::iter::repeat_n(0.0, channels));
        } else {
            self.input.extend(samples);
        }
        self.input_end = end;
        for _ in 0..plan.quanta {
            let frames = self.input_ready().min(quantum);
            let block = self.block.samples_mut();
            for frame in 0..frames {
                for channel in 0..channels {
                    block[channel * quantum + frame] = self
                        .input
                        .pop_front()
                        .expect("checked complete input frame");
                }
            }
            let timing = self.stage.process(&mut self.block, frames as u32, end)?;
            for frame in 0..timing.output_frames as usize {
                for channel in 0..channels {
                    self.output
                        .push_back(self.block.samples()[channel * quantum + frame]);
                }
            }
            observe(LimiterStreamQuantum {
                input_frames: frames as u32,
                input_end: end,
                timestamp_ms: clock.unwrap_or(0),
                timing,
            });
            clock = self.clock_after(clock, frames as u32)?;
        }
        debug_assert_eq!(clock, plan.clock_after);
        self.clock_ms = plan.clock_after;
        Ok(self.write_status(accepted))
    }

    /// Read available interleaved output, without zero padding. A nonempty
    /// read reports EOF only after input ended and the output FIFO is empty.
    pub fn pull_interleaved(&mut self, samples: &mut [f32]) -> Result<LimiterStreamRead> {
        let channels = self.stage.format.channels();
        if !samples.len().is_multiple_of(channels) {
            return Err(Error::InvalidInput(
                "limiter stream output must hold whole frames",
            ));
        }
        let frames = (samples.len() / channels).min(self.output_ready());
        for (target, source) in samples
            .iter_mut()
            .zip(self.output.drain(..frames * channels))
        {
            *target = source;
        }
        Ok(LimiterStreamRead {
            frames: frames as u32,
            end: (!samples.is_empty()).then_some(self.input_end && self.output.is_empty()),
            output_ready: self.output_ready() as u32,
        })
    }
}

#[cfg(test)]
mod tests;
