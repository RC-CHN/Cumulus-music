//! Native device names are opaque, local identifiers. Never turn a UI string
//! into an arbitrary mpv option. Selection must use a freshly enumerated name.
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OutputDevice {
    pub id: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OutputDevices {
    pub devices: Vec<OutputDevice>,
    /// mpv's configured preference, NOT proof of the physical endpoint in use.
    pub configured: String,
    /// The active audio driver, absent until audio has initialized. This cannot
    /// establish which physical endpoint is receiving samples either.
    pub active_driver: Option<String>,
}

pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 4096 && !id.contains('\0')
}
