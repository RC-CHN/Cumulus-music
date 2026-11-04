//! Material-load controls for the final lookahead limiter. These events do
//! not process samples and are not a recipe for a fixed-size LADSPA filter.
use super::super::dsp::{Format, LimiterFrameFlow, LimiterSettings, LimiterStage};
use super::{
    Component, ControlCall, LimiterParameters, MaterialSchema, limiter_calls, route_sections,
};
use crate::Result;
use serde::Serialize;
use serde_json::Value;

/// Defaults from AudioEffectsImpl begin (neaudioeffects.dll x64 3.1.42),
/// after reset and clearing master bypass, before material component dispatch.
pub const MATERIAL_BEGIN_LIMITER: LimiterParameters = LimiterParameters {
    threshold: 0.0,
    attack: 0.005,
    release: 0.1,
    outputgain: 0.0,
    infinite_ratio: true,
    ratio: 4.0,
};

/// The observed node-level events, distinct from the subsequent outer
/// material setter calls. Each bypass=false occurs at a different step.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum LimiterBeginCall {
    ResetBypass {
        value: bool,
    },
    MasterBypassReset {
        value: bool,
    },
    Parameters {
        parameters: LimiterParameters,
        /// Byte +0x30 of the native struct passed to the limiter.
        flag_48: bool,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialLimiterPlan {
    pub schema: MaterialSchema,
    pub begin: [LimiterBeginCall; 3],
    /// EFA280 outer parameter/switch calls. Empty means no dispatch; a
    /// missing or mistyped on within an object emits a real off call.
    pub section: Vec<ControlCall>,
    pub enabled: bool,
    /// The last parameters sent, retained even when the section disables it.
    pub parameters: LimiterParameters,
}

pub fn material_limiter_plan(material: &Value) -> Result<MaterialLimiterPlan> {
    let routing = route_sections(material)?;
    let section = match routing
        .sections
        .iter()
        .find(|entry| entry.component == Component::Limiter)
    {
        Some(entry) => limiter_calls(entry.section)?,
        None => Vec::new(),
    };
    let mut enabled = true;
    let mut parameters = MATERIAL_BEGIN_LIMITER;
    for &call in &section {
        match call {
            ControlCall::LimiterParameters { parameters: next } => parameters = next,
            ControlCall::LimiterOn { value } => enabled = value,
            _ => unreachable!("limiter parser returned a different component"),
        }
    }
    Ok(MaterialLimiterPlan {
        schema: routing.schema,
        begin: [
            LimiterBeginCall::ResetBypass { value: false },
            LimiterBeginCall::MasterBypassReset { value: false },
            LimiterBeginCall::Parameters {
                parameters: MATERIAL_BEGIN_LIMITER,
                flag_48: true,
            },
        ],
        section,
        enabled,
        parameters,
    })
}

impl MaterialLimiterPlan {
    fn attack(&self) -> f32 {
        if self.parameters.attack > 0.0 {
            self.parameters.attack
        } else {
            MATERIAL_BEGIN_LIMITER.attack
        }
    }

    /// Model the frame protocol without applying the PCM kernel.
    pub fn frame_flow(
        &self,
        sample_rate: u32,
        channels: u32,
        max_frames: u32,
    ) -> Result<LimiterFrameFlow> {
        let format = Format::new(sample_rate as f32, channels, max_frames)?;
        LimiterFrameFlow::new(format, self.enabled, self.attack())
    }

    /// Prepare the recovered final node on a fresh stream. This is a single
    /// component renderer; it does not load the rest of the material graph.
    pub fn pcm_stage(&self, format: Format) -> Result<LimiterStage> {
        let parameters = self.parameters;
        LimiterStage::new(
            format,
            LimiterSettings {
                enabled: self.enabled,
                threshold_db: parameters.threshold,
                attack_seconds: self.attack(),
                release_seconds: if parameters.release > 0.0 {
                    parameters.release
                } else {
                    MATERIAL_BEGIN_LIMITER.release
                },
                input_gain_db: 0.0,
                output_gain_db: parameters.outputgain,
                infinite_ratio: parameters.infinite_ratio,
                ratio: parameters.ratio,
            },
        )
    }
}
