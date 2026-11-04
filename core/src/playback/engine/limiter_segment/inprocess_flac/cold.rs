//! Native opt-in bounded render. The host retains the original authorized
//! compressed reader independently of the currently published PCM buffer.
use super::{
    super::{LimiterSegment, SegmentRead},
    Api, CHANNELS, Decoder, FLAC_BLOCK, Generation, MAX_COMPRESSED, RATE, ReadBudget,
};
use crate::{
    Error, Result,
    download::offline::{ColdOfflineFlacAuthority, ColdOfflineFlacReader},
    effects::material::MaterialLimiterPlan,
    playback::engine::Cancellation,
};
use std::{
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

const QUANTUM: u32 = 2048;
const HEADER: usize = 44;
const FRAME_BYTES: usize = CHANNELS * size_of::<f32>();
const MAX_FRAMES: u64 = 30 * RATE as u64;
const RENDER_WAIT: Duration = Duration::from_secs(2);

pub(crate) struct ColdLimiterConfig {
    library: PathBuf,
    plan: Arc<MaterialLimiterPlan>,
}

impl ColdLimiterConfig {
    pub(crate) fn new(library: PathBuf, plan: MaterialLimiterPlan) -> Result<Self> {
        if !library.is_absolute() {
            return Err(Error::InvalidInput(
                "cold limiter requires an absolute native library path",
            ));
        }
        if plan.enabled {
            LimiterSegment::new(&plan, RATE, CHANNELS as u32, QUANTUM)?;
        }
        Ok(Self {
            library,
            plan: Arc::new(plan),
        })
    }

    pub(crate) fn available(&self) -> Option<ColdLimiterRuntime> {
        if !self.plan.enabled {
            return None;
        }
        Some(ColdLimiterRuntime {
            api: Api::open(&self.library).ok()?,
            plan: self.plan.clone(),
        })
    }
}

pub(crate) struct ColdLimiterRuntime {
    api: Api,
    plan: Arc<MaterialLimiterPlan>,
}

pub(crate) struct ColdLimiterSource {
    reader: ColdOfflineFlacReader,
    api: Api,
    plan: Arc<MaterialLimiterPlan>,
    total: u64,
}

/// Opaque PCM authorized by the same grant and focus lease as its source.
pub struct ColdLimiterWave {
    pub(crate) data: Vec<u8>,
    pub(crate) authority: ColdOfflineFlacAuthority,
}

pub(crate) enum ColdLimiterRender {
    Pcm(ColdLimiterWave),
    Stock,
}

impl ColdLimiterRuntime {
    pub(crate) fn select(
        self,
        mut reader: ColdOfflineFlacReader,
    ) -> Result<std::result::Result<ColdLimiterSource, ColdOfflineFlacReader>> {
        let authority = reader.authority();
        authority.validate()?;
        let length = reader.seek(SeekFrom::End(0))?;
        reader.rewind()?;
        if !(42..=MAX_COMPRESSED).contains(&length) {
            return Ok(Err(reader));
        }
        // STREAMINFO is the first 34-byte FLAC metadata payload. Use its
        // format/size before entering the optional decoder or allocating PCM.
        let mut header = [0; 42];
        reader.read_exact(&mut header)?;
        if &header[..4] != b"fLaC" || header[4] & 127 != 0 || header[5..8] != [0, 0, 34] {
            return Err(Error::Media("invalid FLAC STREAMINFO"));
        }
        let min_block = u16::from_be_bytes([header[8], header[9]]);
        let max_block = u16::from_be_bytes([header[10], header[11]]);
        if min_block < 16 || min_block > max_block {
            return Err(Error::Media("invalid FLAC STREAMINFO block bounds"));
        }
        let format = u64::from_be_bytes(header[18..26].try_into().expect("STREAMINFO size"));
        let rate = format >> 44;
        let channels = ((format >> 41) & 7) + 1;
        let bits = ((format >> 36) & 31) + 1;
        let total = format & ((1 << 36) - 1);
        authority.validate()?;
        if !(1..=MAX_FRAMES).contains(&total)
            || u32::from(max_block) > FLAC_BLOCK
            || rate != u64::from(RATE)
            || channels != CHANNELS as u64
            || bits != 16
        {
            return Ok(Err(reader));
        }
        Ok(Ok(ColdLimiterSource {
            reader,
            api: self.api,
            plan: self.plan,
            total,
        }))
    }
}

#[derive(Debug)]
struct ElapsedBudget;
impl std::fmt::Display for ElapsedBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cold FLAC limiter render deadline elapsed")
    }
}
impl std::error::Error for ElapsedBudget {}

impl ColdLimiterSource {
    pub(crate) fn render(
        &mut self,
        first: u64,
        cancel: &Cancellation,
    ) -> Result<ColdLimiterRender> {
        let authority = self.reader.authority();
        authority.validate()?;
        cancel.check()?;
        let result = self.decode(first, cancel, Instant::now() + RENDER_WAIT);
        authority.validate()?;
        cancel.check()?;
        match result {
            Ok(data) => Ok(ColdLimiterRender::Pcm(ColdLimiterWave { data, authority })),
            Err(Error::Io(error))
                if error.get_ref().is_some_and(|cause| {
                    cause.is::<ReadBudget>() || cause.is::<ElapsedBudget>()
                }) =>
            {
                Ok(ColdLimiterRender::Stock)
            }
            Err(error) => Err(error),
        }
    }

    fn decode(&mut self, first: u64, cancel: &Cancellation, deadline: Instant) -> Result<Vec<u8>> {
        if first >= self.total {
            return Err(Error::InvalidInput("cold FLAC seek is outside the track"));
        }
        let source = self.reader.authority().cancellation();
        let request = cancel.clone();
        let guard = Cancellation::new(move || !source.is_cancelled() && !request.is_cancelled());
        let mut decoder = Decoder::open(
            &self.api,
            Box::new(&mut self.reader),
            Generation(1),
            guard.clone(),
        )?;
        if decoder.total != self.total {
            return Err(Error::Media(
                "FLAC frame count changed after eligibility check",
            ));
        }
        let generation = if first == 0 {
            Generation(1)
        } else {
            let generation = Generation(2);
            decoder.seek(generation, first)?;
            generation
        };
        let mut segment = LimiterSegment::new(&self.plan, RATE, CHANNELS as u32, QUANTUM)?;
        segment.activate(generation, first, self.total)?;
        let logical_frames = self.total - first;
        let wav_frames =
            logical_frames + u64::from(logical_frames.is_multiple_of(u64::from(QUANTUM)));
        let payload = wav_frames as usize * FRAME_BYTES;
        let mut data = Vec::new();
        data.try_reserve_exact(HEADER + payload)
            .map_err(|_| Error::State("cold limiter PCM allocation failed"))?;
        data.extend_from_slice(&wave_header(payload as u32));
        let mut output = [0.0; QUANTUM as usize * CHANNELS];
        let check = || -> Result<()> {
            guard.check()?;
            if Instant::now() >= deadline {
                return Err(io::Error::other(ElapsedBudget).into());
            }
            Ok(())
        };
        loop {
            check()?;
            match segment.read(&mut output, || {
                check()?;
                decoder.next().map_err(Into::into)
            })? {
                SegmentRead::Pending => {}
                SegmentRead::Audio { frames, .. } => {
                    if data.len() + frames as usize * FRAME_BYTES > HEADER + payload {
                        return Err(Error::Media("limiter rendered excess PCM"));
                    }
                    for sample in &output[..frames as usize * CHANNELS] {
                        data.extend_from_slice(&sample.to_le_bytes());
                    }
                }
                SegmentRead::End { source_clock, .. } => {
                    if source_clock != self.total || data.len() != HEADER + payload {
                        return Err(Error::Media("limiter PCM did not reach authorized EOF"));
                    }
                    return Ok(data);
                }
                #[cfg(test)]
                SegmentRead::Paused => unreachable!("pre-render has no output pause"),
            }
        }
    }

    pub(crate) fn into_reader(self) -> ColdOfflineFlacReader {
        self.reader
    }

    pub(crate) fn seconds(frame: u64) -> f64 {
        frame as f64 / f64::from(RATE)
    }

    pub(crate) fn duration(&self) -> f64 {
        Self::seconds(self.total)
    }

    pub(crate) fn frame(&self, seconds: f64) -> Result<u64> {
        if !seconds.is_finite() || seconds < 0.0 || seconds >= self.duration() {
            return Err(Error::InvalidInput("cold FLAC seek is outside the track"));
        }
        Ok(((seconds * f64::from(RATE)).round() as u64).min(self.total - 1))
    }
}

fn wave_header(payload: u32) -> [u8; HEADER] {
    let mut out = [0; HEADER];
    out[..4].copy_from_slice(b"RIFF");
    out[4..8].copy_from_slice(&(36 + payload).to_le_bytes());
    out[8..16].copy_from_slice(b"WAVEfmt ");
    out[16..20].copy_from_slice(&16_u32.to_le_bytes());
    out[20..22].copy_from_slice(&3_u16.to_le_bytes());
    out[22..24].copy_from_slice(&(CHANNELS as u16).to_le_bytes());
    out[24..28].copy_from_slice(&RATE.to_le_bytes());
    out[28..32].copy_from_slice(&(RATE * FRAME_BYTES as u32).to_le_bytes());
    out[32..34].copy_from_slice(&(FRAME_BYTES as u16).to_le_bytes());
    out[34..36].copy_from_slice(&32_u16.to_le_bytes());
    out[36..40].copy_from_slice(b"data");
    out[40..44].copy_from_slice(&payload.to_le_bytes());
    out
}
