//! Bounded in-process FLAC producer; no child or new Cargo dependency.
use super::{DecodedPacket, DecoderPayload, Generation};
#[cfg(test)]
use super::{LimiterSegment, SegmentRead};
use crate::playback::engine::{Cancellation, PinnedReader};
#[cfg(test)]
use std::path::PathBuf;
use std::{
    ffi::{c_int, c_void},
    io::{self, SeekFrom},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
};

mod abi;
mod cold;
#[cfg(test)]
mod cold_authority;
#[cfg(all(test, windows))]
mod windows_runtime;
use abi::{Api, FrameHeader, Handle};
pub use cold::ColdLimiterWave;
pub(crate) use cold::{ColdLimiterConfig, ColdLimiterRender, ColdLimiterSource};

const RATE: u32 = 44_100;
const CHANNELS: usize = 2;
const FLAC_BLOCK: u32 = 1024;
#[cfg(test)]
const TOTAL: u64 = 1_048_633;
const BYTE_BUDGET: usize = 128 * 1024;
const SCRATCH: usize = 8192;
const MAX_COMPRESSED: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Default)]
struct Stats {
    bytes_read: u64,
    seeks: usize,
    last_seek: u64,
}

struct Source<'reader> {
    input: Box<dyn PinnedReader + 'reader>,
    authority: Cancellation,
    size: u64,
    offset: u64,
    scratch: Box<[u8; SCRATCH]>,
    stats: Stats,
    bytes_left: usize,
    seeks_left: usize,
    generation: Generation,
    next_source_frame: u64,
    pending: Option<DecodedPacket>,
    fault: Option<&'static str>,
}

const READ_BUDGET_EXHAUSTED: &str = "FLAC operation read budget exhausted";

#[derive(Debug)]
struct ReadBudget;

impl std::fmt::Display for ReadBudget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(READ_BUDGET_EXHAUSTED)
    }
}

impl std::error::Error for ReadBudget {}

fn operation_error(fault: &'static str) -> io::Error {
    if fault == READ_BUDGET_EXHAUSTED {
        io::Error::other(ReadBudget)
    } else {
        io::Error::other(fault)
    }
}

impl Source<'_> {
    fn fail(&mut self, message: &'static str) -> Option<c_int> {
        self.fault.get_or_insert(message);
        None
    }

    fn prepare_operation(&mut self) -> io::Result<()> {
        self.authority.check().map_err(io::Error::other)?;
        if let Some(fault) = self.fault {
            return Err(operation_error(fault));
        }
        self.bytes_left = BYTE_BUDGET;
        self.seeks_left = 32;
        Ok(())
    }
}

fn callback(
    cookie: Handle,
    failure: c_int,
    work: impl FnOnce(&mut Source<'_>) -> Option<c_int>,
) -> c_int {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: these callbacks are registered only with Decoder::open's
        // boxed Source. libFLAC calls them synchronously on this one decoder,
        // which does not hold a Rust borrow of Source during any native call.
        // Decoder's source lifetime keeps an optional borrowed reader alive
        // through finish/delete; neither its reference nor the cookie escapes.
        let source = unsafe { cookie.cast::<Source<'_>>().as_mut() }?;
        if source.fault.is_some() || source.authority.is_cancelled() {
            return source.fail("FLAC source revoked or failed");
        }
        work(source)
    }))
    .ok()
    .flatten()
    .unwrap_or(failure)
}

unsafe extern "C" fn read(_: Handle, out: *mut u8, len: *mut usize, cookie: Handle) -> c_int {
    callback(cookie, abi::READ_ABORT, |source| {
        if len.is_null() || out.is_null() {
            return source.fail("invalid FLAC read callback arguments");
        }
        // SAFETY: libFLAC passes its writable byte count in this callback.
        let requested = unsafe { std::mem::replace(&mut *len, 0) };
        if source.offset == source.size {
            return Some(abi::READ_EOF);
        }
        if requested == 0 {
            return source.fail("invalid FLAC read callback length");
        }
        if source.bytes_left == 0 {
            return source.fail(READ_BUDGET_EXHAUSTED);
        }
        let count = requested
            .min(SCRATCH)
            .min(source.bytes_left)
            .min((source.size - source.offset) as usize);
        let actual = match source.input.read(&mut source.scratch[..count]) {
            Ok(actual) if (1..=count).contains(&actual) => actual,
            _ => return source.fail("FLAC source read failed or truncated"),
        };
        // SAFETY: the destination is allocated for requested bytes; scratch
        // owns initialized bytes and does not alias libFLAC's raw buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(source.scratch.as_ptr(), out, actual);
            len.write(actual);
        }
        source.offset += actual as u64;
        source.stats.bytes_read += actual as u64;
        source.bytes_left -= actual;
        Some(abi::OK)
    })
}

unsafe extern "C" fn seek(_: Handle, offset: u64, cookie: Handle) -> c_int {
    callback(cookie, abi::IO_ERROR, |source| {
        if source.seeks_left == 0 || offset > source.size {
            return source.fail("FLAC seek outside operation budget or source");
        }
        if source.input.seek(SeekFrom::Start(offset)).ok() != Some(offset) {
            return source.fail("FLAC source seek failed");
        }
        source.offset = offset;
        source.stats.seeks += 1;
        source.stats.last_seek = offset;
        source.seeks_left -= 1;
        Some(abi::OK)
    })
}

unsafe extern "C" fn tell(_: Handle, out: *mut u64, cookie: Handle) -> c_int {
    callback(cookie, abi::IO_ERROR, |source| {
        if out.is_null() {
            return source.fail("missing FLAC tell destination");
        }
        // SAFETY: libFLAC provides writable storage for a u64 position.
        unsafe { out.write(source.offset) };
        Some(abi::OK)
    })
}

unsafe extern "C" fn length(_: Handle, out: *mut u64, cookie: Handle) -> c_int {
    callback(cookie, abi::IO_ERROR, |source| {
        if out.is_null() {
            return source.fail("missing FLAC length destination");
        }
        // SAFETY: libFLAC provides writable storage for a u64 length.
        unsafe { out.write(source.size) };
        Some(abi::OK)
    })
}

unsafe extern "C" fn eof(_: Handle, cookie: Handle) -> c_int {
    callback(cookie, 1, |source| {
        Some(c_int::from(source.offset == source.size))
    })
}

unsafe extern "C" fn fault(_: Handle, _: c_int, cookie: Handle) {
    callback(cookie, abi::IO_ERROR, |source| {
        source.fail("native FLAC decode error")
    });
}

unsafe extern "C" fn write(
    _: Handle,
    frame: *const FrameHeader,
    channels: *const *const i32,
    cookie: Handle,
) -> c_int {
    callback(cookie, abi::WRITE_ABORT, |source| {
        // SAFETY: the callback receives a public FLAC__Frame whose first
        // member is FLAC__FrameHeader, valid for the callback's duration.
        let header = unsafe { frame.as_ref() }?;
        if channels.is_null()
            || header.channels != CHANNELS as u32
            || header.sample_rate != RATE
            || header.bits_per_sample != 16
            || header.number_type != abi::SAMPLE_NUMBER
            || !(1..=FLAC_BLOCK).contains(&header.blocksize)
            || source.pending.is_some()
        {
            return source.fail("unsupported or overlapping FLAC frame");
        }
        // SAFETY: libFLAC normalized this frame to a sample number; the
        // validated tag selects the active field of the public C union.
        let start = unsafe { header.number.sample };
        if start != source.next_source_frame {
            return source.fail("non-contiguous FLAC callback source frame");
        }
        // SAFETY: the checked stereo frame has two initialized planar
        // pointers, each with header.blocksize samples, including seek trims.
        let (left, right) = unsafe { (*channels, *channels.add(1)) };
        if left.is_null() || right.is_null() {
            return source.fail("missing FLAC PCM plane");
        }
        let frames = header.blocksize as usize;
        // SAFETY: the bounds and lifetimes follow the same write contract.
        let left = unsafe { std::slice::from_raw_parts(left, frames) };
        // SAFETY: the bounds and lifetimes follow the same write contract.
        let right = unsafe { std::slice::from_raw_parts(right, frames) };
        let mut pcm = Vec::with_capacity(frames * CHANNELS);
        for (&l, &r) in left.iter().zip(right) {
            let (Ok(l), Ok(r)) = (i16::try_from(l), i16::try_from(r)) else {
                return source.fail("FLAC PCM exceeds the selected sample depth");
            };
            pcm.extend([f32::from(l) / 32768.0, f32::from(r) / 32768.0]);
        }
        source.pending = Some(DecodedPacket {
            generation: source.generation,
            first_source_frame: start,
            payload: DecoderPayload::Samples(pcm),
        });
        source.next_source_frame += frames as u64;
        Some(abi::OK)
    })
}

struct Decoder<'api, 'reader> {
    api: &'api Api,
    handle: NonNull<c_void>,
    source: NonNull<Source<'reader>>,
    total: u64,
    initialized: bool,
    sent_end: bool,
}

impl<'api, 'reader> Decoder<'api, 'reader> {
    fn open(
        api: &'api Api,
        mut input: Box<dyn PinnedReader + 'reader>,
        generation: Generation,
        authority: Cancellation,
    ) -> io::Result<Self> {
        let size = input.seek(SeekFrom::End(0))?;
        input.rewind()?;
        let mut magic = [0; 4];
        input.read_exact(&mut magic)?;
        input.rewind()?;
        if &magic != b"fLaC" || !(42..=MAX_COMPRESSED).contains(&size) {
            return Err(io::Error::other("unsupported pinned FLAC source"));
        }
        authority.check().map_err(io::Error::other)?;
        // SAFETY: the loaded SDK function allocates a new opaque decoder.
        let handle = NonNull::new(unsafe { (api.new)() })
            .ok_or_else(|| io::Error::other("cannot allocate FLAC decoder"))?;
        let source = NonNull::from(Box::leak(Box::new(Source {
            input,
            authority,
            size,
            offset: 0,
            scratch: Box::new([0; SCRATCH]),
            stats: Stats::default(),
            bytes_left: BYTE_BUDGET,
            seeks_left: 32,
            generation,
            next_source_frame: 0,
            pending: None,
            fault: None,
        })));
        let mut result = Self {
            api,
            handle,
            source,
            total: 0,
            initialized: false,
            sent_end: false,
        };
        // SAFETY: both decoder and Source are uniquely owned by result; all
        // callbacks match the loaded 1.5.0 SDK and stay valid until deletion.
        let status = unsafe {
            (api.init)(
                handle.as_ptr(),
                Some(read),
                Some(seek),
                Some(tell),
                Some(length),
                Some(eof),
                Some(write),
                None,
                Some(fault),
                source.as_ptr().cast(),
            )
        };
        if status != abi::OK {
            return Err(io::Error::other(format!(
                "FLAC initialization status {status}"
            )));
        }
        result.initialized = true;
        // SAFETY: initialized live decoder and Source with no outstanding borrows.
        let metadata = unsafe { (api.metadata)(handle.as_ptr()) };
        result.checked(metadata)?;
        // SAFETY: reading STREAMINFO value after processing metadata.
        result.total = unsafe { (api.total)(handle.as_ptr()) };
        if result.total == 0 {
            return Err(io::Error::other("FLAC needs a known frame count"));
        }
        Ok(result)
    }

    fn source(&self) -> &Source<'reader> {
        // SAFETY: owns the boxed Source, and libFLAC cannot call back here.
        unsafe { self.source.as_ref() }
    }

    fn source_mut(&mut self) -> &mut Source<'reader> {
        // SAFETY: unique borrow of the decoder with no running native call.
        unsafe { self.source.as_mut() }
    }

    fn checked(&mut self, ok: c_int) -> io::Result<()> {
        self.source().authority.check().map_err(io::Error::other)?;
        if ok == 0 {
            self.source_mut()
                .fault
                .get_or_insert("native FLAC operation failed");
        }
        if let Some(error) = self.source().fault {
            return Err(operation_error(error));
        }
        Ok(())
    }

    fn seek(&mut self, generation: Generation, sample: u64) -> io::Result<()> {
        if sample >= self.total || generation.0 <= self.source().generation.0 {
            return Err(io::Error::other("invalid FLAC logical seek"));
        }
        let source = self.source_mut();
        source.prepare_operation()?;
        source.pending.take();
        source.generation = generation;
        source.next_source_frame = sample;
        self.sent_end = false;
        // SAFETY: the live decoder can call its callbacks. A first partial
        // block may be delivered in the seek itself, so keep its pending PCM.
        let ok = unsafe { (self.api.seek)(self.handle.as_ptr(), sample) };
        self.checked(ok)
    }

    fn next(&mut self) -> io::Result<Option<DecodedPacket>> {
        self.source_mut().prepare_operation()?;
        if self.sent_end {
            return Ok(None);
        }
        for _ in 0..=16 {
            if let Some(packet) = self.source_mut().pending.take() {
                if self.source().next_source_frame > self.total {
                    self.source_mut()
                        .fail("decoded FLAC exceeds STREAMINFO frame count");
                    return Err(io::Error::other(
                        "decoded FLAC exceeds STREAMINFO frame count",
                    ));
                }
                return Ok(Some(packet));
            }
            // SAFETY: valid decoder retained while its public state is queried.
            if unsafe { (self.api.state)(self.handle.as_ptr()) } == abi::END_OF_STREAM {
                if self.source().next_source_frame != self.total {
                    self.source_mut()
                        .fail("decoded FLAC ended before STREAMINFO frame count");
                    return Err(io::Error::other(
                        "decoded FLAC ended before STREAMINFO frame count",
                    ));
                }
                self.sent_end = true;
                return Ok(Some(DecodedPacket {
                    generation: self.source().generation,
                    first_source_frame: self.total,
                    payload: DecoderPayload::End,
                }));
            }
            // SAFETY: initialized live decoder with no borrowed Source.
            let ok = unsafe { (self.api.process)(self.handle.as_ptr()) };
            self.checked(ok)?;
        }
        self.source_mut()
            .fail("native FLAC exceeded its step budget");
        Err(io::Error::other("native FLAC exceeded its step budget"))
    }
}

impl Drop for Decoder<'_, '_> {
    fn drop(&mut self) {
        // SAFETY: closes the native decoder before restoring and dropping its
        // single leaked Box. Any final callback still has a valid Source.
        unsafe {
            if self.initialized {
                (self.api.finish)(self.handle.as_ptr());
            }
            (self.api.delete)(self.handle.as_ptr());
            drop(Box::from_raw(self.source.as_ptr()));
        }
    }
}

#[cfg(test)]
fn source_pair(frame: u64) -> [i32; 2] {
    let word = (frame as u32)
        .wrapping_mul(0x9e3779b1)
        .rotate_left((frame % 29) as u32);
    let left = ((word >> 16) & 0x3fff) as i32 - 8192;
    [left, -left / 8]
}

#[cfg(test)]
fn expect_packet(packet: DecodedPacket, generation: Generation, start: u64) -> usize {
    assert_eq!(packet.generation, generation);
    assert_eq!(packet.first_source_frame, start);
    let DecoderPayload::Samples(pcm) = packet.payload else {
        panic!("expected FLAC audio frame");
    };
    for (offset, pair) in pcm.chunks_exact(CHANNELS).enumerate() {
        assert_eq!(
            pair,
            source_pair(start + offset as u64).map(|v| v as f32 / 32768.0)
        );
    }
    pcm.len() / CHANNELS
}

#[cfg(test)]
fn sdk_path() -> PathBuf {
    std::env::var_os("CUMULUS_FLAC_PROBE_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            #[cfg(target_os = "linux")]
            {
                PathBuf::from("/usr/lib/x86_64-linux-gnu/libFLAC.so.14")
            }
            #[cfg(not(target_os = "linux"))]
            panic!("set CUMULUS_FLAC_PROBE_LIBRARY to the trusted libFLAC DLL");
        })
}

#[test]
#[ignore = "requires a trusted native libFLAC 1.5.0; no ffmpeg executable or audio device"]
fn limiter_inprocess_flac_exact_random_seek_and_generation_eof() {
    let api = Api::open(&sdk_path()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("seek.flac");
    api.encode_fixture(&file, RATE, FLAC_BLOCK, TOTAL, source_pair)
        .unwrap();
    let file = std::fs::File::open(&file).unwrap();
    let bytes = file.metadata().unwrap().len();
    let cancel = Cancellation::default();
    let mut decoder = Decoder::open(&api, Box::new(file), Generation(1), cancel.clone()).unwrap();
    assert_eq!(decoder.total, TOTAL);
    assert_eq!(
        expect_packet(decoder.next().unwrap().unwrap(), Generation(1), 0),
        FLAC_BLOCK as usize
    );

    let mut summary = Vec::new();
    for (generation, position) in [(Generation(2), TOTAL - 8_023), (Generation(3), 12_409)] {
        let before = decoder.source().stats;
        decoder.seek(generation, position).unwrap();
        let trimmed = expect_packet(decoder.next().unwrap().unwrap(), generation, position);
        assert_eq!(
            trimmed as u64,
            u64::from(FLAC_BLOCK) - position % u64::from(FLAC_BLOCK)
        );
        let after = decoder.source().stats;
        assert!(after.seeks > before.seeks);
        assert!(after.bytes_read - before.bytes_read < bytes / 4);
        if generation == Generation(2) {
            assert!(after.last_seek > bytes / 2);
        }
        summary.push((
            position,
            trimmed,
            after.bytes_read - before.bytes_read,
            after.seeks - before.seeks,
        ));
    }

    // Reuse the existing generation-aware limiter, without starting any
    // child, converting the source into a predecoded file, or staging mpv.
    let start = TOTAL - 2301;
    let generation = Generation(4);
    assert!(decoder.seek(generation, TOTAL).is_err());
    assert!(decoder.seek(Generation(3), start).is_err());
    decoder.seek(generation, start).unwrap();
    let plan = crate::effects::material::material_limiter_plan(&serde_json::json!({})).unwrap();
    let mut limiter = LimiterSegment::new(&plan, RATE, CHANNELS as u32, 2048).unwrap();
    limiter.activate(generation, start, TOTAL).unwrap();
    let mut out = [0.0; 127 * CHANNELS];
    let mut cursor = start;
    for _ in 0..64 {
        match limiter
            .read(&mut out, || decoder.next().map_err(crate::Error::from))
            .unwrap()
        {
            SegmentRead::Audio {
                generation: active,
                first_source_frame,
                frames,
                source_clock,
            } => {
                assert_eq!(active, generation);
                assert_eq!(first_source_frame, cursor);
                for (offset, pair) in out[..frames as usize * CHANNELS]
                    .chunks_exact(CHANNELS)
                    .enumerate()
                {
                    assert_eq!(
                        pair,
                        source_pair(cursor + offset as u64).map(|v| v as f32 / 32768.0)
                    );
                }
                cursor += u64::from(frames);
                assert_eq!(source_clock, cursor);
            }
            SegmentRead::End {
                generation: active,
                source_clock,
            } => {
                assert_eq!(active, generation);
                assert_eq!(source_clock, TOTAL);
                assert_eq!(cursor, TOTAL);
                assert!(decoder.sent_end);
                cancel.cancel();
                assert!(decoder.next().is_err());
                eprintln!(
                    "libFLAC 1.5.0 native encode/decode; {bytes} compressed bytes, {TOTAL} frames; seeks (source, first-block frames, compressed bytes read, seeks) {summary:?}; {cursor_minus_start} exact PCM frames drained through the generation-tagged limiter without a child",
                    cursor_minus_start = cursor - start
                );
                return;
            }
            SegmentRead::Pending => continue,
            other => panic!("unexpected in-process FLAC limiter state: {other:?}"),
        }
    }
    panic!("in-process FLAC limiter did not finish");
}
