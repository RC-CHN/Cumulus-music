//! Safe owned boundaries around the recovered C filter/GEQ/BassTreble/PEQ.
//! These components do not choose an entire material graph or grant rights.

mod block;
mod dynamics;
mod dynamics_parameters;
mod ffi;
mod filter;
mod graphic;
mod limiter;
mod loudness;
mod parametric;
mod rotation;
mod rotation_rate;
mod rotation_resampled;
mod state;

pub use block::{Format, PlanarBlock};
pub use dynamics::MultibandDynamics;
pub use dynamics_parameters::{DynamicsBand, DynamicsParameters, DynamicsSettings};
pub use filter::{Biquad, Coefficients, GraphicKind, ParametricKind};
pub use graphic::{BassTreble, BassTrebleSettings, GraphicEq};
pub use limiter::{
    LimiterBlockFlow, LimiterFrameFlow, LimiterSettings, LimiterStage, LimiterStream,
    LimiterStreamQuantum, LimiterStreamRead, LimiterStreamWrite,
};
pub use loudness::{LoudnessParameters, LoudnessStage};
pub use parametric::{ParametricBand, ParametricEq};
pub use rotation::{RotationKernel, RotationResponses, RotationResult, RotationStage};
pub use rotation_rate::{ROTATION_KERNEL_RATE, RotationRateConversion, RotationRatePlan};
pub use rotation_resampled::{ResampledRotationStage, RotationRateConverter};
