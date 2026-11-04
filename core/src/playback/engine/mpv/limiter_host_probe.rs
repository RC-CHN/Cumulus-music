//! Opt-in native experiment. No normal playback, account or network routing.
use super::*;
use crate::effects::{
    dsp::{Format, LimiterStream, LimiterStreamQuantum},
    material::material_limiter_plan,
};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
    thread,
};

const RATE: u32 = 44_100;
const CHANNELS: usize = 2;
const QUANTUM: usize = 2048;
const SOURCE_END: usize = 98_304;
const RESTART: usize = 40_500;
const HEADER: usize = 44;
const FRAME_BYTES: usize = CHANNELS * size_of::<f32>();
const INPUT_SLICE: usize = 613;
const OUTPUT_SLICE: usize = 379;

#[derive(Default)]
struct Trace {
    absolute_seeks: Vec<(usize, usize)>,
    quanta: Vec<LimiterStreamQuantum>,
    reads: usize,
    high_water: usize,
    generations_drained: usize,
}

/// One finite *immutable* WAV generation. A byte seek may regenerate data
/// from this generation's start, but must always return the same bytes.
/// A transport discontinuity instead replaces this reader and its DSP.
struct SyntheticWave {
    stream: LimiterStream,
    source_start: usize,
    source_next: usize,
    source_end: bool,
    drained: bool,
    timestamp: Option<u32>,
    position: usize,
    header: [u8; HEADER],
    size: usize,
    pcm_next: usize,
    pending: [u8; OUTPUT_SLICE * FRAME_BYTES],
    pending_len: usize,
    trace: Arc<Mutex<Trace>>,
}

impl SyntheticWave {
    fn new(source_start: usize, trace: Arc<Mutex<Trace>>) -> Self {
        let remaining = SOURCE_END - source_start;
        let output = remaining + usize::from(remaining.is_multiple_of(QUANTUM));
        let data = (output * FRAME_BYTES) as u32;
        let mut header = [0; HEADER];
        header[..4].copy_from_slice(b"RIFF");
        header[4..8].copy_from_slice(&(36 + data).to_le_bytes());
        header[8..16].copy_from_slice(b"WAVEfmt ");
        header[16..20].copy_from_slice(&16_u32.to_le_bytes());
        header[20..22].copy_from_slice(&3_u16.to_le_bytes()); // IEEE float PCM
        header[22..24].copy_from_slice(&(CHANNELS as u16).to_le_bytes());
        header[24..28].copy_from_slice(&RATE.to_le_bytes());
        header[28..32].copy_from_slice(&(RATE * FRAME_BYTES as u32).to_le_bytes());
        header[32..34].copy_from_slice(&(FRAME_BYTES as u16).to_le_bytes());
        header[34..36].copy_from_slice(&32_u16.to_le_bytes());
        header[36..40].copy_from_slice(b"data");
        header[40..44].copy_from_slice(&data.to_le_bytes());
        let format = Format::new(RATE as f32, CHANNELS as u32, QUANTUM as u32).unwrap();
        let stream = material_limiter_plan(&serde_json::json!({}))
            .unwrap()
            .pcm_stream(format, (QUANTUM * 2) as u32)
            .unwrap();
        let mut result = Self {
            stream,
            source_start,
            source_next: source_start,
            source_end: false,
            drained: false,
            timestamp: None,
            position: 0,
            header,
            size: HEADER + data as usize,
            pcm_next: 0,
            pending: [0; OUTPUT_SLICE * FRAME_BYTES],
            pending_len: 0,
            trace,
        };
        result.reset_generation();
        result
    }

    fn reset_generation(&mut self) {
        self.stream.reset();
        self.source_next = self.source_start;
        self.source_end = false;
        self.drained = false;
        self.timestamp = Some((self.source_start * 1000 / RATE as usize) as u32);
        self.pcm_next = 0;
        self.pending_len = 0;
    }

    fn fill(&mut self) -> io::Result<()> {
        let mut out = [0.0_f32; OUTPUT_SLICE * CHANNELS];
        loop {
            let read = self
                .stream
                .pull_interleaved(&mut out)
                .map_err(io::Error::other)?;
            if read.frames > 0 || read.end == Some(true) {
                self.pending_len = read.frames as usize * FRAME_BYTES;
                for (bytes, value) in self.pending[..self.pending_len]
                    .chunks_exact_mut(4)
                    .zip(out)
                {
                    bytes.copy_from_slice(&value.to_le_bytes());
                }
                self.pcm_next += self.pending_len;
                self.drained = read.end == Some(true);
                if self.drained {
                    if self.pcm_next + HEADER != self.size {
                        return Err(io::Error::other("unexpected limiter EOF length"));
                    }
                    self.trace.lock().unwrap().generations_drained += 1;
                }
                return Ok(());
            }
            if self.source_end {
                return Err(io::Error::other("limiter stalled after input EOF"));
            }
            let mut input = [0.0_f32; INPUT_SLICE * CHANNELS];
            let next = (self.source_next + INPUT_SLICE).min(SOURCE_END);
            let frames = next - self.source_next;
            for (i, pair) in input.chunks_exact_mut(CHANNELS).take(frames).enumerate() {
                let frame = self.source_next + i;
                let left = if frame < RESTART { 2.5 } else { 0.5 };
                pair.copy_from_slice(&[left, -left * 0.125]);
            }
            let trace = &self.trace;
            self.stream
                .push_interleaved_observed(
                    &input[..frames * CHANNELS],
                    self.timestamp.take(),
                    |q| {
                        trace.lock().unwrap().quanta.push(q);
                    },
                )
                .map_err(io::Error::other)?;
            self.source_next = next;
            self.source_end = frames == 0;
        }
    }
}

impl Read for SyntheticWave {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let mut n = 0;
        while n < output.len() && self.position < self.size {
            if self.position < HEADER {
                let take = (HEADER - self.position).min(output.len() - n);
                output[n..n + take]
                    .copy_from_slice(&self.header[self.position..self.position + take]);
                self.position += take;
                n += take;
                continue;
            }
            let offset = self.position - HEADER;
            if offset < self.pcm_next - self.pending_len {
                self.reset_generation();
            }
            while offset >= self.pcm_next && !self.drained {
                self.fill()?;
            }
            let take = (self.pcm_next - offset).min(output.len() - n);
            if take == 0 {
                return Err(io::Error::other("early limiter EOF"));
            }
            let from = offset - (self.pcm_next - self.pending_len);
            output[n..n + take].copy_from_slice(&self.pending[from..from + take]);
            self.position += take;
            n += take;
        }
        if n > 0 {
            let mut trace = self.trace.lock().unwrap();
            trace.reads += 1;
            trace.high_water = trace.high_water.max(self.position);
        }
        Ok(n)
    }
}

impl Seek for SyntheticWave {
    fn seek(&mut self, request: SeekFrom) -> io::Result<u64> {
        let next = match request {
            SeekFrom::Start(position) => i64::try_from(position).ok(),
            SeekFrom::Current(delta) => (self.position as i64).checked_add(delta),
            SeekFrom::End(delta) => (self.size as i64).checked_add(delta),
        }
        .filter(|position| (0..=self.size as i64).contains(position))
        .ok_or_else(|| io::Error::other("invalid proof byte seek"))? as usize;
        if matches!(request, SeekFrom::Start(_)) {
            self.trace
                .lock()
                .unwrap()
                .absolute_seeks
                .push((self.position, next));
        }
        self.position = next;
        Ok(self.position as u64)
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        Ok(self.position as u64)
    }
}

fn seconds(frames: usize) -> f64 {
    frames as f64 / f64::from(RATE)
}

fn wait_for(engine: &mut MpvEngine, mut check: impl FnMut(&EngineSnapshot) -> bool) {
    let until = Instant::now() + Duration::from_secs(8);
    loop {
        let snapshot = engine.poll().unwrap();
        if check(&snapshot) {
            return;
        }
        assert!(Instant::now() < until, "mpv timed out: {snapshot:?}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires an installed compatible libmpv; runs real decode with null output"]
fn limiter_host_stream_clock_and_discontinuity() {
    let mut engine = MpvEngine::open(MpvConfig {
        library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
        null_output: true,
    })
    .unwrap();
    eprintln!(
        "libmpv {:?}; client API {:#x}",
        engine.property(c"mpv-version"),
        engine.api_version()
    );
    engine.control(&["set", "options/cache", "yes"]).unwrap();
    let cancel = Cancellation::default();
    let full = Arc::new(Mutex::new(Trace::default()));
    let original = engine
        .load_pinned(
            Box::new(SyntheticWave::new(0, full.clone())),
            "wav",
            cancel.clone(),
            &cancel,
        )
        .unwrap();
    assert_eq!(original.codec.as_deref(), Some("pcm_f32le"));
    assert!((original.duration.unwrap() - seconds(SOURCE_END + 1)).abs() < 0.0001);
    wait_for(&mut engine, |s| {
        full.lock().unwrap().generations_drained > 0
            && s.paused
            && s.buffered_through
                .is_some_and(|position| position > seconds(RESTART + QUANTUM * 2))
    });
    let seeks_before = {
        let trace = full.lock().unwrap();
        assert_eq!(trace.high_water, HEADER + (SOURCE_END + 1) * FRAME_BYTES);
        let final_input = trace
            .quanta
            .iter()
            .find(|q| q.input_end && q.timing.output_end)
            .unwrap();
        assert_eq!(final_input.input_frames, 1, "empty FIFO EOF sentinel");
        trace.absolute_seeks.len()
    };
    engine.seek(seconds(RESTART)).unwrap();
    wait_for(&mut engine, |s| {
        s.position_for(original.entry_id)
            .is_some_and(|position| (position - seconds(RESTART)).abs() < 0.05)
    });
    {
        let trace = full.lock().unwrap();
        eprintln!(
            "native seek-to-{:.3}s: byte seeks before={}, after={}, offsets={:?}",
            seconds(RESTART),
            seeks_before,
            trace.absolute_seeks.len(),
            trace.absolute_seeks
        );
        assert_eq!(
            trace.absolute_seeks.len(),
            seeks_before,
            "expected cached seek to avoid the underlying reader on this runtime"
        );
    }

    // Same byte offset in the original generation preserves old limiter
    // history; the replacement generation must apply a fresh detector state.
    let sample = |start, relative_frame| {
        let mut wave = SyntheticWave::new(start, Arc::new(Mutex::new(Trace::default())));
        let mut pair = [0_u8; FRAME_BYTES];
        let offset = (HEADER + relative_frame * FRAME_BYTES) as u64;
        wave.seek(SeekFrom::Start(offset)).unwrap();
        wave.read_exact(&mut pair).unwrap();
        // Seek backwards across many DSP calls; rerender immutable bytes.
        wave.seek(SeekFrom::Start(HEADER as u64)).unwrap();
        let mut first = [0; FRAME_BYTES];
        wave.read_exact(&mut first).unwrap();
        wave.seek(SeekFrom::Start(offset)).unwrap();
        let mut repeated = [0; FRAME_BYTES];
        wave.read_exact(&mut repeated).unwrap();
        assert_eq!(repeated, pair);
        pair.chunks_exact(4)
            .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
            .collect::<Vec<_>>()
    };
    let stale = sample(0, RESTART);
    let fresh = sample(RESTART, 0);
    eprintln!("stereo at source frame {RESTART}: cached {stale:?}, new generation {fresh:?}");
    assert!(stale[0] > 0.0 && stale[0] < fresh[0]);
    assert_eq!(fresh[0], 0.5);
    for pair in [stale, fresh] {
        assert_eq!(pair[1], -pair[0] * 0.125, "both channels share one gain");
    }

    let tail = Arc::new(Mutex::new(Trace::default()));
    let restarted = engine
        .load_pinned(
            Box::new(SyntheticWave::new(RESTART, tail.clone())),
            "wav",
            cancel.clone(),
            &cancel,
        )
        .unwrap();
    assert_ne!(restarted.entry_id, original.entry_id);
    assert_eq!(restarted.codec.as_deref(), Some("pcm_f32le"));
    let remaining = seconds(SOURCE_END - RESTART);
    assert!((restarted.duration.unwrap() - remaining).abs() < 0.0001);
    engine.pause(false).unwrap();
    let mut source_clock = seconds(RESTART);
    let mut advancing_ticks = 0;
    let mut observed_null_output = false;
    wait_for(&mut engine, |s| {
        observed_null_output |= s.audio_output.as_deref() == Some("null");
        if let Some(position) = s.position_for(restarted.entry_id) {
            let mapped = seconds(RESTART) + position;
            assert!(
                mapped + 0.02 >= source_clock,
                "mapped clock moved backwards"
            );
            advancing_ticks += usize::from(mapped > source_clock);
            source_clock = source_clock.max(mapped);
            assert!(source_clock <= seconds(SOURCE_END) + 0.02);
        }
        s.events.iter().any(|event| {
            matches!(event, EngineEvent::Ended {
            entry_id, reason: EndReason::Eof, error: 0
        } if *entry_id == restarted.entry_id)
        })
    });
    // The last time-pos tick need not equal the container duration: it can
    // disappear at end-file while the AO finishes its last queued samples.
    // EOF and the reader's exact emitted length are checked separately.
    assert!(
        observed_null_output && advancing_ticks > 2 && source_clock >= seconds(RESTART) + 0.25,
        "only {advancing_ticks} native ticks, final observed source clock {source_clock:.3}s"
    );
    let tail = tail.lock().unwrap();
    let first = tail.quanta.first().unwrap();
    let last = tail
        .quanta
        .iter()
        .find(|q| q.input_end && q.timing.output_end)
        .unwrap();
    assert!(first.timing.skip_output_frames > 0);
    assert!(first.timing.output_frames < first.input_frames);
    assert_eq!(first.timestamp_ms, (RESTART * 1000 / RATE as usize) as u32);
    assert!(last.input_end && last.timing.output_end && last.timing.pad_input_frames > 0);
    assert!(last.timing.output_frames > last.input_frames);
    assert_eq!(
        tail.high_water,
        HEADER + (SOURCE_END - RESTART) * FRAME_BYTES
    );
    assert!(tail.generations_drained > 0);
    eprintln!(
        "restart native clock reached {:.3}s, then EOF; bytes {}, reads {}; first {first:?}; final {last:?}",
        source_clock, tail.high_water, tail.reads
    );
}
