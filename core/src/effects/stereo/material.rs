//! Strict free local equalizer subset and its source-backed SE1 -> default-B0
//! subgraph. This is not an entitlement or a complete SDK/material host.
use super::StereoSettings;
use crate::{
    Error, Result,
    effects::{
        dsp::{
            AmbienceOneResult, Format, LimiterSettings, LimiterStage, PlanarBlock,
            StereoAmbienceOneStage,
        },
        material::MATERIAL_BEGIN_LIMITER,
    },
};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalStereoMaterialPlan {
    stereo: StereoSettings,
}
impl LocalStereoMaterialPlan {
    /// Only JSON emitted by the verified local equalizer lane with all other
    /// top-level components flat/off is admitted. This parser performs no I/O.
    /// Caller must still establish native local origin, isOpen and source scope.
    pub fn from_equalizer_json(value: &Value) -> Result<Self> {
        let invalid = || Error::InvalidInput("unsupported local stereo equalizer subset");
        let root = value.as_object().ok_or_else(invalid)?;
        if root.len() != 4
            || root
                .keys()
                .any(|key| !matches!(key.as_str(), "eq" | "bt" | "rvb" | "se"))
        {
            return Err(invalid());
        }
        let eq = root["eq"].as_object().ok_or_else(invalid)?;
        let bt = root["bt"].as_object().ok_or_else(invalid)?;
        let rvb = root["rvb"].as_object().ok_or_else(invalid)?;
        let se = root["se"].as_object().ok_or_else(invalid)?;
        let gains = eq
            .get("eqs")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        if eq.get("on").and_then(Value::as_bool) != Some(false)
            || gains.len() != 10
            || gains.iter().any(|x| x.as_f64() != Some(0.0))
            || bt.get("on").and_then(Value::as_bool) != Some(false)
            || bt.get("bass").and_then(Value::as_f64) != Some(0.0)
            || bt.get("treble").and_then(Value::as_f64) != Some(0.0)
            || rvb.get("on").and_then(Value::as_bool) != Some(false)
            || se.get("ambience").and_then(Value::as_i64) != Some(1)
            || se.get("sshaper").and_then(Value::as_bool) != Some(true)
        {
            return Err(invalid());
        }
        let presence = se
            .get("presence")
            .and_then(Value::as_i64)
            .filter(|x| (-10..=10).contains(x))
            .ok_or_else(invalid)?;
        let stereoizer = se
            .get("stereoizer")
            .and_then(Value::as_i64)
            .filter(|x| (0..=10).contains(x))
            .ok_or_else(invalid)?;
        let enabled = se.get("on").and_then(Value::as_bool).ok_or_else(invalid)?;
        if enabled != (presence != 0 || stereoizer != 0) {
            return Err(invalid());
        }
        Ok(Self {
            stereo: StereoSettings::new(enabled, presence as f32, stereoizer as f32, true)?,
        })
    }
    pub fn stereo(self) -> StereoSettings {
        self.stereo
    }
}

/// Real two-node computation in the confirmed 1AA50 order, with each original
/// variable-frame protocol. It does not skip or emulate unproven SDK wrappers.
pub struct LocalStereoSubgraph {
    stereo: StereoAmbienceOneStage,
    limiter: LimiterStage,
    format: Format,
    failed: bool,
}
impl LocalStereoSubgraph {
    pub fn new(format: Format, plan: LocalStereoMaterialPlan) -> Result<Self> {
        let mut stereo = StereoAmbienceOneStage::new(format)?;
        stereo.set_controls(plan.stereo.controls())?;
        stereo.set_bypass(!plan.stereo.enabled())?;
        let p = MATERIAL_BEGIN_LIMITER;
        let limiter = LimiterStage::new(
            format,
            LimiterSettings {
                enabled: true,
                threshold_db: p.threshold,
                attack_seconds: p.attack,
                release_seconds: p.release,
                input_gain_db: 0.0,
                output_gain_db: p.outputgain,
                infinite_ratio: p.infinite_ratio,
                ratio: p.ratio,
            },
        )?;
        Ok(Self {
            stereo,
            limiter,
            format,
            failed: false,
        })
    }
    pub fn format(&self) -> Format {
        self.format
    }
    pub fn lookahead_frames(&self) -> u32 {
        self.limiter.lookahead_frames()
    }
    pub fn configure(&mut self, plan: LocalStereoMaterialPlan) -> Result<()> {
        if self.failed {
            return Err(Error::State("local stereo subgraph failed"));
        }
        self.stereo.set_controls(plan.stereo.controls())?;
        self.stereo.set_bypass(!plan.stereo.enabled())
    }
    /// Original per-node reset; SE retains parent gains, cursors/EOF budget,
    /// while the default limiter restarts its proven lookahead/history state.
    pub fn reset(&mut self) -> Result<()> {
        if self.failed {
            return Err(Error::State("local stereo subgraph failed"));
        }
        self.stereo.reset()?;
        self.limiter.reset_stream_state();
        Ok(())
    }
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        if self.failed {
            return Err(Error::State("local stereo subgraph failed"));
        }
        if block.format() != self.format
            || frames > self.format.capacity()
            || block.samples().iter().any(|x| !x.is_finite())
        {
            return Err(Error::InvalidInput("invalid local stereo subgraph PCM"));
        }
        let result = self.stereo.process(block, frames, end).and_then(|flow| {
            self.limiter
                .process(block, flow.valid_frames as u32, flow.end)
        });
        match result {
            Ok(flow) if block.samples().iter().all(|x| x.is_finite()) => Ok(AmbienceOneResult {
                valid_frames: flow.output_frames as usize,
                end: flow.output_end,
            }),
            _ => {
                self.failed = true;
                block.samples_mut().fill(0.0);
                Err(Error::Media("local stereo subgraph processing failed"))
            }
        }
    }
}
