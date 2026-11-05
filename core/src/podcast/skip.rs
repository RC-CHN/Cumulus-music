//! Per-audiobook intro/outro settings, independent of episode authorization
//! and playback-history writes. Official M/144, M/308, M/3045 and PlayProgress.
use super::PodcastApi;
use crate::{Error, Result, protocol::api::identifier, storage::StateStore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const SETTINGS: &str = "podcast-audiobook-skip-v1";
const MANUAL_MAX_SECONDS: u8 = 90;
const SHORT_PROGRAM_SECONDS: f64 = 200.0;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudiobookSkip {
    #[default]
    Close,
    Smart,
    Custom {
        start: u8,
        end: u8,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AudiobookIntervals {
    intro: f64,
    outro: f64,
}

impl AudiobookIntervals {
    pub(crate) fn fits(self, duration: f64) -> bool {
        self.intro < duration
            && (duration < SHORT_PROGRAM_SECONDS || self.intro + self.outro < duration)
    }

    /// M/308 applies this at startup, not when the user moves a slider or seeks.
    pub fn start_position(self, current: f64, duration: f64) -> f64 {
        if !duration.is_finite() || duration <= 0.0 || self.intro > duration {
            return current;
        }
        current.max(self.intro)
    }

    /// PlayProgress uses the native whole-second sample. Preview or noncurrent
    /// sources must be ruled out by the actor before evaluating this policy.
    pub fn reaches_outro(self, position: f64, duration: f64) -> bool {
        self.outro > 0.0
            && self.outro < duration
            && position.is_finite()
            && position >= 0.0
            && duration.is_finite()
            && duration >= SHORT_PROGRAM_SECONDS
            && position.floor() + self.outro >= duration
    }
}

/// M/144.c uses strict numeric category IDs or one of three exact names.
pub fn is_audiobook(radio: &Value) -> bool {
    matches!(
        radio["category"].as_str(),
        Some("有声书" | "广播剧" | "文学出版")
    ) || matches!(
        radio["categoryId"].as_f64(),
        Some(10001.0 | 3088097.0 | 3148096.0)
    )
}

impl AudiobookSkip {
    fn validate(self) -> Result<Self> {
        if let Self::Custom { start, end } = self
            && (start > MANUAL_MAX_SECONDS || end > MANUAL_MAX_SECONDS)
        {
            return Err(Error::InvalidInput(
                "audiobook skip interval exceeds 90 seconds",
            ));
        }
        Ok(self)
    }

    pub fn read(store: &StateStore, owner: &str, radio_id: &str) -> Result<Self> {
        let radio_id = identifier(&json!(radio_id))?;
        match store.get(owner, SETTINGS, &radio_id)? {
            Some(value) => serde_json::from_value::<Self>(value)?.validate(),
            None => Ok(Self::Close),
        }
    }

    pub fn save(self, store: &StateStore, owner: &str, radio_id: &str) -> Result<()> {
        let radio_id = identifier(&json!(radio_id))?;
        match self.validate()? {
            Self::Close => store.delete(owner, SETTINGS, &radio_id),
            value => store.put(owner, SETTINGS, &radio_id, &serde_json::to_value(value)?),
        }
    }

    /// The persisted selection is per radio; only smart asks for episode data.
    /// This is preparation, not a claim that any particular source is skippable.
    pub async fn resolve(
        self,
        api: &PodcastApi,
        episode_id: &str,
        duration: f64,
    ) -> Result<AudiobookIntervals> {
        if !duration.is_finite() || duration <= 0.0 {
            return Ok(AudiobookIntervals::default());
        }
        match self.validate()? {
            Self::Close => Ok(AudiobookIntervals::default()),
            Self::Custom { start, end } => Ok(AudiobookIntervals {
                intro: if duration < SHORT_PROGRAM_SECONDS {
                    0.0
                } else {
                    f64::from(start)
                },
                outro: f64::from(end),
            }),
            Self::Smart => api.audiobook_skip_intervals(episode_id).await,
        }
    }
}

impl PodcastApi {
    pub async fn audiobook_skip_intervals(&self, episode_id: &str) -> Result<AudiobookIntervals> {
        // M/21.W makes the voice ID a string; M/86 passes a real one-element
        // array to programIdsStr. It is not a list of song IDs or a JSON string.
        let episode_id = identifier(&json!(episode_id))?;
        let result = self
            .api
            .call(
                "/api/voice/alg/skip/info/get",
                json!({"programIdsStr":[episode_id]}),
                false,
                &[200],
            )
            .await?;
        let rows = result["data"]
            .as_array()
            .filter(|rows| rows.len() <= 1)
            .ok_or(Error::Response("invalid audiobook skip response"))?;
        let Some(row) = rows.first() else {
            return Ok(AudiobookIntervals::default());
        };
        if !row.is_object()
            || row
                .get("programId")
                .is_some_and(|value| identifier(value).as_deref().ok() != Some(episode_id.as_str()))
        {
            return Err(Error::Response("audiobook skip response identity mismatch"));
        }
        let interval = |field: &str| {
            row.get(field).map_or(Ok(0.0), |value| {
                value
                    .as_f64()
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .ok_or(Error::Response("invalid audiobook skip interval"))
            })
        };
        Ok(AudiobookIntervals {
            intro: interval("skipBeginDuration")?,
            outro: interval("skipEndDuration")?,
        })
    }
}
