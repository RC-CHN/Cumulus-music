//! Public metadata and transport time for native system media surfaces. Reading
//! this projection never initializes a player, fetches artwork, or makes HTTP calls.
use super::*;
use crate::{
    mv::playback::VideoStatus,
    playback::{PlaybackMode, controller::PlaybackStatus},
};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeMediaKind {
    Music,
    Video,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeMediaStatus {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeMediaItem {
    /// Opaque native selection identity, not an account/song ID or media URL.
    pub id: String,
    pub kind: NativeMediaKind,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    /// Whitelisted public image CDN URL, with all service query data removed.
    pub artwork: Option<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMediaDetails {
    pub transport: NativeMediaSnapshot,
    pub item: Option<NativeMediaItem>,
    pub state: NativeMediaStatus,
    /// Display seconds: CUE is clip-relative, preview retains the full-song offset.
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub seek_start: f64,
    pub seek_end: Option<f64>,
    pub can_seek: bool,
    pub volume: f64,
    pub speed: f64,
    /// Absent for MV and coordinator-owned FM, whose queue mode is not editable.
    pub mode: Option<PlaybackMode>,
}
impl Default for NativeMediaDetails {
    fn default() -> Self {
        Self {
            transport: NativeMediaSnapshot::default(),
            item: None,
            state: NativeMediaStatus::Stopped,
            position: None,
            duration: None,
            seek_start: 0.0,
            seek_end: None,
            can_seek: false,
            volume: 100.0,
            speed: 1.0,
            mode: None,
        }
    }
}
fn identity(target: &NativeMediaTarget, kind: &str, selection: &str) -> String {
    let mut hash = Sha256::new();
    // Length-delimited fields make identities unambiguous without exposing UID.
    for part in [
        "cumulus-native-media-v1",
        kind,
        target.token.uid.as_deref().unwrap_or(""),
        &target.token.generation.to_string(),
        selection,
    ] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    hex::encode(hash.finalize())
}
impl MusicClient {
    pub fn native_media_details(&self) -> Result<NativeMediaDetails> {
        let transport = self.native_media_snapshot()?;
        let Some(target) = transport.target.as_ref() else {
            return Ok(NativeMediaDetails::default());
        };
        let mut result = NativeMediaDetails::default();
        match target.media {
            Media::Audio(_) => {
                let value = self
                    .player
                    .controller
                    .get()
                    .ok_or(Error::StaleResponse)?
                    .snapshot();
                target.audio(&value)?;
                let selected = value.selected.as_ref().ok_or(Error::StaleResponse)?;
                let public = super::super::player::track(&selected.resource)?;
                result.item = Some(NativeMediaItem {
                    id: identity(target, "audio", &selected.entry_id),
                    kind: NativeMediaKind::Music,
                    title: public.name,
                    artists: public.artists,
                    album: public.album,
                    artwork: public.cover_url,
                });
                result.state = match value.state {
                    PlaybackStatus::Preparing | PlaybackStatus::Loading => {
                        NativeMediaStatus::Loading
                    }
                    PlaybackStatus::Playing | PlaybackStatus::Buffering => {
                        NativeMediaStatus::Playing
                    }
                    PlaybackStatus::Paused => NativeMediaStatus::Paused,
                    _ => NativeMediaStatus::Stopped,
                };
                result.position = value.position;
                result.duration = value.timeline.as_ref().and_then(|t| t.total).or_else(|| {
                    (public.duration_ms > 0).then_some(public.duration_ms as f64 / 1000.0)
                });
                if let Some(timeline) = value.timeline.as_ref() {
                    result.seek_start = timeline.start;
                    result.seek_end = timeline.end;
                    result.can_seek = matches!(
                        value.state,
                        PlaybackStatus::Playing
                            | PlaybackStatus::Paused
                            | PlaybackStatus::Buffering
                    ) && value.position.is_some();
                }
                result.volume = value.volume;
                result.speed = value.speed;
                result.mode = (!value.fm).then_some(value.mode);
            }
            Media::Video(_) => {
                let value = self
                    .mv
                    .existing(&target.token)?
                    .ok_or(Error::StaleResponse)?
                    .snapshot()?;
                target.video(&value)?;
                let detail = value.detail.as_ref();
                result.item = Some(NativeMediaItem {
                    id: identity(target, "video", &value.play_id),
                    kind: NativeMediaKind::Video,
                    title: transport.title.clone(),
                    artists: detail
                        .map(|d| d.artists.iter().map(|a| a.name.clone()).collect())
                        .unwrap_or_default(),
                    album: String::new(),
                    artwork: detail.and_then(|d| {
                        super::super::mapping::artwork(&serde_json::json!(d.cover_url))
                    }),
                });
                result.state = match value.status {
                    VideoStatus::Resolving | VideoStatus::Loading => NativeMediaStatus::Loading,
                    VideoStatus::Playing => NativeMediaStatus::Playing,
                    VideoStatus::Paused => NativeMediaStatus::Paused,
                    _ => NativeMediaStatus::Stopped,
                };
                result.position = value.position;
                result.duration = value.duration.or_else(|| {
                    detail
                        .and_then(|d| (d.duration_ms > 0).then_some(d.duration_ms as f64 / 1000.0))
                });
                result.seek_end = result.duration;
                result.can_seek =
                    matches!(value.status, VideoStatus::Playing | VideoStatus::Paused)
                        && value.position.is_some();
                result.volume = value.volume;
                result.speed = value.speed;
            }
        }
        target.check(self)?;
        result.transport = transport;
        Ok(result)
    }
}
