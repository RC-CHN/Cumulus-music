//! Material-load controls for the final lookahead limiter. These events do
//! not process samples and are not a recipe for a fixed-size LADSPA filter.
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

/// Frame and EOF contract only. This does not buffer, produce, or modify PCM.
#[derive(Debug)]
pub struct LimiterFrameFlow {
    max_frames: u32,
    lookahead_frames: u32,
    skip: u32,
    drain: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimiterBlockFlow {
    pub output_frames: u32,
    pub output_end: bool,
    pub pad_input_frames: u32,
    pub skip_output_frames: u32,
}

impl MaterialLimiterPlan {
    /// Model one fresh fixed-format stream with this material already loaded.
    /// The native dynamic kernel and mid-stream updates are outside this API.
    pub fn frame_flow(
        &self,
        sample_rate: u32,
        channels: u32,
        max_frames: u32,
    ) -> Result<LimiterFrameFlow> {
        super::super::dsp::Format::new(sample_rate as f32, channels, max_frames)?;
        // The explicit native setter ignores nonpositive attack. At this
        // point the begin bridge has already applied the default.
        let attack = if self.parameters.attack > 0.0 {
            self.parameters.attack
        } else {
            MATERIAL_BEGIN_LIMITER.attack
        };
        let lookahead = if self.enabled {
            // The node's allocation at 64F83–64F97 is sized for 100 ms.
            // Bound this diagnostic to that capacity, never reproduce an
            // out-of-bounds or unverified native case.
            if attack > 0.1 {
                return Err(crate::Error::InvalidInput(
                    "limiter attack exceeds bounded frame flow",
                ));
            }
            // 6501F–6504C: truncate attack * rate * channels, then align to
            // a full frame. 6533F–65351 seeds both skip and drain to frames-1.
            let samples = (f64::from(sample_rate) * f64::from(attack) * f64::from(channels)) as u32;
            let frames = samples / channels;
            frames.checked_sub(1).ok_or(crate::Error::InvalidInput(
                "limiter attack is shorter than one frame",
            ))?
        } else {
            0
        };
        Ok(LimiterFrameFlow {
            max_frames,
            lookahead_frames: lookahead,
            skip: lookahead,
            drain: lookahead,
        })
    }
}

impl LimiterFrameFlow {
    pub fn lookahead_frames(&self) -> u32 {
        self.lookahead_frames
    }

    pub fn next_block(&mut self, input_frames: u32, input_end: bool) -> Result<LimiterBlockFlow> {
        if input_frames > self.max_frames {
            return Err(crate::Error::InvalidInput(
                "limiter input exceeds block capacity",
            ));
        }
        // 63C2E–63C77 extends EOF input within the current block capacity.
        let padding = if input_end {
            (self.max_frames - input_frames).min(self.drain)
        } else {
            0
        };
        self.drain -= padding;
        let processed = input_frames + padding;
        // 64B55–64BB0 withholds the initial lookahead frames, which can
        // span multiple input blocks or even the first EOF blocks.
        let skip = processed.min(self.skip);
        self.skip -= skip;
        Ok(LimiterBlockFlow {
            output_frames: processed - skip,
            output_end: input_end && self.drain == 0,
            pad_input_frames: padding,
            skip_output_frames: skip,
        })
    }
}
