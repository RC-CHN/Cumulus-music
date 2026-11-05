//! Bounded source-clock FIFO around the SE1 -> default-B0 subgraph.
//! This is not a decoder, media grant or physical sink.
use super::{LocalStereoMaterialPlan, LocalStereoSubgraph};
use crate::{
    Error, Result,
    effects::dsp::{Format, LimiterStreamRead, LimiterStreamWrite, PlanarBlock},
};
use std::collections::VecDeque;
const QUANTUM: usize = 2048;
const MAX_FIFO: usize = 524_288;

pub struct LocalStereoStream {
    graph: LocalStereoSubgraph,
    block: PlanarBlock,
    input: VecDeque<f32>,
    output: VecDeque<f32>,
    capacity: usize,
    input_end: bool,
    graph_end: bool,
    clock_ms: Option<u32>,
    failed: bool,
}
impl LocalStereoStream {
    pub(crate) fn retire(&mut self) {
        self.failed = true;
        self.input.clear();
        self.output.clear();
    }
    /// Fixed 2048-frame host quantum. Reserve the real one-second environment
    /// tail before accepting EOF; no hidden allocation in push/pull/process.
    pub fn new(
        rate: u32,
        decoded_channels: u32,
        plan: LocalStereoMaterialPlan,
        fifo_capacity: u32,
    ) -> Result<Self> {
        if decoded_channels != 2 {
            return Err(Error::InvalidInput(
                "local stereo stream needs decoded stereo",
            ));
        }
        let format = Format::new(rate as f32, 2, QUANTUM as u32)?;
        let capacity = fifo_capacity as usize;
        if capacity < rate as usize + 2 * QUANTUM || capacity > MAX_FIFO {
            return Err(Error::InvalidInput(
                "local stereo FIFO cannot hold bounded tail",
            ));
        }
        let mut input = VecDeque::new();
        let mut output = VecDeque::new();
        input
            .try_reserve_exact(2 * capacity)
            .map_err(|_| Error::Media("stereo input FIFO allocation failed"))?;
        output
            .try_reserve_exact(2 * capacity)
            .map_err(|_| Error::Media("stereo output FIFO allocation failed"))?;
        Ok(Self {
            graph: LocalStereoSubgraph::new(format, plan)?,
            block: PlanarBlock::new(format),
            input,
            output,
            capacity,
            input_end: false,
            graph_end: false,
            clock_ms: None,
            failed: false,
        })
    }
    pub fn format(&self) -> Format {
        self.graph.format()
    }
    pub fn lookahead_frames(&self) -> u32 {
        self.graph.lookahead_frames()
    }
    fn available(&self) -> Result<()> {
        if self.failed {
            Err(Error::State("local stereo stream quarantined"))
        } else {
            Ok(())
        }
    }
    pub fn configure(&mut self, plan: LocalStereoMaterialPlan) -> Result<()> {
        self.available()?;
        self.graph.configure(plan)
    }
    /// New decoder generation/seek discards both FIFOs and source-clock state;
    /// reset the existing graph, preserving the original SE gain histories.
    pub fn reset(&mut self) -> Result<()> {
        self.available()?;
        self.graph.reset()?;
        self.input.clear();
        self.output.clear();
        self.input_end = false;
        self.graph_end = false;
        self.clock_ms = None;
        Ok(())
    }
    fn input_ready(&self) -> usize {
        self.input.len() / 2
    }
    fn output_ready(&self) -> usize {
        self.output.len() / 2
    }
    fn status(&self, accepted: usize) -> LimiterStreamWrite {
        LimiterStreamWrite {
            accepted_frames: accepted as u32,
            input_ready: self.input_ready() as u32,
            output_ready: self.output_ready() as u32,
            input_end: self.input_end,
            clock_ms: self.clock_ms,
        }
    }
    fn advance(&self, clock: Option<u32>, frames: usize) -> Result<Option<u32>> {
        match clock {
            Some(value) if value > 0 && frames > 0 => {
                let next =
                    f64::from(value) + frames as f64 / f64::from(self.format().rate()) * 1000.0;
                if next.trunc() > f64::from(i32::MAX) {
                    return Err(Error::InvalidInput("stereo source timestamp overflow"));
                }
                Ok(Some(next as u32))
            }
            old => Ok(old),
        }
    }
    fn quarantine(&mut self) -> Error {
        self.failed = true;
        self.input.clear();
        self.output.clear();
        Error::Media("local stereo FIFO processing failed")
    }
    /// Empty write is verified decoder EOF, never 'no packet available'. As in
    /// 67CE8, an empty *input FIFO* adds one actual silent input frame at EOF;
    /// it is distinct from the SE tail and limiter lookahead padding.
    pub fn push_interleaved(
        &mut self,
        samples: &[f32],
        timestamp_ms: Option<u32>,
    ) -> Result<LimiterStreamWrite> {
        self.available()?;
        if self.input_end {
            return Ok(self.status(0));
        }
        if !samples.len().is_multiple_of(2)
            || samples.iter().any(|x| !x.is_finite())
            || timestamp_ms.is_some_and(|x| x > i32::MAX as u32)
        {
            return Err(Error::InvalidInput("invalid stereo FIFO input"));
        }
        let accepted = samples.len() / 2;
        let end = samples.is_empty();
        let before = self.input_ready();
        let sentinel = end && before == 0;
        let available = before
            .checked_add(accepted)
            .and_then(|n| n.checked_add(usize::from(sentinel)))
            .ok_or(Error::InvalidInput("stereo FIFO size overflow"))?;
        if available > self.capacity {
            return Err(Error::State("stereo input FIFO is full"));
        }
        let mut clock = timestamp_ms.or(self.clock_ms);
        let quanta = available / QUANTUM;
        // Conservative, source-independent reservation before any mutation.
        // Tail can exceed the old limiter's 65536-frame quota at high rates.
        let bound = if end {
            available
                .checked_add(self.format().rate() as usize)
                .and_then(|n| n.checked_add(QUANTUM))
        } else {
            quanta.checked_mul(QUANTUM)
        };
        let Some(bound) = bound.and_then(|n| n.checked_add(self.output_ready())) else {
            return Err(Error::InvalidInput("stereo output size overflow"));
        };
        if bound > self.capacity {
            return Err(Error::State(
                "stereo output FIFO has insufficient tail capacity",
            ));
        }
        // Validate the clock before accepting samples or advancing either DSP.
        let mut predicted = clock;
        let mut remaining = available;
        while remaining >= QUANTUM || (end && remaining > 0) {
            let n = remaining.min(QUANTUM);
            remaining -= n;
            predicted = self.advance(predicted, n)?;
        }
        if sentinel {
            self.input.extend([0.0, 0.0]);
        } else {
            self.input.extend(samples);
        }
        self.input_end = end;
        let limit = available / QUANTUM
            + (self.format().rate() as usize + 2 * QUANTUM).div_ceil(QUANTUM)
            + 3;
        let mut iterations = 0;
        while self.input_ready() >= QUANTUM || end {
            iterations += 1;
            if iterations > limit {
                return Err(self.quarantine());
            }
            let frames = self.input_ready().min(QUANTUM);
            self.block.samples_mut().fill(0.0);
            for frame in 0..frames {
                for channel in 0..2 {
                    self.block.samples_mut()[channel * QUANTUM + frame] = self
                        .input
                        .pop_front()
                        .expect("admitted complete stereo frame");
                }
            }
            let flow = match self.graph.process(&mut self.block, frames, end) {
                Ok(v) => v,
                Err(_) => return Err(self.quarantine()),
            };
            if self.output_ready() + flow.valid_frames > self.capacity {
                return Err(self.quarantine());
            }
            for frame in 0..flow.valid_frames {
                for channel in 0..2 {
                    self.output
                        .push_back(self.block.samples()[channel * QUANTUM + frame]);
                }
            }
            clock = self.advance(clock, frames)?;
            if flow.end {
                self.graph_end = true;
                break;
            }
        }
        debug_assert_eq!(clock, predicted);
        self.clock_ms = clock;
        Ok(self.status(accepted))
    }
    pub fn pull_interleaved(&mut self, target: &mut [f32]) -> Result<LimiterStreamRead> {
        self.available()?;
        if !target.len().is_multiple_of(2) {
            return Err(Error::InvalidInput("stereo read needs whole frames"));
        }
        let frames = (target.len() / 2).min(self.output_ready());
        for (to, from) in target.iter_mut().zip(self.output.drain(..2 * frames)) {
            *to = from;
        }
        Ok(LimiterStreamRead {
            frames: frames as u32,
            end: (!target.is_empty())
                .then_some(self.input_end && self.graph_end && self.output.is_empty()),
            output_ready: self.output_ready() as u32,
        })
    }
}
