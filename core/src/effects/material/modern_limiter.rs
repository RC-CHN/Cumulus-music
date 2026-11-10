//! A bounded, offline graph for one recovered modern material topology.
//! Neither decoding JSON nor constructing this graph authorizes a source.
use super::{
    Component, EqualizationCall, MATERIAL_BEGIN_LIMITER, MaterialLimiterPlan, PeqCall, apply_peq,
    bass_treble_calls, graphic_calls, material_limiter_plan, number, peq_calls,
};
use crate::{
    Error, Result,
    effects::dsp::{
        Coefficients, Format, GraphicEq, LimiterBlockFlow, LimiterStage, LimiterStream,
        LimiterStreamQuantum, LimiterStreamRead, LimiterStreamWrite, LocalFixedStage,
        ParametricBand, ParametricEq, ParametricKind, PlanarBlock, Slot50Tone,
    },
};
use serde_json::{Map, Value};

const COMPONENTS: [(Component, bool); 7] = [
    (Component::Eq, true),
    (Component::Bt, true),
    (Component::Peq, true),
    (Component::Limiter, true),
    (Component::Se, false),
    (Component::Rvb, false),
    (Component::Cmp, false),
];
const UNSUPPORTED: Error = Error::InvalidInput("unsupported modern limiter material");

/// Validated parameter program for GEQ → modern BT → PEQ → explicit B0.
/// This admits no legacy fallback, extra siblings or material authorization.
/// Constructing it does not open files, play audio or use IO.
#[derive(Clone, Debug)]
pub struct ModernLimiterPlan {
    geq: [f32; 10],
    bass: f32,
    treble: f32,
    peq: Vec<PeqCall>,
    limiter: MaterialLimiterPlan,
}

impl ModernLimiterPlan {
    /// Require the exact supported aev2 topology, explicit typed controls
    /// and bounded values. Top-level legacy controls have no effect in aev2.
    pub fn from_material(material: &Value) -> Result<Self> {
        let modern = material
            .get("aev2")
            .and_then(Value::as_object)
            .ok_or(UNSUPPORTED)?;
        if modern.len() != COMPONENTS.len() {
            return Err(UNSUPPORTED);
        }
        for (component, on) in COMPONENTS {
            if modern
                .get(component.key())
                .and_then(|section| section.get("on"))
                .and_then(Value::as_bool)
                != Some(on)
            {
                return Err(UNSUPPORTED);
            }
        }

        let geq = &modern[Component::Eq.key()];
        let gains = geq
            .get("eqs")
            .and_then(Value::as_array)
            .filter(|values| values.len() == 10)
            .ok_or(UNSUPPORTED)?;
        for value in gains {
            checked_gain(number(Some(value))?, 12.0)?;
        }
        let geq = graphic_calls(geq)?;
        let [
            EqualizationCall::EqGains { gains: geq },
            EqualizationCall::EqOn { value: true },
        ] = geq.as_slice()
        else {
            return Err(UNSUPPORTED);
        };
        let geq = *geq;

        let bt = &modern[Component::Bt.key()];
        checked_gain(number(bt.get("bass"))?, 24.0)?;
        checked_gain(number(bt.get("treble"))?, 24.0)?;
        let bt = bass_treble_calls(bt)?;
        let [
            EqualizationCall::BtGains { bass, treble },
            EqualizationCall::BtOn { value: true },
        ] = bt.as_slice()
        else {
            return Err(UNSUPPORTED);
        };
        let (bass, treble) = (*bass, *treble);

        let peq = &modern[Component::Peq.key()];
        require_peq_controls(peq)?;
        let peq = peq_calls(peq)?;

        let limiter = &modern[Component::Limiter.key()];
        checked_gain(number(limiter.get("threshold"))?, 24.0)?;
        checked_gain(number(limiter.get("outputgain"))?, 24.0)?;
        // This profile admits an explicit infinite-ratio setting and a
        // short lookahead. The independent B0 API has wider leaf coverage.
        if limiter.get("infiniteRatio").and_then(Value::as_bool) != Some(true)
            || !number(limiter.get("attack"))?.is_some_and(|value| (0.001..=0.010).contains(&value))
            || !number(limiter.get("release"))?.is_some_and(|value| (0.001..=1.0).contains(&value))
            || !number(limiter.get("ratio"))?.is_some_and(|value| (0.125..=100.0).contains(&value))
        {
            return Err(UNSUPPORTED);
        }
        Ok(Self {
            geq,
            bass,
            treble,
            peq,
            limiter: material_limiter_plan(material)?,
        })
    }

    /// Start a fresh stereo f32 renderer with the official 2048-frame
    /// quantum, separate bounded FIFOs and no attachment to a media sink.
    pub fn pcm_renderer(
        &self,
        format: Format,
        fifo_capacity: u32,
    ) -> Result<ModernLimiterRenderer> {
        Ok(ModernLimiterRenderer {
            stages: self.stages(format)?,
            stream: self.limiter.pcm_stream(format, fifo_capacity)?,
            plan: self.clone(),
            material_active: true,
            quarantined: false,
        })
    }

    // For a transport that already owns the native input quantum and EOF
    // sentinel. The standalone API must not be wrapped in a second FIFO.
    #[cfg(test)]
    pub(crate) fn native_test_blocks(
        &self,
        format: Format,
    ) -> Result<(ModernLimiterStages, LimiterStage)> {
        Ok((self.stages(format)?, self.limiter.pcm_stage(format)?))
    }

    pub(in crate::effects) fn supports_format(&self, format: Format) -> bool {
        matches!(format.rate(), 44_100.0 | 48_000.0)
            && format.channels() == 2
            && format.capacity() == 2048
    }

    pub(crate) fn prepare_control(
        &self,
        format: Format,
        active: bool,
    ) -> Result<Option<LimiterStage>> {
        let material = self.limiter.parameters;
        let defaults = MATERIAL_BEGIN_LIMITER;
        if material == defaults {
            return Ok(None);
        }
        if material.attack == defaults.attack && material.release == defaults.release {
            return Err(Error::InvalidInput(
                "unsupported live limiter parameter change without timing change",
            ));
        }
        // Native B0 setter +C5 and the next active 64F40 setup reset node
        // PCM and restart skip/drain even when geometry is unchanged.
        // Prepare the replacement without modifying any current state.
        let params = if active { material } else { defaults };
        MaterialLimiterPlan::stage_with_parameters(format, true, params).map(Some)
    }

    /// Initial whole-material apply on the same settled old-schema GEQ-only
    /// graph. Prepare B0 before changing C state; no mutation on rejection.
    pub(crate) fn install_on_settled_local(
        &self,
        stage: &mut LocalFixedStage,
        format: Format,
    ) -> Result<Option<LimiterStage>> {
        if !self.supports_format(format) {
            return Err(Error::InvalidInput(
                "unsupported local-to-modern graph format",
            ));
        }
        let b0 = self.prepare_control(format, true)?;
        stage.apply_modern(true, &self.geq, [self.bass, self.treble], &self.peq)?;
        Ok(b0)
    }

    /// Master-off or reapply on the same already-converted local graph.
    /// Keep the previous wrapper flags even when both controls precede PCM.
    pub(crate) fn set_active_on_shared_local(
        &self,
        stage: &mut LocalFixedStage,
        format: Format,
        active: bool,
    ) -> Result<Option<LimiterStage>> {
        let b0 = self.prepare_control(format, active)?;
        if active {
            stage.apply_modern(false, &self.geq, [self.bass, self.treble], &self.peq)?;
        } else {
            stage.modern_master_off()?;
        }
        Ok(b0)
    }

    fn stages(&self, format: Format) -> Result<ModernLimiterStages> {
        if !self.supports_format(format) {
            return Err(Error::InvalidInput(
                "unsupported modern limiter graph format",
            ));
        }

        // Establish each previous bypass before applying the material.
        // The first graph quantum must actually run each dry-to-wet edge.
        let mut geq = GraphicEq::new(format)?;
        geq.set_bypass(true)?;
        geq.reset()?;
        geq.set_gains(&self.geq)?;
        geq.set_bypass(false)?;

        let mut bt = Slot50Tone::new(format)?;
        bt.apply_enabled_modern(self.bass, self.treble)?;

        let mut peq = ParametricEq::new(format, false, true)?;
        begin_peq_defaults(&mut peq)?;
        peq.reset()?;
        apply_peq(&mut peq, &self.peq)?;

        Ok(ModernLimiterStages { geq, bt, peq })
    }
}

fn begin_peq_defaults(peq: &mut ParametricEq) -> Result<()> {
    peq.set_bypass(true)?;
    peq.set_gain(0.0)?;
    for index in 0..9 {
        let (kind, frequency) = match index {
            0 => (ParametricKind::LowShelfQ, 100.0),
            1 => (ParametricKind::Peak, 1_000.0),
            2 => (ParametricKind::HighShelfQ, 10_000.0),
            _ => (ParametricKind::HighShelfQ, 18_000.0),
        };
        peq.set_band(
            index,
            ParametricBand {
                kind,
                gain_db: 0.0,
                frequency,
                q: 1.0,
                enabled: false,
            },
        )?;
    }
    Ok(())
}

fn checked_gain(value: Option<f32>, limit: f32) -> Result<f32> {
    value
        .filter(|value| (-limit..=limit).contains(value))
        .ok_or(UNSUPPORTED)
}

fn require_peq_controls(peq: &Value) -> Result<()> {
    checked_gain(number(peq.get("gain"))?, 24.0)?;
    let filters = peq
        .get("f")
        .and_then(Value::as_array)
        .filter(|filters| (1..=9).contains(&filters.len()))
        .ok_or(UNSUPPORTED)?;
    let mut seen = [false; 9];
    let mut active = false;
    for filter in filters {
        let filter: &Map<String, Value> = filter.as_object().ok_or(UNSUPPORTED)?;
        let index = filter
            .get("band")
            .and_then(Value::as_u64)
            .filter(|index| *index < 9)
            .ok_or(UNSUPPORTED)? as usize;
        if std::mem::replace(&mut seen[index], true) {
            return Err(UNSUPPORTED);
        }
        let kind = filter
            .get("type")
            .and_then(Value::as_u64)
            .filter(|kind| *kind <= 10)
            .ok_or(UNSUPPORTED)?;
        active |= filter
            .get("on")
            .and_then(Value::as_bool)
            .ok_or(UNSUPPORTED)?;
        let gain = checked_gain(number(filter.get("gain"))?, 24.0)?;
        // Strictly below Nyquist at both admitted sample rates. C separately
        // validates each coefficient set before we return the typed plan.
        let freq = number(filter.get("freq"))?
            .filter(|freq| *freq > 0.0 && *freq < 22_050.0)
            .ok_or(UNSUPPORTED)?;
        let q = number(filter.get("q"))?
            .filter(|q| (0.0..=100.0).contains(q) && (*q > 0.0 || matches!(kind, 0 | 1 | 4 | 5)))
            .ok_or(UNSUPPORTED)?;
        let kind = ParametricKind::try_from(kind as u32)?;
        for rate in [44_100.0, 48_000.0] {
            Coefficients::parametric(kind, rate, freq, gain, q)?;
        }
    }
    if !active {
        return Err(UNSUPPORTED);
    }
    Ok(())
}

/// Owned offline PCM stream for the qualified topology. Holds only DSP state
/// and bounded PCM. It does not hold files, identities, URLs, tokens or leases.
pub struct ModernLimiterRenderer {
    stages: ModernLimiterStages,
    stream: LimiterStream,
    plan: ModernLimiterPlan,
    material_active: bool,
    quarantined: bool,
}

pub(crate) struct ModernLimiterStages {
    geq: GraphicEq,
    bt: Slot50Tone,
    peq: ParametricEq,
}

impl ModernLimiterStages {
    pub(crate) fn apply_control(&mut self, plan: &ModernLimiterPlan, active: bool) -> Result<()> {
        if active {
            self.reapply_material(plan)
        } else {
            self.disable_master()
        }
    }

    fn disable_master(&mut self) -> Result<()> {
        self.geq.set_bypass(true)?;
        self.bt.master_bypass()?;
        self.peq.set_bypass(true)
    }

    fn reapply_material(&mut self, plan: &ModernLimiterPlan) -> Result<()> {
        // EFB760 -> 68000/1BBA0 first sends the default component settings.
        // None of those setters settles the previous wrapper state or
        // advances audio; the next graph quantum still owns the bypass mix.
        self.geq.set_bypass(true)?;
        self.geq.set_gains(&[0.0; 10])?;
        self.bt.apply_material_begin_defaults()?;
        begin_peq_defaults(&mut self.peq)?;
        self.geq.set_gains(&plan.geq)?;
        self.geq.set_bypass(false)?;
        self.bt.apply_enabled_modern(plan.bass, plan.treble)?;
        apply_peq(&mut self.peq, &plan.peq)
    }

    pub(crate) fn reset(&mut self) -> Result<()> {
        self.geq.reset()?;
        self.bt.reset()?;
        self.peq.reset()
    }

    pub(crate) fn process(
        &mut self,
        limiter: &mut LimiterStage,
        block: &mut PlanarBlock,
        frames: u32,
        end: bool,
    ) -> Result<LimiterBlockFlow> {
        let valid = frames as usize;
        let stride = block.format().capacity();
        if valid > stride {
            return Err(Error::InvalidInput("modern limiter block exceeds capacity"));
        }
        for channel in block.samples_mut().chunks_exact_mut(stride) {
            channel[valid..].fill(0.0);
        }
        self.geq.process(block, valid)?;
        self.bt.process(block, valid)?;
        self.peq.process(block, valid)?;
        let timing = limiter.process(block, frames, end)?;
        let valid = timing.output_frames as usize;
        if block
            .samples()
            .chunks_exact(stride)
            .any(|channel| channel[..valid].iter().any(|sample| !sample.is_finite()))
        {
            return Err(Error::Media("non-finite modern limiter graph output"));
        }
        Ok(timing)
    }
}

impl ModernLimiterRenderer {
    pub fn lookahead_frames(&self) -> u32 {
        self.stream.lookahead_frames()
    }

    /// Change the current graph between the material installed at creation
    /// and the official master-off state. Off fades GEQ/BT/PEQ to dry while
    /// B0 remains enabled at native defaults. A second call with the same
    /// target is a no-op; this API does not hot-swap other material.
    ///
    /// The pending input/output FIFOs and clock are retained. Changing B0
    /// timing discards only that node's old lookahead and restarts its
    /// output debt; an identical default B0 keeps its existing PCM history.
    /// Live gain/ratio-only B0 changes are outside this verified scope.
    pub fn set_material_active(&mut self, active: bool) -> Result<()> {
        self.ensure_live()?;
        let format = self.stream.control_format()?;
        if self.material_active == active {
            return Ok(());
        }
        let replacement = self.plan.prepare_control(format, active)?;
        if let Err(error) = self.stages.apply_control(&self.plan, active) {
            self.quarantined = true;
            return Err(error);
        }
        if let Some(stage) = replacement {
            self.stream.replace_stage_for_setup(stage);
        }
        self.material_active = active;
        Ok(())
    }

    /// Empty input submits EOF. The source FIFO inserts the native sentinel
    /// before the pre-limiter graph, and B0 adds its own tail padding later.
    pub fn push_interleaved(
        &mut self,
        samples: &[f32],
        timestamp_ms: Option<u32>,
    ) -> Result<LimiterStreamWrite> {
        self.push_interleaved_observed(samples, timestamp_ms, |_| {})
    }

    /// Invoke the observer for each completed graph quantum.
    pub fn push_interleaved_observed(
        &mut self,
        samples: &[f32],
        timestamp_ms: Option<u32>,
        observe: impl FnMut(LimiterStreamQuantum),
    ) -> Result<LimiterStreamWrite> {
        self.ensure_live()?;
        let mut processing = false;
        let result = self.stream.push_interleaved_graph(
            samples,
            timestamp_ms,
            |limiter, block, frames, end| {
                processing = true;
                self.stages.process(limiter, block, frames, end)
            },
            observe,
        );
        // Admission failures never enter the closure; those can be retried.
        // After a kernel error, no buffered PCM may escape through a later read.
        if processing && result.is_err() {
            self.quarantined = true;
        }
        result
    }

    pub fn pull_interleaved(&mut self, samples: &mut [f32]) -> Result<LimiterStreamRead> {
        self.ensure_live()?;
        self.stream.pull_interleaved(samples)
    }

    /// Discard both FIFOs and clear clock, EOF and DSP histories. Preserve
    /// controls and settle the wrappers to their current active targets.
    pub fn reset(&mut self) -> Result<()> {
        self.ensure_live()?;
        if let Err(error) = self.stages.reset() {
            self.quarantined = true;
            return Err(error);
        }
        self.stream.reset();
        Ok(())
    }

    fn ensure_live(&self) -> Result<()> {
        if self.quarantined {
            Err(Error::State(
                "modern limiter graph failed; recreate the renderer",
            ))
        } else {
            Ok(())
        }
    }
}
