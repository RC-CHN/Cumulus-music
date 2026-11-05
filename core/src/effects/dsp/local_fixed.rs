//! Internal safe FFI for the fixed old-schema local scope before final B0.
use super::{
    AmbienceOneResult, Format, PlanarBlock,
    ambience_one::allocate_storage,
    ffi,
    state::{State, status},
};
use crate::{Error, Result, effects::stereo::StereoSettings};

pub(crate) struct LocalFixedStage {
    state: State,
    format: Format,
    _storage: Box<[f32]>,
    dry: PlanarBlock,
    failed: bool,
}
impl LocalFixedStage {
    pub fn new(format: Format, controls: StereoSettings) -> Result<Self> {
        if format.channels() != 2
            || !matches!(format.rate(), 44100.0 | 48000.0 | 96000.0)
            || !matches!(format.capacity(), 512 | 2048)
        {
            return Err(Error::InvalidInput("unsupported fixed local stereo format"));
        }
        let mut value = Self::allocate(format, false)?;
        value.configure(controls)?;
        Ok(value)
    }
    pub fn new_master(format: Format) -> Result<Self> {
        if format.rate() != 44100.0 || format.channels() != 2 || format.capacity() != 2048 {
            return Err(Error::InvalidInput("unsupported fixed local master format"));
        }
        Self::allocate(format, true)
    }
    fn allocate(format: Format, master: bool) -> Result<Self> {
        let mut storage = allocate_storage(format.rate() as u32)?;
        let mut state = State::new(10)?;
        // SAFETY: both constructors use the same queried aligned C object;
        // the validated format selects the exact independently owned arena.
        let code = unsafe {
            if master {
                ffi::cumulus_local_fixed_master_init(
                    state.pointer(),
                    storage.as_mut_ptr(),
                    storage.len(),
                )
            } else {
                ffi::cumulus_local_fixed_init(
                    state.pointer(),
                    format.rate() as u32,
                    format.capacity() as u32,
                    storage.as_mut_ptr(),
                    storage.len(),
                )
            }
        };
        status(code)?;
        Ok(Self {
            state,
            format,
            _storage: storage,
            dry: PlanarBlock::new(format),
            failed: false,
        })
    }
    pub fn master_off(&mut self) -> Result<()> {
        self.available()?;
        // SAFETY: exclusive initialized object/arena; no PCM or output pointers.
        let code = unsafe { ffi::netease_local_fixed_master_off(self.state.pointer()) };
        self.checked_master_control(code)
    }
    pub fn apply_master_material(&mut self, value: StereoSettings) -> Result<()> {
        self.available()?;
        let controls = value.controls();
        controls.validate()?;
        if !value.enabled() || !controls.spectrum_shaper {
            return Err(Error::InvalidInput(
                "fixed master scope requires enabled SE1",
            ));
        }
        // SAFETY: exclusive initialized object/arena and bounded finite values.
        let code = unsafe {
            ffi::netease_local_fixed_master_material(
                self.state.pointer(),
                controls.presence,
                controls.stereoizer,
            )
        };
        self.checked_master_control(code)
    }
    fn checked_master_control(&mut self, code: i32) -> Result<()> {
        // A native failure may have left a partially applied material. Never
        // continue through that state, even though validated inputs are bounded.
        self.failed = code != 0;
        status(code)
    }
    fn available(&self) -> Result<()> {
        if self.failed {
            Err(Error::State("fixed local stereo stage failed"))
        } else {
            Ok(())
        }
    }
    pub fn configure(&mut self, value: StereoSettings) -> Result<()> {
        self.available()?;
        let controls = value.controls();
        controls.validate()?;
        if !controls.spectrum_shaper {
            return Err(Error::InvalidInput(
                "fixed local scope requires spectrum shaper",
            ));
        }
        // SAFETY: exclusive initialized state; bounded validated floats/boolean.
        // The original on=false branch preserves bank and controls in C.
        status(unsafe {
            ffi::netease_local_fixed_parameters(
                self.state.pointer(),
                controls.presence,
                controls.stereoizer,
                value.enabled().into(),
            )
        })
    }
    pub fn reset(&mut self) -> Result<()> {
        self.available()?;
        // SAFETY: same owned live object and arena. No PCM, fake settle block,
        // allocation or callbacks are involved in reset.
        status(unsafe { ffi::netease_local_fixed_reset(self.state.pointer()) })
    }
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        self.available()?;
        if block.format() != self.format
            || frames > self.format.capacity()
            || block.samples().iter().any(|v| !v.is_finite())
        {
            return Err(Error::InvalidInput("invalid fixed local stereo PCM"));
        }
        let mut valid = frames as u32;
        let mut finished = u32::from(end);
        // SAFETY: exclusive matching state, full initialized capacity for both
        // channels even when valid=0, separate full dry buffer. No overlap or
        // external ownership; valid/end are bounded in/out scalar references.
        let code = unsafe {
            ffi::netease_local_fixed_process(
                self.state.pointer(),
                block.samples_mut().as_mut_ptr(),
                &mut valid,
                &mut finished,
                self.dry.samples_mut().as_mut_ptr(),
            )
        };
        if code != 0
            || valid as usize > self.format.capacity()
            || finished > 1
            || block.samples().iter().any(|v| !v.is_finite())
        {
            block.samples_mut().fill(0.0);
            self.failed = true;
            return Err(Error::Media("fixed local stereo processing failed"));
        }
        Ok(AmbienceOneResult {
            valid_frames: valid as usize,
            end: finished != 0,
        })
    }
}

#[cfg(all(
    test,
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]
mod tests {
    use super::ffi;
    use serde::Deserialize;
    use std::io::Read;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Vector {
        phase_bits: u32,
        sine_bits: u32,
        cosine_bits: u32,
    }
    #[derive(Deserialize)]
    struct Corpus {
        vectors: Vec<Vector>,
    }
    #[test]
    fn original_static_sse_peak_trigonometry_including_reduction_boundaries() {
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../../tests/fixtures/effects-geq-static-sse-trig-official.json.gz")
                [..],
        )
        .take(2 * 1024 * 1024)
        .read_to_end(&mut raw)
        .unwrap();
        let corpus: Corpus = serde_json::from_slice(&raw).unwrap();
        assert_eq!(corpus.vectors.len(), 10135);
        for vector in corpus.vectors {
            let mut values = [0.0f32; 2];
            // SAFETY: bounded finite witness phase and exactly two output floats.
            assert_eq!(
                unsafe {
                    ffi::cumulus_geq_peak_trig(
                        f32::from_bits(vector.phase_bits),
                        values.as_mut_ptr(),
                    )
                },
                0
            );
            assert_eq!(
                values.map(f32::to_bits),
                [vector.sine_bits, vector.cosine_bits],
                "phase bits {}",
                vector.phase_bits
            );
        }
    }
}
