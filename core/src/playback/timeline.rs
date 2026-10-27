//! Official M/110 display/seek rules. CUE clips the source clock; previews
//! instead offset the displayed clock by the service-authorized clip start.
use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineMode {
    Full,
    FullTrial,
    Cue,
    Trial,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeline {
    mode: TimelineMode,
    media_start: f64,
    display_start: f64,
    length: Option<f64>,
    total: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineSnapshot {
    pub mode: TimelineMode,
    pub start: f64,
    pub end: Option<f64>,
    pub length: Option<f64>,
    pub total: Option<f64>,
    pub media_start: f64,
}

fn finite(value: f64) -> Result<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Error::InvalidInput("nonfinite timeline position"))
    }
}
fn number(value: &Value) -> Result<f64> {
    finite(
        value
            .as_f64()
            .ok_or(Error::InvalidInput("invalid timeline metadata"))?,
    )
}

impl Timeline {
    /// Apply another authorized preview without confusing a full source's
    /// media clock with the zero-based clock of a downloaded preview clip.
    pub fn restrict(self, start: f64, end: f64) -> Result<Self> {
        finite(start)?;
        finite(end)?;
        if start < 0.0 || end <= start {
            return Err(Error::InvalidInput("invalid preview restriction"));
        }
        let start = start.max(self.display_start);
        let end = self.display_end().map_or(end, |limit| end.min(limit));
        if end <= start {
            return Err(Error::InvalidInput(
                "preview authorization ranges do not overlap",
            ));
        }
        Ok(Self {
            mode: TimelineMode::Trial,
            media_start: self.to_media(start)?,
            display_start: start,
            length: Some(end - start),
            total: self.total,
        })
    }
    pub fn from_source(info: &Value, media_duration: Option<f64>) -> Result<Self> {
        if !info.is_object() {
            return Err(Error::InvalidInput("expected source metadata"));
        }
        if let Some(duration) = media_duration
            && finite(duration)? <= 0.0
        {
            return Err(Error::InvalidInput("media duration must be positive"));
        }
        let segment = info.get("segment").filter(|v| !v.is_null());
        let trial = info
            .get("rights")
            .and_then(|v| v.get("freeTrialInfo"))
            .filter(|v| !v.is_null())
            .or_else(|| {
                info.get("resource_info")
                    .and_then(|v| v.get("freeTrialInfo"))
                    .filter(|v| !v.is_null())
            });
        if segment.is_some() && trial.is_some() {
            return Err(Error::InvalidInput(
                "cannot combine CUE and preview timelines",
            ));
        }
        if let Some(segment) = segment {
            let start = number(&segment["start"])?;
            let length = number(&segment["duration"])?;
            let end = finite(start + length)?;
            if start < 0.0 || length <= 0.0 || media_duration.is_some_and(|d| end > d + 0.02) {
                return Err(Error::InvalidInput("invalid CUE bounds"));
            }
            return Ok(Self {
                mode: TimelineMode::Cue,
                media_start: start,
                display_start: 0.0,
                length: Some(length),
                total: Some(length),
            });
        }
        if let Some(trial) = trial {
            let start = number(&trial["start"])?;
            let end = number(&trial["end"])?;
            if start < 0.0 || end <= start {
                return Err(Error::InvalidInput("invalid preview bounds"));
            }
            let total = info
                .get("track_duration")
                .filter(|v| !v.is_null())
                .map(number)
                .transpose()?;
            if total.is_some_and(|t| t < end) {
                return Err(Error::InvalidInput("preview exceeds track duration"));
            }
            let length = media_duration.map_or(end - start, |d| d.min(end - start));
            return Ok(Self {
                mode: TimelineMode::Trial,
                media_start: 0.0,
                display_start: start,
                length: Some(length),
                total,
            });
        }
        Ok(Self {
            mode: if info["full_trial"].as_bool() == Some(true) {
                TimelineMode::FullTrial
            } else {
                TimelineMode::Full
            },
            media_start: 0.0,
            display_start: 0.0,
            length: media_duration,
            total: media_duration,
        })
    }

    pub fn display_end(&self) -> Option<f64> {
        self.length.map(|length| self.display_start + length)
    }
    pub fn media_end(&self) -> Option<f64> {
        self.length.map(|length| self.media_start + length)
    }

    pub fn to_media(&self, position: f64) -> Result<f64> {
        let position = finite(position)?;
        if position < self.display_start || self.display_end().is_some_and(|end| position > end) {
            return Err(Error::InvalidInput("position outside playable range"));
        }
        finite(self.media_start + position - self.display_start)
    }

    pub fn from_media(&self, position: f64) -> Result<f64> {
        let mut relative = (finite(position)? - self.media_start).max(0.0);
        if let Some(length) = self.length {
            relative = relative.min(length);
        }
        finite(self.display_start + relative)
    }

    pub fn snapshot(&self) -> TimelineSnapshot {
        TimelineSnapshot {
            mode: self.mode,
            start: self.display_start,
            end: self.display_end(),
            length: self.length,
            total: self.total,
            media_start: self.media_start,
        }
    }
}
