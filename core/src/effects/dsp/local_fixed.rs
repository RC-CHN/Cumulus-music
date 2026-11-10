//! Internal safe FFI for the fixed old-schema local scope before final B0.
use super::{
    AmbienceOneResult, Format, PlanarBlock,
    ambience_one::allocate_storage,
    ffi,
    state::{State, status},
};
use crate::{
    Error, Result,
    effects::{material::PeqCall, stereo::StereoSettings},
};

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(super) struct LocalModernBand {
    slot: u32,
    kind: u32,
    enabled: u32,
    gain: f32,
    frequency: f32,
    q: f32,
}

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
    pub fn new_equalizer(format: Format) -> Result<Self> {
        if !matches!(format.rate(), 44100.0 | 48000.0)
            || format.channels() != 2
            || format.capacity() != 2048
        {
            return Err(Error::InvalidInput("unsupported local equalizer format"));
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
                    format.rate() as u32,
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
        self.checked_control(code)
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
        self.checked_control(code)
    }
    pub fn apply_equalizer_material(
        &mut self,
        geq: Option<&[f32; 10]>,
        [bass, treble]: [i32; 2],
        [presence, stereoizer]: [i32; 2],
    ) -> Result<()> {
        self.available()?;
        let gains = geq.map_or(std::ptr::null(), |gains| gains.as_ptr());
        // SAFETY: exclusive initialized state/arena. Gains is null or points
        // to ten live floats for the duration of this synchronous call. C
        // validates the fixed geometry and every scalar before writing state.
        let code = unsafe {
            ffi::netease_local_equalizer_material(
                self.state.pointer(),
                gains,
                bass,
                treble,
                presence,
                stereoizer,
            )
        };
        self.checked_control(code)
    }
    pub fn equalizer_master_off(&mut self) -> Result<()> {
        self.available()?;
        // SAFETY: exclusive initialized state/arena; no PCM/output pointers.
        let code = unsafe { ffi::netease_local_equalizer_master_off(self.state.pointer()) };
        self.checked_control(code)
    }
    pub(crate) fn initial_modern_ready(&self) -> bool {
        self.available().is_ok()
            // SAFETY: this call only reads flags from the initialized object.
            && unsafe { ffi::netease_local_equalizer_initial_modern_ready(self.state.const_pointer()) == 1 }
    }
    pub(crate) fn apply_modern(
        &mut self,
        initial: bool,
        geq: &[f32; 10],
        [bass, treble]: [f32; 2],
        calls: &[PeqCall],
    ) -> Result<()> {
        self.available()?;
        let [
            PeqCall::Gain { value: gain },
            bands @ ..,
            PeqCall::On { value: true },
        ] = calls
        else {
            return Err(Error::InvalidInput("invalid initial modern PEQ program"));
        };
        if !(1..=9).contains(&bands.len()) {
            return Err(Error::InvalidInput("invalid initial modern PEQ program"));
        }
        let mut ffi_bands = [LocalModernBand::default(); 9];
        for (slot, call) in ffi_bands.iter_mut().zip(bands) {
            let PeqCall::Band {
                band,
                kind,
                gain,
                freq,
                q,
                on,
            } = *call
            else {
                return Err(Error::InvalidInput("invalid initial modern PEQ program"));
            };
            *slot = LocalModernBand {
                slot: band,
                kind,
                enabled: u32::from(on),
                gain,
                frequency: freq,
                q,
            };
        }
        // SAFETY: exclusive initialized fixed graph, fixed ten finite gains,
        // at most nine initialized repr(C) records. The C transaction mutates
        // only after the bounded local graph state and all parameters verify.
        let status = unsafe {
            ffi::netease_local_equalizer_apply_modern(
                self.state.pointer(),
                u32::from(initial),
                geq.as_ptr(),
                bass,
                treble,
                *gain,
                ffi_bands.as_ptr(),
                bands.len() as u32,
            )
        };
        Self::check_modern_control(status)
    }
    pub(crate) fn modern_master_off(&mut self) -> Result<()> {
        self.available()?;
        // SAFETY: exclusive initialized state; this checked C operation
        // only changes target bypass flags and commits atomically.
        let status =
            unsafe { ffi::netease_local_equalizer_modern_master_off(self.state.pointer()) };
        Self::check_modern_control(status)
    }
    pub(crate) fn return_legacy_geq(&mut self, geq: &[f32; 10]) -> Result<()> {
        self.available()?;
        // SAFETY: the initialized graph is exclusive and the C setter only
        // reads the ten input gains during its bounded transaction.
        let status =
            unsafe { ffi::netease_local_equalizer_return_geq(self.state.pointer(), geq.as_ptr()) };
        Self::check_modern_control(status)
    }
    fn check_modern_control(status: i32) -> Result<()> {
        match status {
            0 => Ok(()),
            1 => Err(Error::State(
                "modern effect needs a compatible local graph state",
            )),
            _ => Err(Error::InvalidInput("invalid modern local graph controls")),
        }
    }
    fn checked_control(&mut self, code: i32) -> Result<()> {
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
