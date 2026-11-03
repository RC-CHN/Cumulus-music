use super::{AvatarCrop, safe_avatar};
use crate::{
    Error, Result,
    cloud::nos::{NosAllocation, NosStatus},
    playlist::cover::{INPUT_LIMIT, OUTPUT_LIMIT},
    protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvatarPhase {
    Draft,
    Prepared,
    Allocating,
    Uploading,
    Uploaded,
    Binding,
    Confirmed,
    Rejected,
    Abandoned,
    AcceptedCurrentState,
}

impl AvatarPhase {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Confirmed | Self::Rejected | Self::Abandoned | Self::AcceptedCurrentState
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvatarIssue {
    #[serde(rename = "avatar_allocation_uncertain")]
    AllocationUncertain,
    #[serde(rename = "avatar_upload_failed")]
    UploadFailed,
    #[serde(rename = "avatar_bind_uncertain")]
    BindUncertain,
    #[serde(rename = "avatar_bind_not_sent")]
    BindNotSent,
    #[serde(rename = "avatar_changed")]
    Changed,
    #[serde(rename = "avatar_rate_limited")]
    RateLimited,
    #[serde(rename = "avatar_phone_required")]
    PhoneRequired,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarTaskView {
    pub id: String,
    pub phase: AvatarPhase,
    pub crop: Option<AvatarCrop>,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub total: u64,
    pub transfer: Option<NosStatus>,
    pub error: Option<AvatarIssue>,
    pub may_leave_object: bool,
    pub cleanup_pending: bool,
    pub metadata_pending: bool,
    pub observed_revision: Option<String>,
    pub observed_avatar_url: Option<String>,
}

/// Native-only: image bytes, NOS scope and acknowledgement evidence are private.
#[derive(Serialize, Deserialize)]
pub(super) struct Task {
    pub id: String,
    pub created_at: u64,
    pub phase: AvatarPhase,
    pub width: u32,
    pub height: u32,
    pub source: String,
    pub source_sha: String,
    pub crop: Option<AvatarCrop>,
    pub encoded: String,
    pub sha256: String,
    pub md5: String,
    pub total: u64,
    pub baseline: Option<String>,
    pub allocation: Option<NosAllocation>,
    pub imgid: Option<Value>,
    pub acknowledged: Option<String>,
    pub observed: Option<String>,
    pub error: Option<AvatarIssue>,
    pub may_leave_object: bool,
}

pub(super) fn valid_id(id: &str) -> Result<()> {
    if !digest(id, 32) {
        return Err(Error::InvalidInput("invalid avatar task ID"));
    }
    Ok(())
}

fn digest(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn object_url(allocation: &NosAllocation) -> Result<url::Url> {
    if allocation.bucket() != "yyimgs" {
        return Err(Error::Response("avatar allocation returned another bucket"));
    }
    // M2167 chooses a p5/p6 CDN host. Keep one fixed member of that set so the
    // target remains identical across recovery; never interpret the key as a URL.
    let mut url = url::Url::parse("https://p5.music.126.net/").expect("compiled avatar CDN URL");
    let mut segments = url
        .path_segments_mut()
        .map_err(|_| Error::Protocol("avatar CDN path"))?;
    segments.pop_if_empty();
    for part in allocation.object_key().split('/') {
        if part.is_empty()
            || matches!(part, "." | "..")
            || !part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'=' | b'+'))
        {
            return Err(Error::Response("invalid avatar CDN object path"));
        }
        segments.push(part);
    }
    drop(segments);
    Ok(url)
}

impl Task {
    pub fn has_local_bytes(&self) -> bool {
        !self.source.is_empty() || !self.encoded.is_empty()
    }

    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        if self.width < 320
            || self.height < 320
            || u64::from(self.width) * u64::from(self.height) > 16_000_000
            || !digest(&self.source_sha, 64)
            || self.source.len() > INPUT_LIMIT.div_ceil(3) * 4
            || self.encoded.len() > OUTPUT_LIMIT.div_ceil(3) * 4
            || self.total > OUTPUT_LIMIT as u64
            || (!self.phase.terminal() && self.source.is_empty())
            || (!self.phase.terminal()
                && self.phase != AvatarPhase::Draft
                && self.encoded.is_empty())
        {
            return Err(Error::State("invalid persisted avatar task"));
        }
        if let Some(crop) = self.crop {
            crop.validate(self.width, self.height)?;
            if self.total == 0 || !digest(&self.sha256, 64) || !digest(&self.md5, 32) {
                return Err(Error::State("invalid cropped avatar record"));
            }
        } else if self.phase != AvatarPhase::Draft && self.phase != AvatarPhase::Abandoned {
            return Err(Error::State("missing persisted avatar crop"));
        }
        if let Some(allocation) = &self.allocation {
            allocation.validate()?;
            object_url(allocation)?;
            if self
                .imgid
                .as_ref()
                .and_then(|value| identifier(value).ok())
                .as_deref()
                != Some(allocation.resource_id())
            {
                return Err(Error::State("avatar document ID mismatch"));
            }
        } else if self.imgid.is_some()
            || matches!(
                self.phase,
                AvatarPhase::Uploading
                    | AvatarPhase::Uploaded
                    | AvatarPhase::Binding
                    | AvatarPhase::Confirmed
                    | AvatarPhase::Rejected
                    | AvatarPhase::AcceptedCurrentState
            )
        {
            return Err(Error::State("missing avatar allocation"));
        }
        for url in [&self.baseline, &self.acknowledged, &self.observed]
            .into_iter()
            .flatten()
        {
            if safe_avatar(&Value::String(url.clone())).as_deref() != Some(url.as_str()) {
                return Err(Error::State("invalid persisted avatar locator"));
            }
        }
        if matches!(
            self.phase,
            AvatarPhase::Allocating
                | AvatarPhase::Uploading
                | AvatarPhase::Uploaded
                | AvatarPhase::Binding
                | AvatarPhase::Confirmed
                | AvatarPhase::Rejected
                | AvatarPhase::AcceptedCurrentState
        ) && self.baseline.is_none()
        {
            return Err(Error::State("missing avatar baseline"));
        }
        Ok(())
    }

    pub fn observed_revision(&self) -> Option<String> {
        use sha2::{Digest, Sha256};
        self.observed
            .as_ref()
            .map(|url| hex::encode(Sha256::digest(url.as_bytes())))
    }
}
