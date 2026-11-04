//! Disabled transport prototype between a decoder and the recovered limiter.
//! It knows only interleaved PCM; processes, libmpv and GUI state live outside.
use crate::effects::dsp::Format;
use crate::{
    Error, Result,
    effects::{dsp::LimiterStream, material::MaterialLimiterPlan},
};

mod native_decoder;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Generation(u64);

struct DecodedPacket {
    generation: Generation,
    first_source_frame: u64,
    payload: DecoderPayload,
}

enum DecoderPayload {
    Samples(Vec<f32>),
    End, // verified successful decoder EOF
    Failed(std::io::Error),
}

#[derive(Debug, PartialEq, Eq)]
enum SegmentRead {
    Paused,
    Pending,
    Audio {
        generation: Generation,
        first_source_frame: u64,
        frames: u32,
        source_clock: u64,
    },
    End {
        generation: Generation,
        source_clock: u64,
    },
}

struct LimiterSegment {
    dsp: LimiterStream,
    channels: usize,
    max_packet_frames: usize,
    rate: u32,
    generation: Generation,
    source_start: u64,
    source_end: u64,
    next_decoder_frame: u64,
    next_rendered_frame: u64,
    paused: bool,
    decoder_end: bool,
    failed: bool,
}

impl LimiterSegment {
    fn new(plan: &MaterialLimiterPlan, rate: u32, channels: u32, quantum: u32) -> Result<Self> {
        let format = Format::new(rate as f32, channels, quantum)?;
        Ok(Self {
            dsp: plan.pcm_stream(format, quantum * 2)?,
            channels: format.channels(),
            max_packet_frames: format.capacity(),
            rate,
            generation: Generation(0),
            source_start: 0,
            source_end: 0,
            next_decoder_frame: 0,
            next_rendered_frame: 0,
            paused: false,
            decoder_end: false,
            failed: false,
        })
    }

    /// Logical transport seek (or first load). Call before accepting any PCM
    /// from the replacement decoder. Pause is independent of discontinuities.
    fn start(&mut self, first: u64, end: u64) -> Result<Generation> {
        let next = Generation(
            self.generation
                .0
                .checked_add(1)
                .ok_or(Error::State("limiter generation exhausted"))?,
        );
        self.activate(next, first, end)?;
        Ok(next)
    }

    /// Use an identity issued by the controller for a newly owned reader.
    fn activate(&mut self, generation: Generation, first: u64, end: u64) -> Result<()> {
        // The recovered millisecond clock is signed 32-bit. Leave room for
        // its one additional EOF frame and for timestamp arithmetic.
        if generation.0 <= self.generation.0
            || first > end
            || self.millis(end.saturating_add(1)).is_none()
        {
            return Err(Error::InvalidInput("unsupported limiter source range"));
        }
        self.generation = generation;
        self.dsp.reset();
        self.source_start = first;
        self.source_end = end;
        self.next_decoder_frame = first;
        self.next_rendered_frame = first;
        self.decoder_end = false;
        self.failed = false;
        Ok(())
    }

    fn millis(&self, frame: u64) -> Option<u32> {
        frame
            .checked_mul(1000)
            .map(|frame| frame / u64::from(self.rate))
            .and_then(|ms| i32::try_from(ms).ok())
            .map(|ms| ms as u32)
    }

    fn pause(&mut self, paused: bool) {
        self.paused = paused;
    }

    fn admit(&mut self, packet: DecodedPacket) -> Result<()> {
        if packet.generation != self.generation {
            return Ok(());
        }
        if let DecoderPayload::Failed(error) = packet.payload {
            return Err(error.into());
        }
        if self.decoder_end || packet.first_source_frame != self.next_decoder_frame {
            return Err(Error::State("non-contiguous limiter decoder packet"));
        }
        let timestamp = (self.next_decoder_frame == self.source_start).then(|| {
            self.millis(self.source_start)
                .expect("validated source time")
        });
        if let DecoderPayload::Samples(pcm) = packet.payload {
            let frames = pcm.len() / self.channels;
            if frames == 0
                || frames > self.max_packet_frames
                || !pcm.len().is_multiple_of(self.channels)
            {
                return Err(Error::InvalidInput("invalid limiter decoder packet size"));
            }
            let next = self.next_decoder_frame + frames as u64;
            if next > self.source_end {
                return Err(Error::State("limiter decoder overran its source range"));
            }
            self.dsp.push_interleaved(&pcm, timestamp)?;
            self.next_decoder_frame = next;
        } else {
            if self.next_decoder_frame != self.source_end {
                return Err(Error::State(
                    "limiter decoder ended before its source range",
                ));
            }
            self.dsp.push_interleaved(&[], timestamp)?;
            self.decoder_end = true;
        }
        Ok(())
    }

    fn read(
        &mut self,
        output: &mut [f32],
        receive: impl FnMut() -> Result<Option<DecodedPacket>>,
    ) -> Result<SegmentRead> {
        if self.generation == Generation(0) || self.failed {
            return Err(Error::State("limiter segment requires a new generation"));
        }
        if output.is_empty() || !output.len().is_multiple_of(self.channels) {
            return Err(Error::InvalidInput(
                "limiter read needs whole output frames",
            ));
        }
        if self.paused {
            return Ok(SegmentRead::Paused);
        }
        let result = self.read_running(output, receive);
        self.failed = result.is_err();
        result
    }

    fn read_running(
        &mut self,
        output: &mut [f32],
        mut receive: impl FnMut() -> Result<Option<DecodedPacket>>,
    ) -> Result<SegmentRead> {
        // Limit work per caller tick, including discarded stale packets.
        for budget in 0..=4 {
            let read = self.dsp.pull_interleaved(output)?;
            if read.frames > 0 {
                let first_source_frame = self.next_rendered_frame;
                // The official empty-FIFO EOF sentinel is output PCM but
                // must not advance the original media's source-frame clock.
                self.next_rendered_frame = self
                    .source_end
                    .min(first_source_frame + u64::from(read.frames));
                return Ok(SegmentRead::Audio {
                    generation: self.generation,
                    first_source_frame,
                    frames: read.frames,
                    source_clock: self.next_rendered_frame,
                });
            }
            if read.end == Some(true) {
                return Ok(SegmentRead::End {
                    generation: self.generation,
                    source_clock: self.next_rendered_frame,
                });
            }
            if budget == 4 {
                break;
            }
            let Some(packet) = receive()? else { break };
            self.admit(packet)?;
        }
        Ok(SegmentRead::Pending)
    }
}

#[test]
fn limiter_segment_fences_stale_eof_and_errors_and_rejects_invalid_current_packets() {
    use std::{collections::VecDeque, io};

    let plan = crate::effects::material::material_limiter_plan(&serde_json::json!({})).unwrap();
    let mut segment = LimiterSegment::new(&plan, 48_000, 2, 2048).unwrap();
    let old = segment.start(0, 100).unwrap();
    let current = segment.start(10, 11).unwrap();
    let packet = |generation, first_source_frame, payload| DecodedPacket {
        generation,
        first_source_frame,
        payload,
    };
    let mut incoming = VecDeque::from([
        packet(
            old,
            0,
            DecoderPayload::Failed(io::Error::other("old decoder fault")),
        ),
        packet(old, 100, DecoderPayload::End),
        packet(current, 10, DecoderPayload::Samples(vec![0.25, -0.125])),
        packet(current, 11, DecoderPayload::End),
    ]);
    let mut output = [0.0; 2];
    assert_eq!(
        segment
            .read(&mut output, || Ok(incoming.pop_front()))
            .unwrap(),
        SegmentRead::Audio {
            generation: current,
            first_source_frame: 10,
            frames: 1,
            source_clock: 11
        }
    );
    assert_eq!(output, [0.25, -0.125]);
    assert_eq!(
        segment
            .read(&mut output, || panic!("read after clean EOF"))
            .unwrap(),
        SegmentRead::End {
            generation: current,
            source_clock: 11
        }
    );

    for (frame, payload, error) in [
        (
            12,
            DecoderPayload::Samples(vec![0.1, -0.1]),
            "non-contiguous limiter decoder packet",
        ),
        (
            10,
            DecoderPayload::Samples(vec![0.1]),
            "invalid limiter decoder packet size",
        ),
        (
            10,
            DecoderPayload::End,
            "limiter decoder ended before its source range",
        ),
    ] {
        let next = segment.start(10, 11).unwrap();
        let mut incoming = Some(packet(next, frame, payload));
        let result = segment.read(&mut output, || Ok(incoming.take()));
        match result {
            Err(Error::InvalidInput(detail) | Error::State(detail)) => assert_eq!(detail, error),
            _ => panic!("invalid producer event was accepted: {result:?}"),
        }
        assert_eq!(segment.next_rendered_frame, 10);
        assert!(
            segment
                .read(&mut output, || panic!("read after protocol failure"))
                .is_err()
        );
    }
}
