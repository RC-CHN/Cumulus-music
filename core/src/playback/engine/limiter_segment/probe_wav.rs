//! Shared bounded WAV sink for isolated native limiter probes.
use super::{
    DecodedPacket, DecoderPayload, Generation, LimiterSegment, SegmentRead,
    native_decoder::{CHANNELS, QUANTUM, RATE, SEEK_FRAME, SOURCE_END, make_segment},
};
use crate::{Result, playback::engine::Cancellation};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

pub(super) const HEADER: usize = 44;
pub(super) const FRAME_BYTES: usize = CHANNELS * size_of::<f32>();
const PULL_FRAMES: usize = 379;
const MAX_PASSES: usize = 4;

#[derive(Default)]
pub(super) struct Trace {
    pub(super) seeks: Vec<(usize, usize)>,
    pub(super) passes: usize,
    pub(super) ends: usize,
    pub(super) bytes_returned: usize,
    pub(super) pcm_high_water: u64,
    pub(super) first_decoder_packet: Option<DecodedPacket>,
    pub(super) seek_pair: Option<[f32; CHANNELS]>,
    pub(super) stale_offers: usize,
    pub(super) pcm_digest: Option<[u8; 32]>,
    pub(super) failure: Option<String>,
}

pub(super) type Receive = Box<dyn Fn() -> Result<Option<DecodedPacket>> + Send>;
pub(super) type StartPass = Box<dyn Fn(Generation, u64) -> io::Result<Receive> + Send>;

struct Pass {
    receive: Receive,
    segment: LimiterSegment,
    digest: Sha256,
}

/// The pinned callback still sees a finite immutable file. Internal byte
/// seeks may replay the same short fixture from its origin. They must not
/// retarget or reset the logical media stream to the requested byte offset.
pub(super) struct DecodedWave {
    start_pass: StartPass,
    generation: Generation,
    first: u64,
    authority: Cancellation,
    trace: Arc<Mutex<Trace>>,
    late: Option<DecodedPacket>,
    header: [u8; HEADER],
    size: usize,
    offset: usize,
    pass: Option<Pass>,
    pcm_next: usize,
    window: [u8; PULL_FRAMES * FRAME_BYTES],
    window_len: usize,
}

impl DecodedWave {
    pub(super) fn new(
        start_pass: StartPass,
        generation: Generation,
        first: u64,
        authority: Cancellation,
        trace: Arc<Mutex<Trace>>,
        late: Option<DecodedPacket>,
    ) -> Self {
        let remaining = SOURCE_END - first;
        let frames = remaining + u64::from(remaining.is_multiple_of(u64::from(QUANTUM)));
        let data = frames as u32 * FRAME_BYTES as u32;
        let mut header = [0; HEADER];
        header[..4].copy_from_slice(b"RIFF");
        header[4..8].copy_from_slice(&(36 + data).to_le_bytes());
        header[8..16].copy_from_slice(b"WAVEfmt ");
        header[16..20].copy_from_slice(&16_u32.to_le_bytes());
        header[20..22].copy_from_slice(&3_u16.to_le_bytes());
        header[22..24].copy_from_slice(&(CHANNELS as u16).to_le_bytes());
        header[24..28].copy_from_slice(&RATE.to_le_bytes());
        header[28..32].copy_from_slice(&(RATE * FRAME_BYTES as u32).to_le_bytes());
        header[32..34].copy_from_slice(&(FRAME_BYTES as u16).to_le_bytes());
        header[34..36].copy_from_slice(&32_u16.to_le_bytes());
        header[36..40].copy_from_slice(b"data");
        header[40..44].copy_from_slice(&data.to_le_bytes());
        Self {
            start_pass,
            generation,
            first,
            authority,
            trace,
            late,
            header,
            size: HEADER + data as usize,
            offset: 0,
            pass: None,
            pcm_next: 0,
            window: [0; PULL_FRAMES * FRAME_BYTES],
            window_len: 0,
        }
    }

    fn begin_pass(&mut self) -> io::Result<()> {
        self.pass.take(); // close the previous pass before requesting a replay
        self.pcm_next = 0;
        self.window_len = 0;
        let mut trace = self.trace.lock().unwrap();
        if trace.passes == MAX_PASSES {
            return Err(io::Error::other("native fixture replay budget exceeded"));
        }
        trace.passes += 1;
        drop(trace);
        let mut segment = make_segment();
        segment
            .activate(self.generation, self.first, SOURCE_END)
            .map_err(io::Error::other)?;
        let receive = (self.start_pass)(self.generation, self.first)?;
        self.pass = Some(Pass {
            receive,
            segment,
            digest: Sha256::new(),
        });
        Ok(())
    }

    fn fill(&mut self, deadline: Instant) -> io::Result<()> {
        if self.pass.is_none() {
            self.begin_pass()?;
        }
        let pass = self.pass.as_mut().unwrap();
        let mut output = [0.0_f32; PULL_FRAMES * CHANNELS];
        loop {
            self.authority.check().map_err(io::Error::other)?;
            if Instant::now() >= deadline {
                return Err(io::Error::other("decoded WAV input timed out"));
            }
            let trace = &self.trace;
            let late = &mut self.late;
            match pass
                .segment
                .read(&mut output, || {
                    if let Some(packet) = late.take() {
                        trace.lock().unwrap().stale_offers += 1;
                        return Ok(Some(packet));
                    }
                    let packet = (pass.receive)()?;
                    if let Some(packet) = packet.as_ref()
                        && let DecoderPayload::Samples(samples) = &packet.payload
                    {
                        let mut trace = trace.lock().unwrap();
                        if trace.first_decoder_packet.is_none() && self.first == 0 {
                            trace.first_decoder_packet = Some(DecodedPacket {
                                generation: packet.generation,
                                first_source_frame: packet.first_source_frame,
                                payload: DecoderPayload::Samples(samples.clone()),
                            });
                        }
                    }
                    Ok(packet)
                })
                .map_err(io::Error::other)?
            {
                SegmentRead::Pending => thread::sleep(Duration::from_millis(1)),
                SegmentRead::Audio {
                    generation,
                    first_source_frame,
                    frames,
                    source_clock,
                } if generation == self.generation => {
                    self.window_len = frames as usize * FRAME_BYTES;
                    self.pcm_next += self.window_len;
                    if self.pcm_next + HEADER > self.size {
                        return Err(io::Error::other("too much decoded WAV data"));
                    }
                    for (bytes, value) in self.window[..self.window_len]
                        .chunks_exact_mut(4)
                        .zip(output)
                    {
                        bytes.copy_from_slice(&value.to_le_bytes());
                    }
                    pass.digest.update(&self.window[..self.window_len]);
                    let mut trace = self.trace.lock().unwrap();
                    trace.pcm_high_water = trace.pcm_high_water.max(source_clock);
                    if let Some(index) = SEEK_FRAME.checked_sub(first_source_frame)
                        && index < u64::from(frames)
                    {
                        let from = index as usize * CHANNELS;
                        let pair = output[from..from + CHANNELS]
                            .try_into()
                            .expect("whole stereo frame");
                        if trace.seek_pair.is_some_and(|previous| previous != pair) {
                            return Err(io::Error::other("WAV byte seek changed immutable PCM"));
                        }
                        trace.seek_pair = Some(pair);
                    }
                    if self.pcm_next + HEADER == self.size {
                        // The registry caps its last Read at the WAV size;
                        // observe successful decoder and limiter EOF *before*
                        // exposing those bytes to libmpv as the final payload.
                        let end = pass
                            .segment
                            .read(&mut output, &pass.receive)
                            .map_err(io::Error::other)?;
                        if end
                            != (SegmentRead::End {
                                generation,
                                source_clock: SOURCE_END,
                            })
                        {
                            return Err(io::Error::other("missing limiter drain at WAV end"));
                        }
                        let digest: [u8; 32] = std::mem::take(&mut pass.digest).finalize().into();
                        if trace.pcm_digest.is_some_and(|first| first != digest) {
                            return Err(io::Error::other("decoder replay changed WAV PCM bytes"));
                        }
                        trace.pcm_digest = Some(digest);
                        trace.ends += 1;
                    }
                    return Ok(());
                }
                _ => {
                    return Err(io::Error::other(
                        "unexpected limiter output generation or state",
                    ));
                }
            }
        }
    }
}

impl DecodedWave {
    fn read_pcm(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut written = 0;
        while written < output.len() && self.offset < self.size {
            if self.offset < HEADER {
                let take = (HEADER - self.offset).min(output.len() - written);
                output[written..written + take]
                    .copy_from_slice(&self.header[self.offset..self.offset + take]);
                self.offset += take;
                written += take;
                continue;
            }
            let pcm = self.offset - HEADER;
            if pcm < self.pcm_next - self.window_len {
                self.begin_pass()?;
            }
            while pcm >= self.pcm_next {
                self.fill(deadline)?;
            }
            let take = (self.pcm_next - pcm).min(output.len() - written);
            let from = pcm - (self.pcm_next - self.window_len);
            output[written..written + take].copy_from_slice(&self.window[from..from + take]);
            self.offset += take;
            written += take;
        }
        self.trace.lock().unwrap().bytes_returned += written;
        Ok(written)
    }
}

impl Read for DecodedWave {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.read_pcm(output).inspect_err(|error| {
            let mut trace = self.trace.lock().unwrap();
            if trace.failure.is_none() {
                trace.failure = Some(error.to_string());
            }
        })
    }
}

impl Seek for DecodedWave {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = match from {
            SeekFrom::Start(at) => i64::try_from(at).ok(),
            SeekFrom::Current(delta) => (self.offset as i64).checked_add(delta),
            SeekFrom::End(delta) => (self.size as i64).checked_add(delta),
        }
        .filter(|n| (0..=self.size as i64).contains(n))
        .ok_or_else(|| io::Error::other("invalid decoded WAV seek"))? as usize;
        if matches!(from, SeekFrom::Start(_)) {
            self.trace.lock().unwrap().seeks.push((self.offset, next));
        }
        self.offset = next;
        Ok(next as u64)
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        Ok(self.offset as u64)
    }
}

pub(super) fn seconds(frames: u64) -> f64 {
    frames as f64 / f64::from(RATE)
}
