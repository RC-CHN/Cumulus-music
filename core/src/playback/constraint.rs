//! Host-imposed playback bounds intersect source authorization; never replace it.
use super::Timeline;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaybackConstraint {
    Blocked,
    Preview { start: f64, end: f64 },
}
impl PlaybackConstraint {
    pub fn from_resource(resource: &Value) -> Result<Option<Self>> {
        let Some(value) = resource.get("playback_constraint").filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        let constraint: Self = serde_json::from_value(value.clone())?;
        if let Self::Preview { start, end } = constraint
            && (!start.is_finite() || !end.is_finite() || start < 0.0 || end <= start)
        {
            return Err(Error::InvalidInput("invalid shared preview range"));
        }
        Ok(Some(constraint))
    }
    pub(crate) fn playable(resource: &Value) -> bool {
        matches!(
            Self::from_resource(resource),
            Ok(None | Some(Self::Preview { .. }))
        )
    }
    pub(crate) fn apply(value: Option<&Self>, timeline: Timeline) -> Result<Timeline> {
        match value {
            None => Ok(timeline),
            Some(Self::Blocked) => Err(Error::State("resource is not jointly playable")),
            Some(Self::Preview { start, end }) => timeline.restrict(*start, *end),
        }
    }
}
