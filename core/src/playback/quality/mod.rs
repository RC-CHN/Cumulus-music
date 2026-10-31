//! Official M/40, M/86 and M/100 song quality capabilities and use rights.
//! Capability, entitlement and the actual authorized stream remain distinct.
mod api;
mod policy;
mod read;
pub mod trials;
pub use api::{QualityApi, QualityFormat, QualityRight, QualitySizes, QualityUseRight, TrialPhase};
pub use policy::{
    QualityAccess, QualityAvailability, QualityExperiments, QualityPolicy, QualityPrivilege,
    QualitySubscriber,
};
pub use read::{QualityInformation, QualityReadState, QualityReadStatus, QualityReader};
