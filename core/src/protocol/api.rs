//! Transport-independent recovered API contracts. A concrete HTTP transport
//! will implement `Api`; this layer never chooses a new protocol after failure.
use crate::{Error, Result, error::code_matches};
use serde_json::{Value, json};
use std::{future::Future, pin::Pin};

pub type ApiFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
pub type PodcastProgressPermission = dyn Fn() -> Result<bool> + Send + Sync;

/// Fixed logical read from M/182 -> M/438 for the standard vinyl panel.
/// Constructed from native current-track identity, never deserialized from IPC.
#[derive(Clone)]
pub struct VinylCommentRead {
    song_id: String,
    offset: usize,
}

impl VinylCommentRead {
    pub const LIMIT: usize = 60;

    pub fn new(song_id: &str, offset: usize) -> Result<Self> {
        if song_id.is_empty()
            || song_id.len() > 32
            || song_id.starts_with('0')
            || !song_id.bytes().all(|digit| digit.is_ascii_digit())
            || offset > 1_000_000
        {
            return Err(Error::InvalidInput("invalid vinyl comment page"));
        }
        Ok(Self {
            song_id: song_id.into(),
            offset,
        })
    }

    pub fn into_request(self) -> (String, Value) {
        (
            format!("/api/v1/resource/comments/R_SO_4_{}", self.song_id),
            json!({"limit":Self::LIMIT,"offset":self.offset,"beforeTime":0,"commentId":"0",
                "compareUserLocation":false,"composeConcert":false,"markReplied":false,
                "forceFlatComment":false,"showInner":false}),
        )
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

    /// The two official search-box long-audio charts are hidden by default.
    fn search_audio_toplist_assignment(&self) -> ApiFuture<'_, bool> {
        Box::pin(async { Ok(false) })
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
