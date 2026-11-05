//! Transport-independent recovered API contracts. A concrete HTTP transport
//! will implement `Api`; this layer never chooses a new protocol after failure.
use crate::{Error, Result, error::code_matches};
use serde_json::{Value, json};
use std::{future::Future, pin::Pin};

pub type ApiFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
pub type PodcastProgressPermission = dyn Fn() -> Result<bool> + Send + Sync;
pub type VinylCommentWritePermission<'a> = dyn Fn() -> Result<()> + Send + Sync + 'a;

fn canonical_vinyl_comment_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && !id.starts_with('0')
        && id.bytes().all(|digit| digit.is_ascii_digit())
}

/// Fixed panel and single-comment verification reads.
/// Constructed from native current-track identity, never deserialized from IPC.
#[derive(Clone)]
pub struct VinylCommentRead {
    song_id: String,
    offset: usize,
    comment_id: Option<String>,
}

impl VinylCommentRead {
    pub const LIMIT: usize = 60;

    pub fn new(song_id: &str, offset: usize) -> Result<Self> {
        if !canonical_vinyl_comment_id(song_id) || offset > 1_000_000 {
            return Err(Error::InvalidInput("invalid vinyl comment page"));
        }
        Ok(Self {
            song_id: song_id.into(),
            offset,
            comment_id: None,
        })
    }

    pub fn target(song_id: &str, comment_id: &str) -> Result<Self> {
        if !canonical_vinyl_comment_id(comment_id) {
            return Err(Error::InvalidInput("invalid vinyl comment target"));
        }
        let mut read = Self::new(song_id, 0)?;
        read.comment_id = Some(comment_id.into());
        Ok(read)
    }

    pub fn into_request(self) -> (String, Value) {
        let limit = if self.comment_id.is_some() {
            1
        } else {
            Self::LIMIT
        };
        (
            format!("/api/v1/resource/comments/R_SO_4_{}", self.song_id),
            json!({"limit":limit,"offset":self.offset,"beforeTime":0,"commentId":self.comment_id.as_deref().unwrap_or("0"),
                "compareUserLocation":false,"composeConcert":false,"markReplied":false,
                "forceFlatComment":false,"showInner":false}),
        )
    }
}

/// Fixed mutations from an already authorized current track (M/438, M/232, M/55).
pub struct VinylCommentWrite {
    song_id: String,
    action: VinylCommentWriteAction,
}

enum VinylCommentWriteAction {
    Publish {
        content: String,
        reply_to: Option<String>,
    },
    Like {
        comment_id: String,
        desired: bool,
    },
    Report {
        comment_id: String,
        reason: String,
    },
}

impl VinylCommentWrite {
    pub fn new(song_id: &str, content: &str, reply_to: Option<&str>) -> Result<Self> {
        if !canonical_vinyl_comment_id(song_id)
            || reply_to.is_some_and(|id| !canonical_vinyl_comment_id(id))
        {
            return Err(Error::InvalidInput("invalid vinyl comment write target"));
        }
        crate::comments::validate_comment_text(content)?;
        Ok(Self {
            song_id: song_id.into(),
            action: VinylCommentWriteAction::Publish {
                content: content.into(),
                reply_to: reply_to.map(str::to_owned),
            },
        })
    }

    pub fn like(song_id: &str, comment_id: &str, desired: bool) -> Result<Self> {
        if !canonical_vinyl_comment_id(song_id) || !canonical_vinyl_comment_id(comment_id) {
            return Err(Error::InvalidInput("invalid vinyl comment like target"));
        }
        Ok(Self {
            song_id: song_id.into(),
            action: VinylCommentWriteAction::Like {
                comment_id: comment_id.into(),
                desired,
            },
        })
    }

    pub(crate) fn report(song_id: &str, comment_id: &str, reason: String) -> Result<Self> {
        VinylCommentRead::target(song_id, comment_id)?;
        Ok(Self {
            song_id: song_id.into(),
            action: VinylCommentWriteAction::Report {
                comment_id: comment_id.into(),
                reason,
            },
        })
    }

    pub fn path(&self) -> &'static str {
        match &self.action {
            VinylCommentWriteAction::Publish {
                reply_to: Some(_), ..
            } => "/api/v1/resource/comments/reply",
            VinylCommentWriteAction::Publish { reply_to: None, .. } => "/api/resource/comments/add",
            VinylCommentWriteAction::Like { desired: true, .. } => "/api/v1/comment/like",
            VinylCommentWriteAction::Like { desired: false, .. } => "/api/v1/comment/unlike",
            VinylCommentWriteAction::Report { .. } => "/api/report/reportcomment",
        }
    }

    pub fn into_request(self) -> (&'static str, Value) {
        let path = self.path();
        let thread_id = format!("R_SO_4_{}", self.song_id);
        let payload = match self.action {
            VinylCommentWriteAction::Publish {
                content,
                reply_to: Some(id),
            } => {
                json!({"threadId":thread_id,"commentId":id,"content":content,"resourceType":0,"resourceId":0})
            }
            VinylCommentWriteAction::Publish {
                content,
                reply_to: None,
            } => json!({"threadId":thread_id,"content":content,"resourceType":0,"resourceId":0,
                    "expressionPicId":-1,"bubbleId":-1}),
            VinylCommentWriteAction::Like { comment_id, .. } => {
                json!({"threadId":thread_id,"commentId":comment_id})
            }
            VinylCommentWriteAction::Report { comment_id, reason } => {
                json!({"threadId":thread_id,"commentId":comment_id,"reason":reason})
            }
        };
        (path, payload)
    }
}

pub trait Api: Send + Sync + std::any::Any {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value>;

    /// Scoped native credential revision for the episode progress write.
    /// Ordinary adapters do not opt into automatic account writes.
    fn podcast_progress_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn upload_podcast_progress<'a>(
        &'a self,
        _payload: Value,
        _revision: &'a str,
        _allowed: &'a PodcastProgressPermission,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async { Err(Error::State("podcast progress write is unavailable")) })
    }

    /// Native signed authority only. A generic API adapter does not acquire
    /// experiment access by implementing the ordinary request transport.
    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn verified_pc_experiment_groups<'a>(
        &'a self,
        _revision: &'a str,
    ) -> ApiFuture<'a, super::pc_experiments::PcExperimentGroups> {
        Box::pin(async {
            Err(Error::State(
                "PC experiment requires a native signed transport",
            ))
        })
    }

    /// The scoped read must fence the signed cohort revision at dispatch.
    /// Ordinary request transports cannot emulate that native authority.
    fn request_vinyl_comment_page<'a>(
        &'a self,
        _read: VinylCommentRead,
        _revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("vinyl comment dispatch authority unavailable")) })
    }

    /// Only call permission at the dispatch boundary. It consumes the user's
    /// live authorization once. RequestNotSent is reserved for pre-backend failures.
    fn request_vinyl_comment_write<'a>(
        &'a self,
        _write: VinylCommentWrite,
        _revision: &'a str,
        _permission: &'a VinylCommentWritePermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::RequestNotSent(Box::new(Error::State(
                "vinyl comment write authority unavailable",
            ))))
        })
    }

    /// The two official search-box long-audio charts are hidden by default.
    fn search_audio_toplist_assignment(&self) -> ApiFuture<'_, bool> {
        Box::pin(async { Ok(false) })
    }

    /// Optional native credential version used by authoritative membership reads.
    fn membership_credential_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Profile revision used to fence native search privilege receipts.
    fn search_privilege_credential_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Internal authority for a default search context from this credential set.
    fn search_default_context_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn request_search_with_default_context<'a>(
        &'a self,
        _path: &'a str,
        _payload: Value,
        _revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("search default context authority unavailable")) })
    }

    fn playback_power_experiment(
        &self,
    ) -> ApiFuture<'_, Option<super::pc_experiments::PcExperimentGroups>> {
        Box::pin(async { Ok(None) })
    }

    fn queue_more_experiment_groups(
        &self,
    ) -> ApiFuture<'_, super::pc_experiments::PcExperimentGroups> {
        Box::pin(async {
            Ok(super::pc_experiments::PcExperimentGroups::from_startup_cache(&Value::Null))
        })
    }

    /// Native transport identity used by device-scoped service settings. Never
    /// accepted from IPC payloads; adapters without a bound identity are read-only.
    fn local_device_id(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Best available native service clock, for informational time windows.
    /// Fixture and other transports can leave this unavailable.
    fn observed_server_time_ms(&self) -> Option<u64> {
        None
    }

    fn call<'a>(
        &'a self,
        path: &'a str,
        payload: Value,
        cache: bool,
        accepted_codes: &'a [i64],
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let value = self.request(path, payload, cache).await?;
            validate_response(path, value, accepted_codes)
        })
    }

    fn account(&self) -> ApiFuture<'_, Value> {
        Box::pin(async move { Ok(self.account_reply().await?["profile"].clone()) })
    }

    /// Native identity envelope. Account commands return only typed projections.
    fn account_reply(&self) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            const PATH: &str = "/api/w/nuser/account/get";
            let response = self.call(PATH, json!({}), false, &[200]).await?;
            match response.get("profile") {
                Some(profile) if profile.is_object() && identifier(&profile["userId"]).is_ok() => {
                    Ok(response)
                }
                _ => Err(Error::service(PATH, json!(301), response)),
            }
        })
    }
}

pub fn validate_response(path: &str, value: Value, accepted_codes: &[i64]) -> Result<Value> {
    if !value.is_object() {
        return Err(Error::Response("API response must be an object"));
    }
    let code = &value["code"];
    if !accepted_codes
        .iter()
        .any(|expected| code_matches(code, *expected))
    {
        return Err(Error::service(path, code.clone(), value));
    }
    Ok(value)
}

pub fn identifier(value: &Value) -> Result<String> {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => {
            return Err(Error::InvalidInput(
                "expected a positive decimal identifier",
            ));
        }
    };
    if text.is_empty()
        || !text.bytes().all(|c| c.is_ascii_digit())
        || !text.bytes().any(|c| c != b'0')
    {
        return Err(Error::InvalidInput(
            "expected a positive decimal identifier",
        ));
    }
    // No u64 narrowing: official IDs are decimal strings, including leading
    // zeros and values larger than JavaScript's exact integer range.
    Ok(text)
}

pub fn packed_ids(values: &[Value]) -> Result<String> {
    Ok(serde_json::to_string(
        &values.iter().map(identifier).collect::<Result<Vec<_>>>()?,
    )?)
}

pub fn official_text_length(value: &str) -> usize {
    value
        .encode_utf16()
        .map(|unit| if unit <= 255 { 1 } else { 2 })
        .sum()
}

pub fn playlist_name(value: &str) -> Result<String> {
    // Python's recovered validator also strips the four ASCII information
    // separators, unlike Rust str::trim. Preserve that accepted-input boundary.
    let value =
        value.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c));
    if value.is_empty() || value.contains(['@', '#']) || official_text_length(value) > 40 {
        return Err(Error::InvalidInput(
            "invalid playlist name or weighted length exceeds 40",
        ));
    }
    Ok(value.into())
}
