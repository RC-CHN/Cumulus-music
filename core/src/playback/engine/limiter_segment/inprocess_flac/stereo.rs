//! Held audio sources for the recovered local equalizer graph.
mod graph;
mod native;
mod wave;
use super::{Api, Decoder, FLAC_BLOCK, Generation, MAX_COMPRESSED, RATE};
use crate::{
    Error, Result,
    download::offline::{ColdOfflineFlacAuthority, ColdOfflineFlacReader},
    effects::{
        dsp::{Format, PlanarBlock},
        stereo::LocalStereoMaterialPlan,
    },
    playback::engine::{
        Cancellation, StereoPcmControl, StereoPcmReader,
        limiter_segment::{DecodedPacket, DecoderPayload},
        pcm_wave::PcmWave,
        stereo_pcm::{Shared, channel, native_channel},
    },
};
use graph::SourceGraph;
pub(crate) use native::{
    NativeStereoPass, NativeStereoSelection, NativeStereoSource, NativeStereoStart,
};
use std::{
    collections::VecDeque,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const QUANTUM: usize = 2048;
const PACKET_FRAMES: usize = FLAC_BLOCK as usize;
const WAIT: Duration = Duration::from_secs(3);

pub enum ColdStereoSelection {
    Qualified(Box<ColdStereoSource>),
    /// The exact held reader remains available for the existing stock path.
    Stock(ColdOfflineFlacReader),
}
pub enum ColdStereoStart {
    Ready(Box<ColdStereoPass>),
    /// Preparation/seek failed before a native sink was published. Authority
    /// is checked again, and the same held descriptor/graph are returned.
    Stock(Box<ColdStereoSource>),
}

pub struct ColdStereoSource(HeldSource<ColdOfflineFlacReader>);
pub struct ColdStereoPass(HeldPass<ColdOfflineFlacReader>);

impl ColdStereoSource {
    /// Native opt-in only. Inspect the actual authenticated STREAMINFO first;
    /// unknown/mono/other formats or absent trusted runtime preserve stock.
    /// No account cookies, fresh authorization URL, path reopen or child is used.
    pub fn select(
        reader: ColdOfflineFlacReader,
        library: &Path,
        plan: LocalStereoMaterialPlan,
    ) -> Result<ColdStereoSelection> {
        Ok(
            match HeldSource::select_flac(reader, library, FlacRates::Cold, |format| {
                SourceGraph::new(format, plan)
            })? {
                Selection::Qualified(source) => {
                    ColdStereoSelection::Qualified(Box::new(Self(*source)))
                }
                Selection::Stock(reader) => ColdStereoSelection::Stock(reader),
            },
        )
    }

    pub fn total_frames(&self) -> u64 {
        self.0.total
    }

    pub fn into_reader(self) -> Result<ColdOfflineFlacReader> {
        self.0.into_reader()
    }

    /// A replacement pass can only be made from a reclaimed source.
    pub fn start(self, first: u64, cancel: &Cancellation) -> Result<ColdStereoStart> {
        Ok(match self.0.start(first, cancel)? {
            Start::Ready(pass) => ColdStereoStart::Ready(Box::new(ColdStereoPass(*pass))),
            Start::Stock(source) => ColdStereoStart::Stock(Box::new(Self(*source))),
        })
    }
}

impl ColdStereoPass {
    pub fn control(&self) -> StereoPcmControl {
        self.0.control.clone()
    }
    pub fn take_reader(&mut self) -> Result<StereoPcmReader> {
        self.0.take_reader()
    }
    pub fn reclaim(self) -> Result<ColdStereoSource> {
        self.0.reclaim().map(ColdStereoSource)
    }
}

// The cold grant and the native file lifetime are deliberately distinct;
// sharing the decoder owner never transfers account/download authority.
enum SourceAuthority {
    Cold(ColdOfflineFlacAuthority),
    Native(Cancellation),
}
impl SourceAuthority {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Cold(authority) => authority.validate(),
            Self::Native(lifetime) => lifetime.check(),
        }
    }
    fn lifetime(&self) -> Cancellation {
        match self {
            Self::Cold(authority) => authority.cancellation(),
            Self::Native(lifetime) => lifetime.clone(),
        }
    }
}
trait HeldInput: Read + Seek + Send + 'static {
    fn source_authority(&self) -> SourceAuthority;
}
impl HeldInput for ColdOfflineFlacReader {
    fn source_authority(&self) -> SourceAuthority {
        SourceAuthority::Cold(self.authority())
    }
}
enum Selection<R> {
    Qualified(Box<HeldSource<R>>),
    Stock(R),
}
enum Start<R> {
    Ready(Box<HeldPass<R>>),
    Stock(Box<HeldSource<R>>),
}
struct HeldSource<R> {
    reader: R,
    decoder: DecoderKind,
    graph: SourceGraph,
    rate: u32,
    total: u64,
}
enum FlacRates {
    Cold,
    Local,
}
impl FlacRates {
    fn allows(&self, rate: u32) -> bool {
        rate == RATE || (matches!(self, Self::Local) && rate == 48_000)
    }
}
enum DecoderKind {
    Flac(Api),
    Wave(PcmWave),
}
struct HeldPass<R> {
    reader: Option<StereoPcmReader>,
    control: StereoPcmControl,
    retired: Receiver<HeldSource<R>>,
    worker: Option<JoinHandle<()>>,
}
impl<R: HeldInput> HeldSource<R> {
    fn select_flac(
        mut reader: R,
        library: &Path,
        rates: FlacRates,
        graph: impl FnOnce(Format) -> Result<SourceGraph>,
    ) -> Result<Selection<R>> {
        let authority = reader.source_authority();
        authority.validate()?;
        let length = reader.seek(SeekFrom::End(0))?;
        reader.rewind()?;
        if !(42..=MAX_COMPRESSED).contains(&length) {
            return Ok(Selection::Stock(reader));
        }
        let mut header = [0; 42];
        reader.read_exact(&mut header)?;
        reader.rewind()?;
        if &header[..4] != b"fLaC" || header[4] & 127 != 0 || header[5..8] != [0, 0, 34] {
            return Ok(Selection::Stock(reader));
        }
        let min_block = u16::from_be_bytes([header[8], header[9]]);
        let max_block = u16::from_be_bytes([header[10], header[11]]);
        let format = u64::from_be_bytes(header[18..26].try_into().expect("STREAMINFO"));
        let rate = (format >> 44) as u32;
        let channels = ((format >> 41) & 7) + 1;
        let bits = ((format >> 36) & 31) + 1;
        let total = format & ((1 << 36) - 1);
        authority.validate()?;
        if min_block < 16
            || min_block > max_block
            || u32::from(max_block) > FLAC_BLOCK
            || !rates.allows(rate)
            || channels != 2
            || bits != 16
            || !(1..=u64::from(rate) * 3600).contains(&total)
        {
            return Ok(Selection::Stock(reader));
        }
        let Ok(api) = Api::open(library) else {
            authority.validate()?;
            return Ok(Selection::Stock(reader));
        };
        Self::qualified(reader, DecoderKind::Flac(api), rate, total, graph)
    }

    fn qualified(
        reader: R,
        decoder: DecoderKind,
        rate: u32,
        total: u64,
        graph: impl FnOnce(Format) -> Result<SourceGraph>,
    ) -> Result<Selection<R>> {
        let authority = reader.source_authority();
        authority.validate()?;
        let graph = match graph(Format::new(rate as f32, 2, QUANTUM as u32)?) {
            Ok(graph) => graph,
            Err(_) => {
                authority.validate()?;
                return Ok(Selection::Stock(reader));
            }
        };
        Ok(Selection::Qualified(Box::new(Self {
            reader,
            decoder,
            graph,
            rate,
            total,
        })))
    }

    fn into_reader(mut self) -> Result<R> {
        self.reader.source_authority().validate()?;
        self.reader.rewind()?;
        Ok(self.reader)
    }

    /// A replacement pass can only be made from a reclaimed source. Reclaim
    /// withholds ownership until the old native reader really closed and the
    /// old decoder worker exited. Same-graph reset retains the original gains.
    fn start(self, first: u64, cancel: &Cancellation) -> Result<Start<R>> {
        self.reader.source_authority().validate()?;
        cancel.check()?;
        if first >= self.total {
            return Err(Error::InvalidInput("stereo seek outside source"));
        }
        let actual = self.reader.source_authority();
        let source = actual.lifetime();
        let request = cancel.clone();
        let preparing = Arc::new(AtomicBool::new(true));
        let in_prepare = preparing.clone();
        // The caller's request cancels preparation, not a completed media
        // lifetime. Active playback stays bound to the held reader's actual
        // source lease and this pass's explicit native cancellation.
        let guard = Cancellation::new(move || {
            !source.is_cancelled()
                && (!in_prepare.load(Ordering::Acquire) || !request.is_cancelled())
        });
        let pipe = match actual {
            SourceAuthority::Cold(grant) => channel(RATE, first, self.total, guard, Some(grant)),
            SourceAuthority::Native(_) => native_channel(self.rate, first, self.total, guard),
        };
        let (reader, control, shared) = match pipe {
            Ok(pipe) => pipe,
            Err(_) => {
                self.reader.source_authority().validate()?;
                cancel.check()?;
                return Ok(Start::Stock(Box::new(self)));
            }
        };
        let (ready, status) = mpsc::sync_channel(1);
        let (retire, retired) = mpsc::sync_channel(1);
        // Keep ownership recoverable even when OS thread creation fails: a
        // failed spawn drops its closure without taking the sole source.
        let source_slot = Arc::new(Mutex::new(Some(self)));
        let worker_source = source_slot.clone();
        let worker = match thread::Builder::new()
            .name("cumulus-stereo-pcm".into())
            .spawn(move || {
                let Some(mut source) = worker_source.lock().ok().and_then(|mut slot| slot.take())
                else {
                    let _ = ready.send(false);
                    shared.fail();
                    return;
                };
                let result = run(&mut source, first, &shared, &ready);
                if result.is_err() {
                    shared.fail();
                }
                // run has dropped the decoder before returning the sole
                // held reader and the original graph to the retirement channel.
                let _ = retire.send(source);
            }) {
            Ok(worker) => worker,
            Err(_) => {
                drop(reader);
                let source = source_slot
                    .lock()
                    .map_err(|_| Error::LockPoisoned)?
                    .take()
                    .ok_or(Error::State(
                        "stereo source ownership missing after failed spawn",
                    ))?;
                source.reader.source_authority().validate()?;
                cancel.check()?;
                return Ok(Start::Stock(Box::new(source)));
            }
        };
        let pass = HeldPass {
            reader: Some(reader),
            control,
            retired,
            worker: Some(worker),
        };
        match status.recv_timeout(WAIT) {
            Ok(true) => {
                pass.control.validate_host()?;
                cancel.check()?;
                preparing.store(false, Ordering::Release);
                Ok(Start::Ready(Box::new(pass)))
            }
            Ok(false) => {
                let source = pass.reclaim()?;
                source.reader.source_authority().validate()?;
                cancel.check()?;
                Ok(Start::Stock(Box::new(source)))
            }
            Err(_) => Err(Error::State("stereo decoder preparation timed out")),
        }
    }
}

impl<R: HeldInput> HeldPass<R> {
    fn take_reader(&mut self) -> Result<StereoPcmReader> {
        self.control.validate_host()?;
        self.reader
            .take()
            .ok_or(Error::State("stereo reader was already published"))
    }
    fn reclaim(mut self) -> Result<HeldSource<R>> {
        // A reader still held here was never published to a sink.
        drop(self.reader.take());
        let state = self.control.snapshot()?;
        if !state.reader_closed || !state.native_sink_retired {
            return Err(Error::State("old native stereo reader is not closed"));
        }
        self.control.cancel();
        let deadline = Instant::now() + WAIT;
        let source = self
            .retired
            .recv_timeout(WAIT)
            .map_err(|_| Error::State("stereo worker retirement timed out"))?;
        let worker = self.worker.take().expect("owned stereo worker");
        while !worker.is_finished() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::State("stereo worker retirement timed out"));
            }
            thread::park_timeout(remaining.min(Duration::from_millis(1)));
        }
        worker
            .join()
            .map_err(|_| Error::State("stereo worker panicked"))?;
        source.reader.source_authority().validate()?;
        Ok(source)
    }
}
impl<R> Drop for HeldPass<R> {
    fn drop(&mut self) {
        self.control.cancel();
        drop(self.reader.take());
        if let Some(worker) = self.worker.take().filter(JoinHandle::is_finished) {
            let _ = worker.join();
        }
        // A stalled worker keeps ownership; drop never lends its reader to a
        // second sink or blocks the UI indefinitely.
    }
}

fn run<R: HeldInput>(
    source: &mut HeldSource<R>,
    first: u64,
    shared: &Arc<Shared>,
    ready: &mpsc::SyncSender<bool>,
) -> Result<()> {
    let generation = if first == 0 {
        Generation(1)
    } else {
        Generation(2)
    };
    let next = &mut source.reader;
    let graph = &mut source.graph;
    let rate = source.rate;
    let total = source.total;
    match &source.decoder {
        DecoderKind::Flac(api) => {
            let mut decoder = match Decoder::open_with_rate(
                api,
                Box::new(next),
                Generation(1),
                shared.authority_clone(),
                rate,
            ) {
                Ok(decoder) if decoder.total == total => decoder,
                _ => {
                    let _ = ready.send(false);
                    return Err(Error::Media("stereo decoder preparation failed"));
                }
            };
            if first > 0 && decoder.seek(generation, first).is_err() {
                let _ = ready.send(false);
                return Err(Error::Media("stereo decoder seek preparation failed"));
            }
            run_ready(graph, first, total, shared, ready, || {
                let packet = decoder.next()?;
                if packet.as_ref().is_some_and(|p| p.generation != generation) {
                    return Err(io::Error::other("wrong stereo decoder generation"));
                }
                Ok(packet)
            })
        }
        DecoderKind::Wave(layout) => {
            let mut decoder = match PcmWave::inspect(next).and_then(|current| {
                if current != *layout {
                    return Err(Error::Media("held PCM WAV layout changed"));
                }
                wave::WavePackets::open(next, current, first, generation)
            }) {
                Ok(decoder) => decoder,
                Err(error) => {
                    let _ = ready.send(false);
                    return Err(error);
                }
            };
            run_ready(graph, first, total, shared, ready, || decoder.next())
        }
    }
}

fn run_ready(
    graph: &mut SourceGraph,
    first: u64,
    total: u64,
    shared: &Shared,
    ready: &mpsc::SyncSender<bool>,
    next: impl FnMut() -> io::Result<Option<DecodedPacket>>,
) -> Result<()> {
    graph.prepare(first)?;
    shared.check()?;
    if ready.send(true).is_err() {
        return Err(Error::Cancelled);
    }
    pump(graph, first, total, shared, next)
}

fn pump(
    graph: &mut SourceGraph,
    first: u64,
    total: u64,
    shared: &Shared,
    mut next: impl FnMut() -> io::Result<Option<DecodedPacket>>,
) -> Result<()> {
    let mut input = VecDeque::new();
    input
        .try_reserve_exact((QUANTUM + PACKET_FRAMES) * 2)
        .map_err(|_| Error::Media("stereo decoder FIFO allocation failed"))?;
    let mut block = PlanarBlock::new(graph.format());
    let mut output = [0.0; QUANTUM * 2];
    let mut cursor = first;
    let mut source_end = false;
    let mut first_end = true;
    let mut zero_steps = 0;
    loop {
        shared.wait_for_credit(|control| graph.apply(control))?;
        while input.len() < QUANTUM * 2 && !source_end {
            shared.check()?;
            let packet = next()?.ok_or(Error::Media("stereo decoder omitted verified EOF"))?;
            shared.check()?;
            if packet.first_source_frame != cursor {
                return Err(Error::Media("noncontiguous stereo decoder output"));
            }
            match packet.payload {
                DecoderPayload::Samples(pcm) => {
                    let frames = pcm.len() / 2;
                    if frames == 0
                        || frames > PACKET_FRAMES
                        || !pcm.len().is_multiple_of(2)
                        || pcm.iter().any(|v| !v.is_finite())
                        || cursor + frames as u64 > total
                        || input.len() + pcm.len() > (QUANTUM + PACKET_FRAMES) * 2
                    {
                        return Err(Error::Media("invalid stereo decoder output"));
                    }
                    cursor += frames as u64;
                    input.extend(pcm);
                }
                DecoderPayload::End if cursor == total => source_end = true,
                _ => return Err(Error::Media("stereo decoder ended before source range")),
            }
        }
        let frames = (input.len() / 2).min(QUANTUM);
        let sentinel = source_end && first_end && frames == 0;
        block.samples_mut().fill(0.0);
        for frame in 0..frames {
            for channel in 0..2 {
                block.samples_mut()[channel * QUANTUM + frame] =
                    input.pop_front().expect("complete stereo frame");
            }
        }
        let end = source_end && input.is_empty();
        let flow = graph.process(&mut block, frames + usize::from(sentinel), end)?;
        if end {
            first_end = false;
        }
        block.copy_interleaved(flow.valid_frames, &mut output[..flow.valid_frames * 2])?;
        shared.publish(&output[..flow.valid_frames * 2], cursor, flow.end)?;
        if flow.end {
            return Ok(());
        }
        zero_steps = if flow.valid_frames == 0 {
            zero_steps + 1
        } else {
            0
        };
        if zero_steps > 32 {
            return Err(Error::Media("stereo graph made no bounded progress"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan(presence: i32) -> LocalStereoMaterialPlan {
        LocalStereoMaterialPlan::from_equalizer_json(&json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":true,"presence":presence,"stereoizer":5,"sshaper":true,"ambience":1}})).unwrap()
    }

    #[test]
    fn paced_real_graph_preserves_variable_tail_and_applies_control_before_short_source_is_decoded()
    {
        let first = 123;
        let total = u64::from(RATE) * 2 + 7;
        let (mut reader, control, shared) =
            channel(RATE, first, total, Cancellation::default(), None).unwrap();
        control.publish_sink().unwrap();
        control.bind_entry(77).unwrap();
        control.observe(77, Some(0.0), false, false).unwrap();
        let producer = shared.clone();
        let worker = thread::spawn(move || {
            let mut graph =
                SourceGraph::new(Format::new(RATE as f32, 2, 2048).unwrap(), plan(3)).unwrap();
            graph.prepare(first).unwrap();
            let mut cursor = first;
            let mut ended = false;
            pump(&mut graph, first, total, &producer, || {
                if cursor == total {
                    assert!(!ended);
                    ended = true;
                    return Ok(Some(DecodedPacket {
                        generation: Generation(1),
                        first_source_frame: cursor,
                        payload: DecoderPayload::End,
                    }));
                }
                let frames = (total - cursor).min(u64::from(FLAC_BLOCK)) as usize;
                let pcm = (0..frames)
                    .flat_map(|i| {
                        let phase = (cursor + i as u64) % 97;
                        [phase as f32 / 256.0 - 0.125, -(phase as f32) / 1024.0]
                    })
                    .collect();
                let packet = DecodedPacket {
                    generation: Generation(1),
                    first_source_frame: cursor,
                    payload: DecoderPayload::Samples(pcm),
                };
                cursor += frames as u64;
                Ok(Some(packet))
            })
            .unwrap();
        });
        let mut bytes = [0; 4093];
        let mut count = 0u64;
        let mut rounds = 0;
        loop {
            let n = reader.read(&mut bytes).unwrap();
            if n == 0 {
                break;
            }
            count += n as u64;
            rounds += 1;
            let observed = count / 8;
            if rounds == 2 {
                control
                    .observe(77, Some(observed as f64 / f64::from(RATE)), true, false)
                    .unwrap();
                let revision = control.configure(plan(-3)).unwrap();
                control.wait_applied(revision, WAIT).unwrap();
                let state = control.snapshot().unwrap();
                assert!(
                    state.decoded_source_frame < total,
                    "bounded rendering must not decode this short source before adjustment"
                );
                assert!(!state.graph_end);
                assert_eq!(control.snapshot().unwrap().applied_revision, revision);
            }
            control
                .observe(77, Some(observed as f64 / f64::from(RATE)), false, false)
                .unwrap();
            let state = control.snapshot().unwrap();
            assert!(
                state.produced_frames
                    <= state.native_output_frame + u64::from(RATE.div_ceil(4)) + QUANTUM as u64
            );
        }
        worker.join().unwrap();
        assert_eq!(count / 8, total - first + u64::from(RATE));
        let state = control.snapshot().unwrap();
        assert!(state.graph_end);
        assert_eq!(state.decoded_source_frame, total);
        assert_eq!(state.native_source_frame, total);
        assert!(
            state.produced_frames > total - first,
            "actual environment tail remains output PCM"
        );
        drop(reader);
        assert!(control.snapshot().unwrap().reader_closed);
        assert!(!control.snapshot().unwrap().native_sink_retired);
        control.retire_sink();
    }
}
