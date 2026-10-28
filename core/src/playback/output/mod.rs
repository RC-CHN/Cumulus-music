//! Device-local output preferences and native application policy. Device names
//! are preferences, not evidence of a physical endpoint receiving samples.
mod managed;
mod preferences;
mod runtime;
pub use managed::ManagedOutput;
pub use preferences::{OutputPreferences, OutputSelection};
pub use runtime::{
    OutputIssue, OutputKind, OutputLease, OutputObservation, OutputPhase, OutputRuntime,
    OutputSnapshot,
};
