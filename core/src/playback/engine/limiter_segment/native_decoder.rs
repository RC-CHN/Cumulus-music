//! Opt-in native FFmpeg CLI fixture, not an installed-app dependency.
use super::{DecodedPacket, DecoderPayload, Generation, LimiterSegment, SegmentRead};
use crate::{Error, Result, effects::material::material_limiter_plan};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, BufWriter, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) const RATE: u32 = 44_100;
pub(super) const CHANNELS: usize = 2;
pub(super) const QUANTUM: u32 = 2048;
const PACKET_FRAMES: usize = 613;
const PACKET_BYTES: usize = PACKET_FRAMES * CHANNELS * size_of::<f32>();
const QUEUE_PACKETS: usize = 2;
pub(super) const SOURCE_END: u64 = 98_304;
pub(super) const SEEK_FRAME: u64 = 40_500;

mod pipeline;

struct NativeProducer {
    packets: Option<Receiver<DecodedPacket>>,
    worker: Option<JoinHandle<()>>,
    child: Arc<Mutex<Child>>,
    attempted: Arc<AtomicUsize>,
}

impl NativeProducer {
    fn start(binary: &OsString, path: &Path, generation: Generation, first: u64) -> Self {
        let mut child = Command::new(binary)
            .args([
                "-nostdin",
                "-v",
                "error",
                "-xerror",
                "-protocol_whitelist",
                "file,pipe",
                "-f",
                "flac",
                "-i",
            ])
            .arg(path)
            .args(["-map", "0:a:0", "-af"])
            // Exact source sample for this lossless probe. Decoding the whole
            // prefix is an intentional proof limit, not a production seek.
            .arg(format!("atrim=start_sample={first}"))
            .args(["-c:a", "pcm_f32le", "-f", "f32le", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("trusted FFmpeg fixture decoder");
        let stdout = child.stdout.take().expect("piped decoder output");
        let child = Arc::new(Mutex::new(child));
        let child_for_worker = child.clone();
        let (send, packets) = sync_channel(QUEUE_PACKETS);
        let attempted = Arc::new(AtomicUsize::new(0));
        let attempted_for_worker = attempted.clone();
        let worker = thread::spawn(move || {
            if let Err(error) = decode(
                stdout,
                &child_for_worker,
                &send,
                &attempted_for_worker,
                generation,
                first,
            ) {
                let _ = send.send(DecodedPacket {
                    generation,
                    first_source_frame: first,
                    payload: DecoderPayload::Failed(error),
                });
            }
        });
        Self {
            packets: Some(packets),
            worker: Some(worker),
            child,
            attempted,
        }
    }

    fn receive(&self) -> Result<Option<DecodedPacket>> {
        match self.packets.as_ref().expect("active decoder").try_recv() {
            Ok(packet) => Ok(Some(packet)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                Err(Error::State("limiter decoder closed without EOF"))
            }
        }
    }
}

impl Drop for NativeProducer {
    fn drop(&mut self) {
        // First wake a producer blocked on the bounded queue, then wake a
        // producer blocked in pipe read. No lock is held across the join.
        self.packets.take();
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Ok(mut child) = self.child.lock() {
            let _ = child.wait();
        }
    }
}

fn decode(
    mut stdout: impl Read,
    child: &Mutex<Child>,
    send: &SyncSender<DecodedPacket>,
    attempted: &AtomicUsize,
    generation: Generation,
    mut next_frame: u64,
) -> io::Result<()> {
    let mut bytes = [0; PACKET_BYTES];
    loop {
        let mut len = 0;
        while len < bytes.len() {
            match stdout.read(&mut bytes[len..]) {
                Ok(0) => break,
                Ok(n) => len += n,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        if len == 0 {
            break;
        }
        if !len.is_multiple_of(CHANNELS * size_of::<f32>()) {
            return Err(io::Error::other("partial frame from fixture decoder"));
        }
        let pcm = bytes[..len]
            .chunks_exact(size_of::<f32>())
            .map(|v| f32::from_le_bytes(v.try_into().expect("one PCM sample")))
            .collect();
        let packet = DecodedPacket {
            generation,
            first_source_frame: next_frame,
            payload: DecoderPayload::Samples(pcm),
        };
        attempted.fetch_add(1, Ordering::Release);
        if send.send(packet).is_err() {
            return Ok(()); // old generation cancelled
        }
        next_frame += (len / (CHANNELS * size_of::<f32>())) as u64;
    }
    loop {
        // Never wait while holding the mutex: Drop must be able to kill a
        // child that stopped writing stdout but is still running.
        let status = child.lock().unwrap().try_wait()?;
        if let Some(status) = status {
            if !status.success() {
                return Err(io::Error::other(format!(
                    "fixture decoder failed: {status}"
                )));
            }
            let _ = send.send(DecodedPacket {
                generation,
                first_source_frame: next_frame,
                payload: DecoderPayload::End,
            });
            return Ok(());
        }
        thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn input_pair(frame: u64) -> [i16; CHANNELS] {
    let left = if frame < SEEK_FRAME {
        24_576
    } else {
        4096 + 8 * (frame % 251) as i16
    };
    [left, -left / 8]
}

fn make_flac(binary: &OsString, dir: &Path) -> std::path::PathBuf {
    let wav = dir.join("source.wav");
    let flac = dir.join("source.flac");
    let mut out = BufWriter::new(File::create(&wav).unwrap());
    let data = SOURCE_END as u32 * CHANNELS as u32 * size_of::<i16>() as u32;
    out.write_all(b"RIFF").unwrap();
    out.write_all(&(36 + data).to_le_bytes()).unwrap();
    out.write_all(b"WAVEfmt ").unwrap();
    out.write_all(&[16, 0, 0, 0, 1, 0, 2, 0]).unwrap();
    out.write_all(&RATE.to_le_bytes()).unwrap();
    out.write_all(&(RATE * 4).to_le_bytes()).unwrap();
    out.write_all(&[4, 0, 16, 0]).unwrap();
    out.write_all(b"data").unwrap();
    out.write_all(&data.to_le_bytes()).unwrap();
    for frame in 0..SOURCE_END {
        for sample in input_pair(frame) {
            out.write_all(&sample.to_le_bytes()).unwrap();
        }
    }
    out.flush().unwrap();
    let status = Command::new(binary)
        .args([
            "-nostdin",
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(wav)
        .args(["-map", "0:a:0", "-c:a", "flac"])
        .arg(&flac)
        .status()
        .unwrap();
    assert!(status.success(), "synthetic FLAC creation failed");
    flac
}

pub(super) fn make_segment() -> LimiterSegment {
    let plan = material_limiter_plan(&serde_json::json!({
        "aev2": { "limiter": { "on": true, "threshold": -12.0, "attack": 0.005, "release": 0.1, "outputgain": 0.0, "infiniteRatio": true } }
    })).unwrap();
    assert_eq!(plan.parameters.threshold, -12.0);
    LimiterSegment::new(&plan, RATE, CHANNELS as u32, QUANTUM).unwrap()
}

pub(super) fn until<T>(mut poll: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = poll() {
            return value;
        }
        assert!(Instant::now() < deadline, "native decoder probe timed out");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "requires a trusted FFmpeg executable; decodes synthetic FLAC without audio hardware"]
fn limiter_segment_real_decode_pause_seek_and_eof() {
    let binary =
        std::env::var_os("CUMULUS_LIMITER_PROBE_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
    let dir = tempfile::tempdir().unwrap();
    let flac = make_flac(&binary, dir.path());
    let mut segment = make_segment();
    let old = segment.start(0, SOURCE_END).unwrap();
    let producer = NativeProducer::start(&binary, &flac, old, 0);
    let mut output = [0.0; 379 * CHANNELS];
    until(
        || match segment.read(&mut output, || producer.receive()).unwrap() {
            SegmentRead::Pending => None,
            SegmentRead::Audio {
                generation,
                source_clock,
                ..
            } => {
                assert_eq!(generation, old);
                assert!(source_clock > 0);
                assert!(0.0 < output[0] && output[0] < 0.75);
                Some(())
            }
            state => panic!("unexpected original source state: {state:?}"),
        },
    );
    segment.pause(true);
    let paused_clock = segment.next_rendered_frame;
    assert_eq!(
        segment
            .read(&mut output, || panic!("read while paused"))
            .unwrap(),
        SegmentRead::Paused
    );
    until(|| {
        // Two queued packets and one in the producer's blocked send.
        let attempted = producer.attempted.load(Ordering::Acquire);
        (attempted * PACKET_FRAMES
            >= segment.next_decoder_frame as usize + (QUEUE_PACKETS + 1) * PACKET_FRAMES)
            .then_some(attempted)
    });
    let paused_attempts = producer.attempted.load(Ordering::Acquire);
    thread::sleep(Duration::from_millis(15));
    assert_eq!(producer.attempted.load(Ordering::Acquire), paused_attempts);
    assert_eq!(segment.next_rendered_frame, paused_clock);
    segment.pause(false);
    assert!(matches!(
        segment.read(&mut output, || panic!("resume dropped buffered PCM")).unwrap(),
        SegmentRead::Audio { generation, first_source_frame, source_clock, .. }
            if generation == old && first_source_frame == paused_clock && source_clock > paused_clock
    ));
    segment.pause(true);
    let dropped_packet = producer.receive().unwrap().unwrap();
    until(|| (producer.attempted.load(Ordering::Acquire) > paused_attempts).then_some(()));

    let new = segment.start(SEEK_FRAME, SOURCE_END).unwrap();
    let old_child = producer.child.clone();
    drop(producer); // drops queued old PCM, interrupts reads and reaps the child
    assert!(old_child.lock().unwrap().try_wait().unwrap().is_some());
    assert_ne!(old, new);
    assert_eq!(
        segment
            .read(&mut output, || panic!("seek cleared user pause"))
            .unwrap(),
        SegmentRead::Paused
    );
    let producer = NativeProducer::start(&binary, &flac, new, SEEK_FRAME);
    segment.pause(false);
    let mut late = Some(dropped_packet);
    let mut rendered = 0_u64;
    until(|| {
        match segment
            .read(&mut output, || {
                if late.is_some() {
                    Ok(late.take())
                } else {
                    producer.receive()
                }
            })
            .unwrap()
        {
            SegmentRead::Pending => None,
            SegmentRead::Audio {
                generation,
                first_source_frame,
                frames,
                source_clock,
            } => {
                assert_eq!(generation, new);
                assert_eq!(first_source_frame, SEEK_FRAME + rendered);
                for (offset, actual) in output[..frames as usize * CHANNELS]
                    .chunks_exact(CHANNELS)
                    .enumerate()
                {
                    let expected = input_pair(first_source_frame + offset as u64)
                        .map(|v| f32::from(v) / 32768.0);
                    assert_eq!(actual, expected, "wrong decoded/source frame after restart");
                }
                rendered += u64::from(frames);
                assert_eq!(source_clock, SEEK_FRAME + rendered);
                None
            }
            SegmentRead::End {
                generation,
                source_clock,
            } => {
                assert_eq!(generation, new);
                assert_eq!(source_clock, SOURCE_END);
                Some(())
            }
            state => panic!("unexpected restarted source state: {state:?}"),
        }
    });
    assert!(late.is_none());
    assert_eq!(rendered, SOURCE_END - SEEK_FRAME);
    drop(producer);

    // A failed decoder is not clean EOF. The failed generation stays closed
    // until a new explicit seek/load resets both the DSP and the fault.
    let bad = dir.path().join("bad.flac");
    std::fs::write(&bad, b"not a compressed audio stream").unwrap();
    let failed = segment.start(0, SOURCE_END).unwrap();
    let producer = NativeProducer::start(&binary, &bad, failed, 0);
    until(|| match segment.read(&mut output, || producer.receive()) {
        Ok(SegmentRead::Pending) => None,
        Err(Error::Io(error)) => {
            assert!(error.to_string().contains("fixture decoder failed"));
            Some(())
        }
        state => panic!("unexpected corrupt-source state: {state:?}"),
    });
    assert!(
        segment
            .read(&mut output, || panic!("read after fault"))
            .is_err()
    );
    drop(producer);
    let recovered = segment.start(SOURCE_END, SOURCE_END).unwrap();
    let producer = NativeProducer::start(&binary, &flac, recovered, SOURCE_END);
    let sentinel = until(
        || match segment.read(&mut output, || producer.receive()).unwrap() {
            SegmentRead::Pending => None,
            event => Some(event),
        },
    );
    assert_eq!(
        sentinel,
        SegmentRead::Audio {
            generation: recovered,
            first_source_frame: SOURCE_END,
            frames: 1,
            source_clock: SOURCE_END
        }
    );
    assert_eq!(&output[..CHANNELS], &[0.0, 0.0]);
    assert_eq!(
        segment
            .read(&mut output, || panic!("polled after EOF"))
            .unwrap(),
        SegmentRead::End {
            generation: recovered,
            source_clock: SOURCE_END
        }
    );
    eprintln!(
        "decoded native FLAC; pause at source frame {paused_clock} bounded producer at {paused_attempts} packet attempts; reset to {SEEK_FRAME} and checked all {rendered} decoded stereo frames to EOF; decoder failure was terminal and recovered at exact end with one silent frame and unchanged source clock"
    );
}
