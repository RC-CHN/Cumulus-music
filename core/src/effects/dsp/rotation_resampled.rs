//! Non-44.1 kHz Rotate3D wrapper, with the resampler selected by a native host.
use super::{
    Format, PlanarBlock, ROTATION_KERNEL_RATE, RotationKernel, RotationRateConversion,
    RotationRatePlan, RotationResponses, RotationResult,
};
use crate::{Error, Result};
use std::collections::VecDeque;

const KERNEL_FRAMES: usize = 256;
const FIFO_LIMIT: usize = 65_536;

/// Split planar stereo converter configured for a single rate direction.
/// Some (including empty planes) is ordinary input; None requests end-of-input.
/// All provided input belongs to the converter after a successful call. The
/// implementation must not retain borrowed pointers after return.
pub trait RotationRateConverter {
    fn process(&mut self, input: Option<[&[f32]; 2]>, output: [&mut [f32]; 2]) -> Result<usize>;

    fn clear(&mut self) -> Result<()>;
}

type Frame = [f32; 2];
type Fifo = VecDeque<Frame>;

fn append(fifo: &mut Fifo, left: &[f32], right: &[f32]) -> Result<()> {
    if left.len() > FIFO_LIMIT - fifo.len() {
        return Err(Error::Media("rotation conversion FIFO exceeded its bound"));
    }
    if left.iter().chain(right).any(|value| !value.is_finite()) {
        return Err(Error::Media("non-finite rotation conversion output"));
    }
    fifo.extend(left.iter().zip(right).map(|(&l, &r)| [l, r]));
    Ok(())
}

fn append_output(fifo: &mut Fifo, storage: &[Vec<f32>; 2], count: usize) -> Result<()> {
    let [left, right] = storage;
    if count > left.len() {
        return Err(Error::Media(
            "rotation converter returned an invalid frame count",
        ));
    }
    append(fifo, &left[..count], &right[..count])
}

fn drain_once<C: RotationRateConverter>(
    converter: &mut C,
    fifo: &mut Fifo,
    [left, right]: &mut [Vec<f32>; 2],
) -> Result<()> {
    // 22A30: a non-null zero-capacity call precedes one capped end call.
    if converter.process(Some([&[], &[]]), [&mut [], &mut []])? != 0 {
        return Err(Error::Media("rotation converter wrote into an empty block"));
    }
    let count = converter.process(None, [left, right])?;
    if count > left.len() {
        return Err(Error::Media(
            "rotation converter returned an invalid frame count",
        ));
    }
    append(fifo, &left[..count], &right[..count])?;
    converter.clear()
}

/// The recovered non-44.1 kHz outer stage. End/disable drain no more than one
/// block from each converter and then clear it, matching observed original
/// behavior even when that does not preserve the input frame total.
pub struct ResampledRotationStage<C> {
    format: Format,
    kernel: RotationKernel,
    kernel_block: PlanarBlock,
    ingress: C,
    egress: C,
    in_scratch: [Vec<f32>; 2],
    out_scratch: [Vec<f32>; 2],
    input: Fifo,
    output: Fifo,
    bypass: bool,
    previous: bool,
    period: f32,
    applied_period: f32,
    failed: bool,
}

impl<C: RotationRateConverter> ResampledRotationStage<C> {
    pub fn new(
        format: Format,
        responses: RotationResponses,
        mut converter: impl FnMut(RotationRateConversion) -> Result<C>,
    ) -> Result<Self> {
        if format.channels() != 2
            || format.capacity() < KERNEL_FRAMES
            || format.rate().fract() != 0.0
        {
            return Err(Error::InvalidInput(
                "invalid resampled rotation block format",
            ));
        }
        let RotationRatePlan::Convert { ingress, egress } =
            RotationRatePlan::new(format.rate() as u32)?
        else {
            return Err(Error::InvalidInput("use direct rotation for 44.1 kHz"));
        };
        let input_capacity = ingress.output_capacity(format.capacity() as u32, 0.0)? as usize;
        Ok(Self {
            format,
            kernel: RotationKernel::new(responses, 0.0)?,
            kernel_block: PlanarBlock::new(Format::new(
                ROTATION_KERNEL_RATE as f32,
                2,
                KERNEL_FRAMES as u32,
            )?),
            ingress: converter(ingress)?,
            egress: converter(egress)?,
            in_scratch: [vec![0.0; input_capacity], vec![0.0; input_capacity]],
            out_scratch: [vec![0.0; format.capacity()], vec![0.0; format.capacity()]],
            input: VecDeque::with_capacity(FIFO_LIMIT),
            output: VecDeque::with_capacity(FIFO_LIMIT),
            bypass: true,
            previous: true,
            period: 40.0,
            applied_period: 40.0,
            failed: false,
        })
    }

    pub fn set_period(&mut self, seconds: f32) -> Result<bool> {
        if !seconds.is_finite() {
            return Err(Error::InvalidInput("invalid rotation period"));
        }
        if seconds < 1.0 {
            return Ok(false);
        }
        self.period = seconds;
        Ok(true)
    }

    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    /// Reset conversion and overlap while preserving phase and desired controls.
    pub fn reset(&mut self) -> Result<()> {
        self.require_live()?;
        let result = self.ingress.clear().and_then(|()| self.egress.clear());
        self.failed = result.is_err();
        result?;
        self.input.clear();
        self.output.clear();
        self.kernel.reset();
        Ok(())
    }

    fn require_live(&self) -> Result<()> {
        if self.failed {
            Err(Error::State("rotation failed; recreate the stage"))
        } else {
            Ok(())
        }
    }

    fn read_kernel_block(&mut self) -> usize {
        let count = KERNEL_FRAMES.min(self.input.len());
        let samples = self.kernel_block.samples_mut();
        samples.fill(0.0);
        for i in 0..count {
            let [l, r] = self.input.pop_front().expect("count checked");
            samples[i] = l;
            samples[KERNEL_FRAMES + i] = r;
        }
        count
    }

    fn convert_kernel_output(&mut self, frames: usize) -> Result<()> {
        if frames == 0 {
            return Ok(());
        }
        let data = self.kernel_block.samples();
        let [left, right] = &mut self.out_scratch;
        let count = self.egress.process(
            Some([
                &data[..frames],
                &data[KERNEL_FRAMES..KERNEL_FRAMES + frames],
            ]),
            [left, right],
        )?;
        append_output(&mut self.output, &self.out_scratch, count)
    }

    fn flush(&mut self) -> Result<()> {
        // 23140: extract one kernel tail block, reset/clear phase, then
        // forward unprocessed ingress through egress and drain it once.
        self.kernel_block.samples_mut().fill(0.0);
        let tail = self.kernel.process(&mut self.kernel_block, KERNEL_FRAMES)?;
        self.convert_kernel_output(tail)?;
        self.kernel.reset();
        self.kernel.clear_phase();
        drain_once(&mut self.ingress, &mut self.input, &mut self.in_scratch)?;
        while !self.input.is_empty() {
            let [left, right] = &mut self.in_scratch;
            let frames = left.len().min(self.input.len());
            for i in 0..frames {
                let [l, r] = self.input.pop_front().expect("count checked");
                left[i] = l;
                right[i] = r;
            }
            let [out_l, out_r] = &mut self.out_scratch;
            let count = self
                .egress
                .process(Some([&left[..frames], &right[..frames]]), [out_l, out_r])?;
            append_output(&mut self.output, &self.out_scratch, count)?;
        }
        drain_once(&mut self.egress, &mut self.output, &mut self.out_scratch)
    }

    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<RotationResult> {
        self.require_live()?;
        if block.format() != self.format || frames > self.format.capacity() {
            return Err(Error::InvalidInput(
                "rotation block format or frame count mismatch",
            ));
        }
        if block.samples().iter().any(|value| !value.is_finite()) {
            return Err(Error::InvalidInput("non-finite rotation input"));
        }
        let result = self.advance(block, frames, end);
        self.failed = result.is_err();
        result
    }

    fn advance(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<RotationResult> {
        let cap = self.format.capacity();
        let data = block.samples();
        let planes = [&data[..frames], &data[cap..cap + frames]];
        if self.bypass {
            if self.previous != self.bypass {
                self.previous = self.bypass;
                self.flush()?;
            }
            if self.output.is_empty() {
                return Ok(RotationResult {
                    valid_frames: frames,
                    end,
                    phase: self.kernel.phase(),
                });
            }
            append(&mut self.output, planes[0], planes[1])?;
        } else {
            self.previous = self.bypass;
            if self.applied_period != self.period {
                self.kernel.set_period(self.period)?;
                self.applied_period = self.period;
            }
            if frames != 0 {
                let [left, right] = &mut self.in_scratch;
                let count = self.ingress.process(Some(planes), [left, right])?;
                append_output(&mut self.input, &self.in_scratch, count)?;
            }
            if end {
                drain_once(&mut self.ingress, &mut self.input, &mut self.in_scratch)?;
            }
            while self.input.len() >= KERNEL_FRAMES || end {
                let count = self.read_kernel_block();
                if count == 0 {
                    self.flush()?;
                    break;
                }
                let processed = self.kernel.process(&mut self.kernel_block, count)?;
                self.convert_kernel_output(processed)?;
            }
        }
        let count = if end || self.output.len() >= cap {
            cap.min(self.output.len())
        } else {
            0
        };
        let samples = block.samples_mut();
        for i in 0..count {
            let [l, r] = self.output.pop_front().expect("count checked");
            samples[i] = l;
            samples[cap + i] = r;
        }
        Ok(RotationResult {
            valid_frames: count,
            end: end && self.output.is_empty(),
            phase: self.kernel.phase(),
        })
    }
}
