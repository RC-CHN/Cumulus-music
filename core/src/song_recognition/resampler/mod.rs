use crate::{Error, Result};
use ffi::{Api, Context, Format, MONO, STEREO};
use std::path::Path;

mod ffi;
mod loader;

#[cfg(test)]
mod tests;

const DESTINATION_RATE: i32 = 8_000;
const MAX_PACKET_FRAMES: usize = 144_250;
const MAX_INPUT_SECONDS: usize = 16;

#[derive(Clone, Copy)]
pub enum StereoSampleRate {
    Hz44100,
    Hz48000,
}

impl StereoSampleRate {
    fn hz(self) -> i32 {
        match self {
            Self::Hz44100 => 44_100,
            Self::Hz48000 => 48_000,
        }
    }
}

#[derive(Clone, Copy)]
pub enum StereoPcmFormat {
    I16,
    F32,
}

/// One interleaved stereo packet. Float input must be finite and within ±4.0.
pub enum StereoPcmPacket<'a> {
    I16(&'a [[i16; 2]]),
    F32(&'a [[f32; 2]]),
}

/// Explicitly supplied, exact-version offline converter libraries.
/// The application never looks for or loads this pair automatically.
pub struct ResamplerLibraries(Api);

impl ResamplerLibraries {
    /// Load the stock swresample 3.7.100 / avutil 56.51.100 pair from an
    /// absolute local directory. No global CPU or loader configuration is set.
    ///
    /// # Safety
    ///
    /// Only pass a caller-verified, immutable directory with trusted binaries
    /// and dependencies. Loading runs native initializers before checking
    /// versions; version checks are not an authenticity or safety check.
    pub unsafe fn load_local(directory: &Path) -> Result<Self> {
        // SAFETY: this API passes the caller's trust requirement to the loader.
        unsafe { Api::load(directory) }.map(Self)
    }

    /// New conversion state for one input format and sample rate. A device
    /// format change must discard the old instance and construct another.
    pub fn stream(
        &self,
        rate: StereoSampleRate,
        format: StereoPcmFormat,
    ) -> Result<RecognitionResampler<'_>> {
        let rate = rate.hz();
        let pipeline = match format {
            StereoPcmFormat::I16 => NativePipeline::I16(Pipeline::new(&self.0, rate)?),
            StereoPcmFormat::F32 => NativePipeline::F32(Pipeline::new(&self.0, rate)?),
        };
        Ok(RecognitionResampler {
            pipeline,
            limit: rate as usize * MAX_INPUT_SECONDS,
            consumed: 0,
            poisoned: false,
        })
    }
}

/// The two streaming contexts for one offline recognition attempt.
/// Dropping it discards any delayed tail without flushing.
pub struct RecognitionResampler<'a> {
    pipeline: NativePipeline<'a>,
    limit: usize,
    consumed: usize,
    poisoned: bool,
}

impl RecognitionResampler<'_> {
    /// Return only the samples actually produced by this packet, as 8 kHz
    /// mono i16. A zero-length packet does not drain or change the state.
    /// At most 144,250 input frames per packet and sixteen seconds per stream.
    pub fn push(&mut self, packet: StereoPcmPacket<'_>) -> Result<&[i16]> {
        if self.poisoned {
            return Err(Error::State("recognition resampler must be restarted"));
        }
        let count = match &packet {
            StereoPcmPacket::I16(values) => values.len(),
            StereoPcmPacket::F32(values) => values.len(),
        };
        if count > MAX_PACKET_FRAMES || count > self.limit - self.consumed {
            return Err(Error::InvalidInput(
                "recognition resampler input exceeds its bound",
            ));
        }
        let result = match (&mut self.pipeline, packet) {
            (NativePipeline::I16(pipeline), StereoPcmPacket::I16(values)) => pipeline.push(values),
            (NativePipeline::F32(pipeline), StereoPcmPacket::F32(values)) => {
                if values
                    .iter()
                    .flatten()
                    .any(|sample| !sample.is_finite() || sample.abs() > 4.0)
                {
                    return Err(Error::InvalidInput(
                        "recognition float samples are outside their finite bound",
                    ));
                }
                pipeline.push(values)
            }
            _ => return Err(Error::InvalidInput("recognition input format changed")),
        };
        match result {
            Ok(samples) => {
                self.consumed += count;
                Ok(samples)
            }
            Err(error) => {
                // An earlier stage may have consumed part of this packet. Never
                // replay through those contexts or accept a new packet after it.
                self.poisoned = true;
                Err(error)
            }
        }
    }
}

enum NativePipeline<'a> {
    I16(Pipeline<'a, i16>),
    F32(Pipeline<'a, f32>),
}

trait Sample: Copy + Default {
    const ABI: i32;
}
impl Sample for i16 {
    const ABI: i32 = 1;
}
impl Sample for f32 {
    const ABI: i32 = 3;
}

struct Pipeline<'a, T> {
    mixer: Stage<'a, T>,
    resampler: Stage<'a, i16>,
}

impl<'a, T: Sample> Pipeline<'a, T> {
    fn new(api: &'a Api, rate: i32) -> Result<Self> {
        let stereo = Format {
            layout: STEREO,
            sample: T::ABI,
            rate,
        };
        let mono = Format {
            layout: MONO,
            ..stereo
        };
        let destination = Format {
            rate: DESTINATION_RATE,
            sample: i16::ABI,
            ..mono
        };
        Ok(Self {
            mixer: Stage::new(Context::new(api, stereo, mono)?),
            resampler: Stage::new(Context::new(api, mono, destination)?),
        })
    }

    fn push(&mut self, packet: &[[T; 2]]) -> Result<&[i16]> {
        if packet.is_empty() {
            return Ok(&[]);
        }
        // SAFETY: input is packed stereo T, the mixer was created for exactly
        // that format, and the caller checked the frame count.
        let mono = unsafe { self.mixer.push(packet.as_ptr().cast(), packet.len())? };
        if mono.len() != packet.len() {
            return Err(Error::State(
                "recognition downmix produced an unexpected frame count",
            ));
        }
        // SAFETY: the first stage returns exactly the mono T format required
        // by the second stage, and the slice lives through the native call.
        unsafe { self.resampler.push(mono.as_ptr().cast(), mono.len()) }
    }
}

struct Stage<'a, T> {
    context: Context<'a>,
    buffer: Vec<T>,
    #[cfg(test)]
    last: StageProbe,
}

#[cfg(test)]
#[derive(Default)]
struct StageProbe {
    required: usize,
    frames: usize,
    delay: i64,
}

impl<'a, T: Sample> Stage<'a, T> {
    fn new(context: Context<'a>) -> Self {
        debug_assert_eq!(context.output.sample, T::ABI);
        debug_assert_eq!(context.output.layout, MONO);
        Self {
            context,
            buffer: Vec::new(),
            #[cfg(test)]
            last: StageProbe::default(),
        }
    }

    unsafe fn push(&mut self, input: *const u8, frames: usize) -> Result<&[T]> {
        let in_rate = self.context.input.rate as usize;
        let out_rate = self.context.output.rate as usize;
        let delay = self.context.delay()?;
        let required = (frames + delay as usize) * out_rate;
        let required = required.div_ceil(in_rate);
        if required > MAX_PACKET_FRAMES + in_rate || required == 0 {
            return Err(Error::State(
                "recognition resampler output is outside its bound",
            ));
        }
        if required > self.buffer.len() {
            self.buffer
                .try_reserve_exact(required - self.buffer.len())
                .map_err(|_| Error::State("recognition resampler buffer allocation failed"))?;
            self.buffer.resize(required, T::default());
        }
        // SAFETY: capacity is a bounded count of initialized packed output
        // samples. Input's layout/extent are checked by Pipeline::push.
        let produced = unsafe {
            self.context.convert(
                self.buffer.as_mut_ptr().cast(),
                self.buffer.len() as i32,
                Some(input),
                frames as i32,
            )?
        };
        #[cfg(test)]
        {
            self.last = StageProbe {
                required,
                frames: produced,
                delay,
            };
        }
        Ok(&self.buffer[..produced])
    }
}
