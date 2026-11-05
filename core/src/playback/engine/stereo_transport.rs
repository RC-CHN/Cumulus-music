//! Generation/source-frame transport for the qualified local stereo subgraph.
//! It owns no decoder/sink and does not establish a media entitlement.
use super::Cancellation;
use crate::{
    Error, Result,
    effects::stereo::{LocalStereoMaterialPlan, LocalStereoStream},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StereoGeneration {
    owner: uuid::Uuid,
    sequence: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StereoRead {
    Pending,
    /// Coordinates of produced PCM, which may be buffered ahead of the sink.
    /// source_clock is not the audible/native playback position.
    Audio {
        generation: StereoGeneration,
        first_source_frame: u64,
        frames: u32,
        source_clock: u64,
    },
    /// Clean producer/DSP end. The owner still needs the same native sink's
    /// actual EOF event before declaring playback complete/advancing a queue.
    End {
        generation: StereoGeneration,
        source_clock: u64,
    },
}
pub struct StereoSourceTransport {
    stream: LocalStereoStream,
    authority: Cancellation,
    rate: u32,
    allowed_first: u64,
    allowed_end: u64,
    generation: StereoGeneration,
    first: u64,
    next_decoder: u64,
    next_rendered: u64,
    decoder_end: bool,
    retired: bool,
}
impl StereoSourceTransport {
    /// authority must come from the actual held-reader/current-account source
    /// owner. A plan or generation alone grants no right to offer/read audio.
    pub fn new(
        rate: u32,
        decoded_channels: u32,
        plan: LocalStereoMaterialPlan,
        allowed_first: u64,
        allowed_end: u64,
        authority: Cancellation,
    ) -> Result<Self> {
        authority.check()?;
        if allowed_first > allowed_end
            || allowed_end
                .checked_add(1)
                .and_then(|n| n.checked_mul(1000))
                .is_none_or(|n| n / u64::from(rate.max(1)) > i32::MAX as u64)
        {
            return Err(Error::InvalidInput("invalid stereo source range"));
        }
        let fifo = rate
            .checked_add(131072)
            .ok_or(Error::InvalidInput("stereo tail capacity overflow"))?;
        let stream = LocalStereoStream::new(rate, decoded_channels, plan, fifo)?;
        Ok(Self {
            stream,
            authority,
            rate,
            allowed_first,
            allowed_end,
            generation: StereoGeneration {
                owner: uuid::Uuid::new_v4(),
                sequence: 0,
            },
            first: allowed_first,
            next_decoder: allowed_first,
            next_rendered: allowed_first,
            decoder_end: false,
            retired: false,
        })
    }
    fn require(&mut self) -> Result<()> {
        if self.retired {
            return Err(Error::State("stereo source retired"));
        }
        if let Err(error) = self.authority.check() {
            self.retired = true;
            self.stream.retire();
            return Err(error);
        }
        Ok(())
    }
    /// Caller must retire/acknowledge the old native sink and decoder worker
    /// before publishing a replacement. This generation fence rejects their
    /// late packets; it cannot itself acknowledge foreign thread destruction.
    pub fn start(&mut self, first_source_frame: u64) -> Result<StereoGeneration> {
        self.require()?;
        if !(self.allowed_first..=self.allowed_end).contains(&first_source_frame) {
            return Err(Error::InvalidInput("stereo seek outside held source range"));
        }
        let generation = StereoGeneration {
            owner: self.generation.owner,
            sequence: self
                .generation
                .sequence
                .checked_add(1)
                .ok_or(Error::State("stereo generation exhausted"))?,
        };
        self.stream.reset()?;
        self.require()?;
        self.generation = generation;
        self.first = first_source_frame;
        self.next_decoder = first_source_frame;
        self.next_rendered = first_source_frame;
        self.decoder_end = false;
        Ok(generation)
    }
    fn timestamp(&self) -> Option<u32> {
        (self.next_decoder == self.first)
            .then(|| ((self.first * 1000) / u64::from(self.rate)) as u32)
    }
    /// Update only the installed local subgraph; do not synthesize PCM, reset
    /// source position or reinterpret a plan as a new media authorization.
    pub fn configure(&mut self, plan: LocalStereoMaterialPlan) -> Result<()> {
        self.require()?;
        self.stream.configure(plan)?;
        self.require()
    }
    /// Stale-generation packets are ignored before inspecting their payload;
    /// an empty samples packet is invalid, not decoder EOF or buffer starvation.
    pub fn offer_samples(
        &mut self,
        generation: StereoGeneration,
        first: u64,
        pcm: &[f32],
    ) -> Result<bool> {
        self.require()?;
        if generation != self.generation {
            return Ok(false);
        }
        let frames = pcm.len() / 2;
        if self.generation.sequence == 0
            || self.decoder_end
            || first != self.next_decoder
            || frames == 0
            || frames > 2048
            || !pcm.len().is_multiple_of(2)
        {
            return Err(Error::State(
                "invalid or noncontiguous stereo decoder packet",
            ));
        }
        let next = first
            .checked_add(frames as u64)
            .filter(|n| *n <= self.allowed_end)
            .ok_or(Error::State("stereo decoder exceeded held range"))?;
        self.stream.push_interleaved(pcm, self.timestamp())?;
        self.require()?;
        self.next_decoder = next;
        Ok(true)
    }
    pub fn offer_end(&mut self, generation: StereoGeneration, first: u64) -> Result<bool> {
        self.require()?;
        if generation != self.generation {
            return Ok(false);
        }
        if self.generation.sequence == 0
            || self.decoder_end
            || first != self.next_decoder
            || first != self.allowed_end
        {
            return Err(Error::State(
                "stereo decoder ended before held source range",
            ));
        }
        self.stream.push_interleaved(&[], self.timestamp())?;
        self.require()?;
        self.decoder_end = true;
        Ok(true)
    }
    /// Pending is never EOF and must not be returned to a native byte callback
    /// as zero bytes. Native pause/clock remain the downstream sink's state.
    /// An old or foreign sink cannot consume PCM belonging to this generation.
    pub fn read(&mut self, generation: StereoGeneration, output: &mut [f32]) -> Result<StereoRead> {
        if let Err(error) = self.require() {
            output.fill(0.0);
            return Err(error);
        }
        if generation != self.generation {
            output.fill(0.0);
            return Err(Error::StaleResponse);
        }
        if self.generation.sequence == 0 || output.is_empty() || !output.len().is_multiple_of(2) {
            output.fill(0.0);
            return Err(Error::InvalidInput(
                "stereo read requires active whole frames",
            ));
        }
        let result = match self.stream.pull_interleaved(output) {
            Ok(value) => value,
            Err(error) => {
                output.fill(0.0);
                self.retired = true;
                self.stream.retire();
                return Err(error);
            }
        };
        if let Err(error) = self.require() {
            output.fill(0.0);
            return Err(error);
        }
        if result.frames > 0 {
            let first = self.next_rendered;
            self.next_rendered = self.allowed_end.min(first + u64::from(result.frames));
            Ok(StereoRead::Audio {
                generation: self.generation,
                first_source_frame: first,
                frames: result.frames,
                source_clock: self.next_rendered,
            })
        } else if self.decoder_end && result.end == Some(true) {
            Ok(StereoRead::End {
                generation: self.generation,
                source_clock: self.next_rendered,
            })
        } else {
            Ok(StereoRead::Pending)
        }
    }
}
