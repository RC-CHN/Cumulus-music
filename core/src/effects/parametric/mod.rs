//! Native-only control for the recovered PEQ component, not a whole preset.
mod host;
pub use host::ParametricHost;

use super::{
    dsp::{ParametricEq, ParametricKind},
    material::{Component, PeqCall, apply_peq, peq_calls, route_sections},
};
use crate::{Error, Result};
use serde_json::Value;

pub const ABI_VERSION: u32 = 1;
pub const MAX_OPERATIONS: usize = 11;

/// POD operations passed synchronously to the native filter. The numeric tags
/// are gain=0, band=1 and enable=2. No strings or borrowed fields cross the ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PeqOperation {
    pub opcode: u32,
    pub band: u32,
    pub kind: u32,
    pub enabled: u32,
    pub gain: f32,
    pub frequency: f32,
    pub q: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ParametricPlan {
    calls: [PeqCall; MAX_OPERATIONS],
    len: usize,
}
impl ParametricPlan {
    /// Use only the component selected by EFB760, never a legacy PEQ fragment.
    pub fn from_material(material: &Value) -> Result<Option<Self>> {
        let route = route_sections(material)?;
        route
            .sections
            .into_iter()
            .find(|entry| entry.component == Component::Peq)
            .map(|entry| Self::from_calls(&peq_calls(entry.section)?))
            .transpose()
    }

    pub fn from_calls(calls: &[PeqCall]) -> Result<Self> {
        if calls.len() > MAX_OPERATIONS || !matches!(calls.last(), Some(PeqCall::On { .. })) {
            return Err(Error::InvalidInput("invalid bounded PEQ operation plan"));
        }
        let mut stage = [PeqCall::On { value: false }; MAX_OPERATIONS];
        for (index, call) in calls.iter().enumerate() {
            match *call {
                PeqCall::Gain { value } if value.is_finite() => {}
                PeqCall::Band {
                    band,
                    kind,
                    gain,
                    freq,
                    q,
                    ..
                } if gain.is_finite() && freq.is_finite() && q.is_finite() => {
                    if band < 9 {
                        ParametricKind::try_from(kind)?;
                    }
                }
                PeqCall::On { .. } if index + 1 == calls.len() => {}
                _ => return Err(Error::InvalidInput("invalid PEQ operation")),
            }
            stage[index] = *call;
        }
        Ok(Self {
            calls: stage,
            len: calls.len(),
        })
    }

    pub fn from_operations(ops: &[PeqOperation]) -> Result<Self> {
        if ops.len() > MAX_OPERATIONS {
            return Err(Error::InvalidInput("invalid bounded PEQ operation plan"));
        }
        let mut calls = [PeqCall::On { value: false }; MAX_OPERATIONS];
        for (call, raw) in calls.iter_mut().zip(ops) {
            *call = match raw.opcode {
                0 => PeqCall::Gain { value: raw.gain },
                1 if raw.enabled <= 1 => PeqCall::Band {
                    band: raw.band,
                    kind: raw.kind,
                    gain: raw.gain,
                    freq: raw.frequency,
                    q: raw.q,
                    on: raw.enabled != 0,
                },
                2 if raw.enabled <= 1 => PeqCall::On {
                    value: raw.enabled != 0,
                },
                _ => return Err(Error::InvalidInput("invalid PEQ operation tag or state")),
            }
        }
        Self::from_calls(&calls[..ops.len()])
    }

    pub fn enabled(self) -> bool {
        matches!(self.calls[self.len - 1], PeqCall::On { value: true })
    }

    /// Validates actual-rate coefficients before changing native state.
    pub fn apply(self, stage: &mut ParametricEq) -> Result<()> {
        apply_peq(stage, &self.calls[..self.len])
    }

    fn operations(self) -> ([PeqOperation; MAX_OPERATIONS], u32) {
        let mut result = [PeqOperation::default(); MAX_OPERATIONS];
        for (raw, call) in result.iter_mut().zip(&self.calls[..self.len]) {
            *raw = match *call {
                PeqCall::Gain { value } => PeqOperation {
                    gain: value,
                    ..Default::default()
                },
                PeqCall::Band {
                    band,
                    kind,
                    gain,
                    freq,
                    q,
                    on,
                } => PeqOperation {
                    opcode: 1,
                    band,
                    kind,
                    gain,
                    frequency: freq,
                    q,
                    enabled: on.into(),
                },
                PeqCall::On { value } => PeqOperation {
                    opcode: 2,
                    enabled: value.into(),
                    ..Default::default()
                },
            }
        }
        (result, self.len as u32)
    }
}
