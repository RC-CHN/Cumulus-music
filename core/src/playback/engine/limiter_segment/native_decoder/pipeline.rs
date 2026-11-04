//! Test the actual decoder, limiter and libmpv transport in one session.
use super::{
    CHANNELS, NativeProducer, QUANTUM, RATE, SEEK_FRAME, SOURCE_END, input_pair, make_flac,
    make_segment, until,
};
use crate::playback::engine::{
    AudioEngine, Cancellation, EndReason, EngineEvent, MpvConfig, MpvEngine,
    limiter_segment::{DecodedPacket, DecoderPayload, Generation, LimiterSegment, SegmentRead},
};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const HEADER: usize = 44;
const FRAME_BYTES: usize = CHANNELS * size_of::<f32>();
const PULL_FRAMES: usize = 379;
const MAX_PASSES: usize = 4;

#[derive(Default)]
struct Trace {
    seeks: Vec<(usize, usize)>,
    passes: usize,
    ends: usize,
    bytes_returned: usize,
    pcm_high_water: u64,
    first_decoder_packet: Option<DecodedPacket>,
    seek_pair: Option<[f32; CHANNELS]>,
    stale_offers: usize,
    pcm_digest: Option<[u8; 32]>,
}

struct Pass {
    producer: NativeProducer,
    segment: LimiterSegment,
    digest: Sha256,
}

/// The pinned callback still sees a finite immutable file. Internal byte
/// seeks may replay the same short fixture from its origin. They must not
/// retarget or reset the logical media stream to the requested byte offset.
struct DecodedWave {
    ffmpeg: OsString,
    flac: PathBuf,
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
    fn new(
        ffmpeg: OsString,
        flac: PathBuf,
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
            ffmpeg,
            flac,
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
        self.pass.take(); // terminate/reap before any new callback producer
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
        let producer = NativeProducer::start(&self.ffmpeg, &self.flac, self.generation, self.first);
        self.pass = Some(Pass {
            producer,
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
                    let packet = pass.producer.receive()?;
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
                            .read(&mut output, || pass.producer.receive())
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

impl Read for DecodedWave {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
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

fn seconds(frames: u64) -> f64 {
    frames as f64 / f64::from(RATE)
}

#[test]
#[ignore = "requires compatible libmpv and trusted FFmpeg; runs native FLAC-to-libmpv with null output"]
fn limiter_native_pipeline_generation_seek_and_eof() {
    let ffmpeg =
        std::env::var_os("CUMULUS_LIMITER_PROBE_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
    let dir = tempfile::tempdir().unwrap();
    let flac = make_flac(&ffmpeg, dir.path());
    let mut engine = MpvEngine::open(MpvConfig {
        library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
        null_output: true,
    })
    .unwrap();
    let old = Generation(1);
    let old_auth = Cancellation::default();
    let old_trace = Arc::new(Mutex::new(Trace::default()));
    let original = engine
        .load_probe_wav(
            DecodedWave::new(
                ffmpeg.clone(),
                flac.clone(),
                old,
                0,
                old_auth.clone(),
                old_trace.clone(),
                None,
            ),
            old_auth.clone(),
        )
        .unwrap();
    assert_eq!(original.codec.as_deref(), Some("pcm_f32le"));
    assert!((original.duration.unwrap() - seconds(SOURCE_END + 1)).abs() < 0.0001);
    let (paused_host, prepared) = until(|| {
        let snapshot = engine.poll().unwrap();
        let trace = old_trace.lock().unwrap();
        let position = snapshot.position_for(original.entry_id)?;
        (snapshot.paused && trace.seek_pair.is_some() && trace.ends > 0)
            .then_some((position, trace.pcm_high_water))
    });
    assert!(
        seconds(prepared) > paused_host + 0.5,
        "decoded cursor is not the playback clock"
    );
    engine.pause(false).unwrap();
    let before_seek = until(|| {
        let snapshot = engine.poll().unwrap();
        snapshot
            .position_for(original.entry_id)
            .filter(|pos| *pos >= 0.15)
    });
    engine.pause(true).unwrap();
    let old_pair;
    let late;
    {
        let mut trace = old_trace.lock().unwrap();
        old_pair = trace.seek_pair.unwrap();
        late = trace.first_decoder_packet.take().unwrap();
        assert_eq!(late.generation, old);
        eprintln!(
            "old entry {} paused at {:.3}s, decode high-water {} frames; play advanced to {:.3}s; {} byte callbacks, {} decoder passes",
            original.entry_id,
            paused_host,
            prepared,
            before_seek,
            trace.seeks.len(),
            trace.passes
        );
    }
    engine.stop().unwrap();
    old_auth.cancel();
    let generation = Generation(2);
    let new_auth = Cancellation::default();
    let trace = Arc::new(Mutex::new(Trace::default()));
    let resumed = engine
        .load_probe_wav(
            DecodedWave::new(
                ffmpeg,
                flac,
                generation,
                SEEK_FRAME,
                new_auth.clone(),
                trace.clone(),
                Some(late),
            ),
            new_auth,
        )
        .unwrap();
    assert_ne!(resumed.entry_id, original.entry_id);
    assert_eq!(resumed.codec.as_deref(), Some("pcm_f32le"));
    let remaining = seconds(SOURCE_END - SEEK_FRAME);
    assert!((resumed.duration.unwrap() - remaining).abs() < 0.0001);
    let new_pair = until(|| {
        let snapshot = engine.poll().unwrap();
        assert!(snapshot.paused, "logical seek lost the pause state");
        trace.lock().unwrap().seek_pair
    });
    let expected = input_pair(SEEK_FRAME).map(|v| f32::from(v) / 32768.0);
    assert_eq!(new_pair, expected);
    assert!(old_pair[0] > 0.0 && old_pair[0] < new_pair[0]);
    assert_eq!(old_pair[1], -old_pair[0] * 0.125);

    engine.pause(false).unwrap();
    let mut mapped_clock = seconds(SEEK_FRAME);
    let mut ticks = 0;
    let mut null = false;
    until(|| {
        let snapshot = engine.poll().unwrap();
        null |= snapshot.audio_output.as_deref() == Some("null");
        if let Some(pos) = snapshot.position_for(resumed.entry_id) {
            let mapped = seconds(SEEK_FRAME) + pos;
            assert!(mapped + 0.02 >= mapped_clock && mapped <= seconds(SOURCE_END) + 0.02);
            ticks += usize::from(mapped > mapped_clock);
            mapped_clock = mapped_clock.max(mapped);
        }
        snapshot.events.iter().any(|event| matches!(event, EngineEvent::Ended { entry_id, reason: EndReason::Eof, error: 0 } if *entry_id == resumed.entry_id)).then_some(())
    });
    let trace = trace.lock().unwrap();
    assert!(null && ticks > 2 && mapped_clock >= seconds(SEEK_FRAME) + 0.25);
    assert_eq!(trace.stale_offers, 1);
    assert_eq!(trace.pcm_high_water, SOURCE_END);
    assert!(trace.ends > 0);
    let digest = hex::encode(trace.pcm_digest.unwrap());
    assert!(trace.bytes_returned >= HEADER + (SOURCE_END - SEEK_FRAME) as usize * FRAME_BYTES);
    eprintln!(
        "new entry {} at frame {SEEK_FRAME}: old/new pairs {old_pair:?} -> {new_pair:?}; old PCM rejected, decode reached {} frames ({} terminal passes, {} callback bytes, PCM SHA-256 {digest}); mapped libmpv clock {:.3}s across {ticks} ticks then native EOF; libmpv API {:#x}",
        resumed.entry_id,
        trace.pcm_high_water,
        trace.ends,
        trace.bytes_returned,
        mapped_clock,
        engine.api_version()
    );
}
