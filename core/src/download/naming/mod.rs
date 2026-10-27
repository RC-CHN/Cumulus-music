mod clean;
mod official;
mod portable;

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathRequest {
    pub path_type: u8,
    pub file_type: u8,
    #[serde(default)]
    pub track: Option<Value>,
    #[serde(default)]
    pub voice: Option<Value>,
    #[serde(default)]
    pub mv: Option<Value>,
    pub offline_path_length: usize,
    pub suffix: String,
    #[serde(default)]
    pub yun_name: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct PathPlan {
    /// M/27 layout, with complete Unicode scalar values at a truncation edge.
    pub official_windows_path: String,
    /// Safe components for the host, also avoiding Windows device names.
    pub relative: PathBuf,
    pub portable_adjustment: bool,
}

/// Calculate a relative path, without touching the filesystem. The caller
/// supplies root length in UTF-16 units including its trailing separator.
pub fn plan_path(request: &PathRequest) -> Result<PathPlan> {
    if request.path_type > 2 || request.file_type > 2 {
        return Err(Error::InvalidInput("download layout must be 0, 1 or 2"));
    }
    if request.suffix.is_empty()
        || request.suffix.len() > 8
        || !request.suffix.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(Error::InvalidInput(
            "download suffix must be 1 to 8 ASCII letters/digits",
        ));
    }
    let mut parts = official::components(request)?;
    // Empty components never become a root/current/parent path. Native's
    // splice-during-forEach corner case is intentionally not reproduced.
    parts.retain(|s| !s.is_empty());
    let separators = parts.len().saturating_sub(1) * 2;
    let budget = 200usize
        .checked_sub(request.suffix.len())
        .and_then(|n| n.checked_sub(request.offline_path_length))
        .and_then(|n| n.checked_sub(separators))
        .filter(|n| *n >= parts.len() && !parts.is_empty())
        .ok_or(Error::InvalidInput(
            "download root leaves no filename budget",
        ))?;
    official::fit(&mut parts, budget)?;
    let official_windows_path = format!("{}.{}", parts.join("\\"), request.suffix);
    let portable_adjustment = portable::adapt(&mut parts, &request.suffix, budget)?;
    let mut relative: PathBuf = parts.iter().collect();
    let name = format!("{}.{}", parts.last().expect("nonempty"), request.suffix);
    relative.set_file_name(name);
    Ok(PathPlan {
        official_windows_path,
        relative,
        portable_adjustment,
    })
}
