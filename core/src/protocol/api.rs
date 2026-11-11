//! Transport-independent recovered API contracts. A concrete HTTP transport
//! will implement `Api`; this layer never chooses a new protocol after failure.
use crate::{Error, Result, error::code_matches};
use serde_json::{Value, json};
use std::{future::Future, pin::Pin};

pub type ApiFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
pub type PodcastProgressPermission = dyn Fn() -> Result<bool> + Send + Sync;
pub type VinylCommentWritePermission<'a> = dyn Fn() -> Result<()> + Send + Sync + 'a;
pub type LyricBountyPermission<'a> = dyn Fn() -> Result<()> + Send + Sync + 'a;
pub type VinylPlaybackReadPermission<'a> = dyn Fn() -> Result<()> + Send + Sync + 'a;
pub type VinylWikiReadPermission<'a> = VinylPlaybackReadPermission<'a>;
pub type SongLikePermission<'a> = dyn Fn() -> Result<()> + Send + Sync + 'a;
pub type PlaylistAddPermission<'a> = dyn Fn() -> Result<()> + Send + Sync + 'a;
pub type PlaybackChorusPermission<'a> = dyn Fn() -> Result<()> + Send + Sync + 'a;
pub type MessageReadAllPermission<'a> = dyn Fn() -> Result<()> + Send + Sync + 'a;

pub(crate) const COMMENT_INFORMATION_PATH: &str = "/api/resource/commentInfo/list";
pub(crate) const MESSAGE_COUNT_PATH: &str = "/api/pl/count";
pub(crate) const MESSAGE_READ_ALL_PATH: &str = "/api/msg/count/clear";
pub(crate) const PODCAST_RESUME_WRITE_PATH: &str = "/api/dj/user/setting/resumeplay/update";
pub(crate) const PODCAST_RESUME_READ_PATH: &str = "/api/dj/user/setting/resumeplay/get";

/// One original chorus endpoint batch. The IDs are selected by the native queue.
pub struct PlaybackChorusRead(Vec<Value>);

impl PlaybackChorusRead {
    pub(crate) const PATH: &str = "/api/song/chorus";
    pub(crate) const LIMIT: usize = 500;

    pub(crate) fn new(ids: Vec<Value>) -> Result<Self> {
        if ids.is_empty() || ids.len() > Self::LIMIT || ids.iter().any(|id| identifier(id).is_err())
        {
            return Err(Error::InvalidInput("invalid chorus batch"));
        }
        Ok(Self(ids))
    }

    pub fn into_request(self) -> Result<(&'static str, Value)> {
        Ok((
            Self::PATH,
            json!({ "ids": serde_json::to_string(&self.0)? }),
        ))
    }
}

/// Fixed native reads needed to identify and confirm the system-liked playlist.
pub struct SystemLikedRead(SystemLikedQuery);
pub type SongCardLikedRead = SystemLikedRead;

enum SystemLikedQuery {
    StarList { owner: String, page: usize },
    Detail(String),
    Membership(String),
}

impl SystemLikedRead {
    pub(crate) const LIST: &str = "/api/user/playlist";
    pub(crate) const DETAIL: &str = "/api/v6/playlist/detail";
    pub(crate) const MEMBERSHIP: &str = "/api/song/like/get";
    pub(crate) const LIST_LIMIT: usize = 1000;
    pub(crate) const MAX_PAGES: usize = 10;

    pub(crate) fn list(owner: &str, page: usize) -> Self {
        Self(SystemLikedQuery::StarList {
            owner: owner.into(),
            page,
        })
    }

    pub(crate) fn detail(id: &str) -> Self {
        Self(SystemLikedQuery::Detail(id.into()))
    }

    pub(crate) fn membership(owner: &str) -> Self {
        Self(SystemLikedQuery::Membership(owner.into()))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        match self.0 {
            SystemLikedQuery::StarList { owner, page } => (
                Self::LIST,
                json!({"uid":owner,"offset":page * Self::LIST_LIMIT,"limit":Self::LIST_LIMIT}),
            ),
            SystemLikedQuery::Detail(id) => (Self::DETAIL, json!({"id":id,"n":100000,"s":0})),
            SystemLikedQuery::Membership(owner) => (Self::MEMBERSHIP, json!({"uid":owner})),
        }
    }
}

/// A native bounded song selection for an explicitly chosen playlist, never IPC data.
pub struct GuardedPlaylistAdd {
    playlist: String,
    songs: Vec<String>,
}

/// The recovered mixed unfav branch is a fixed remove; device IDs are never
/// part of its wire payload and the ordinary editor's `imme` is absent.
pub struct GuardedPlaylistRemove {
    playlist: String,
    songs: Vec<String>,
}

impl GuardedPlaylistRemove {
    pub(crate) const PATH: &str = GuardedPlaylistAdd::PATH;

    pub(crate) fn new(playlist: String, songs: Vec<String>) -> Result<Self> {
        if songs.is_empty() || songs.len() > 100_000 {
            return Err(Error::InvalidInput("invalid guarded remove selection"));
        }
        Ok(Self { playlist, songs })
    }

    pub fn into_request(self) -> Result<(&'static str, Value)> {
        let ids = packed_ids(
            &self
                .songs
                .into_iter()
                .map(Value::String)
                .collect::<Vec<_>>(),
        )?;
        Ok((
            Self::PATH,
            json!({"pid":identifier(&json!(self.playlist))?,"trackIds":ids,"op":"del"}),
        ))
    }
}

pub struct GuardedPlaylistCreate {
    uid: String,
    name: String,
    private: bool,
}

impl GuardedPlaylistCreate {
    pub(crate) const PATH: &str = "/api/playlist/create";

    pub(crate) fn new(uid: String, name: String, private: bool) -> Self {
        Self { uid, name, private }
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (
            Self::PATH,
            json!({"uid":self.uid,"name":self.name,"privacy":if self.private {10} else {0}}),
        )
    }
}

impl GuardedPlaylistAdd {
    pub(crate) const PATH: &str = "/api/v1/playlist/manipulate/tracks";

    pub(crate) const MAX_SONGS: usize = 100;
    // Recent history can merge 300 device rows with 300 service rows. Its
    // guarded collector preserves one intent and one ordered service write.
    pub(crate) const MAX_HISTORY_SONGS: usize = 600;

    pub(crate) fn new(playlist: String, songs: Vec<String>) -> Result<Self> {
        Self::with_limit(playlist, songs, Self::MAX_SONGS)
    }

    pub(crate) fn new_history(playlist: String, songs: Vec<String>) -> Result<Self> {
        Self::with_limit(playlist, songs, Self::MAX_HISTORY_SONGS)
    }

    fn with_limit(playlist: String, songs: Vec<String>, max: usize) -> Result<Self> {
        if !(1..=max).contains(&songs.len()) {
            return Err(Error::InvalidInput("invalid guarded add selection"));
        }
        Ok(Self { playlist, songs })
    }

    pub fn into_request(self) -> Result<(&'static str, Value)> {
        let ids = packed_ids(
            &self
                .songs
                .into_iter()
                .map(Value::String)
                .collect::<Vec<_>>(),
        )?;
        Ok((
            Self::PATH,
            json!({"pid":self.playlist,"trackIds":ids,"op":"add"}),
        ))
    }
}

/// A native account membership action. This is not an IPC payload.
pub struct GuardedSongLike {
    uid: String,
    track_id: String,
    liked: bool,
}

impl GuardedSongLike {
    pub(crate) const PATH: &str = "/api/song/like";

    pub(crate) fn new(uid: String, track_id: String, liked: bool) -> Self {
        Self {
            uid,
            track_id,
            liked,
        }
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (
            Self::PATH,
            json!({"trackId":self.track_id,"userid":self.uid,"like":self.liked}),
        )
    }
}

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

/// Fixed PC vinyl related-song query. The renderer never selects a seed.
pub struct VinylSimilarSongsRead(String);

impl VinylSimilarSongsRead {
    pub(crate) fn new(song_id: &str) -> Result<Self> {
        if !canonical_vinyl_comment_id(song_id) {
            return Err(Error::InvalidInput("invalid vinyl similar-song seed"));
        }
        Ok(Self(song_id.into()))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        ("/api/v1/discovery/simiSong", json!({"songid":self.0}))
    }
}

/// Fixed PC vinyl related-playlist query. The renderer never selects a seed.
pub struct VinylRelatedPlaylistsRead(String);

impl VinylRelatedPlaylistsRead {
    pub(crate) const PATH: &'static str = "/api/discovery/simiPlaylist";

    pub(crate) fn new(song_id: &str) -> Result<Self> {
        if !canonical_vinyl_comment_id(song_id) {
            return Err(Error::InvalidInput("invalid vinyl related-playlist seed"));
        }
        Ok(Self(song_id.into()))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (Self::PATH, json!({"songid":self.0}))
    }
}

/// The current ordinary episode’s SongCard read, not the embedded voice songs.
pub struct VinylVoiceSongsRead(String);

impl VinylVoiceSongsRead {
    pub(crate) const PATH: &'static str = "/api/v1/dj/program/tracks";

    pub(crate) fn new(voice_id: &str) -> Result<Self> {
        if voice_id.len() > 32 || identifier(&json!(voice_id)).is_err() {
            return Err(Error::InvalidInput("invalid vinyl episode seed"));
        }
        Ok(Self(voice_id.into()))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        // M15 puts `id` in the logical URL; M402 moves that string into
        // the request data before encrypting the normalized path.
        (Self::PATH, json!({"id":self.0}))
    }
}

/// Fixed M109.j/M15.yb PC detail read for the native current episode.
pub struct VinylVoiceDetailRead(String);

impl VinylVoiceDetailRead {
    pub(crate) const PATH: &'static str = "/api/dj/program/detail";

    pub(crate) fn new(voice_id: &str) -> Result<Self> {
        if !canonical_vinyl_comment_id(voice_id) {
            return Err(Error::InvalidInput("invalid vinyl voice detail seed"));
        }
        Ok(Self(voice_id.into()))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (Self::PATH, json!({"id":self.0}))
    }
}

/// Fixed, native-only preflight/read-back for the current playback radio.
pub struct PlayerPodcastResumeRead(String);

impl PlayerPodcastResumeRead {
    pub(crate) fn new(radio: &str) -> Result<Self> {
        if !canonical_vinyl_comment_id(radio) {
            return Err(Error::InvalidInput("invalid current podcast"));
        }
        Ok(Self(radio.into()))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (PODCAST_RESUME_READ_PATH, json!({"radioId":self.0}))
    }
}

/// Fixed, native-only update for the radio bound to the current playback entry.
pub struct PlayerPodcastResumeWrite {
    radio: String,
    enabled: bool,
}

impl PlayerPodcastResumeWrite {
    pub(crate) fn new(radio: &str, enabled: bool) -> Result<Self> {
        if !canonical_vinyl_comment_id(radio) {
            return Err(Error::InvalidInput("invalid current podcast"));
        }
        Ok(Self {
            radio: radio.into(),
            enabled,
        })
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (
            PODCAST_RESUME_WRITE_PATH,
            json!({"radioId":self.radio,"resumePlayValue":u8::from(self.enabled)}),
        )
    }
}

/// Read-back of the radio selected by a native fresh VoicePage receipt.
pub struct VinylPodcastSubscriptionRead(String);

impl VinylPodcastSubscriptionRead {
    pub(crate) const PATH: &'static str = "/api/djradio/v3/get";

    pub(crate) fn new(radio_id: &str) -> Result<Self> {
        if !canonical_vinyl_comment_id(radio_id) {
            return Err(Error::InvalidInput("invalid vinyl podcast target"));
        }
        Ok(Self(radio_id.into()))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (Self::PATH, json!({"id":self.0}))
    }
}

/// M15 On/Cn, authorized solely by a native fresh VoicePage receipt.
pub struct VinylPodcastSubscriptionWrite {
    id: String,
    desired: bool,
}

impl VinylPodcastSubscriptionWrite {
    pub(crate) fn new(id: &str, desired: bool) -> Result<Self> {
        let VinylPodcastSubscriptionRead(id) = VinylPodcastSubscriptionRead::new(id)?;
        Ok(Self { id, desired })
    }

    pub(crate) fn path(&self) -> &'static str {
        if self.desired {
            "/api/djradio/sub"
        } else {
            "/api/djradio/unsub"
        }
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (self.path(), json!({"id":self.id}))
    }
}

/// Fixed PC RcmdCard read for the native current-episode identity.
pub struct VinylVoiceRecommendationsRead(String);

impl VinylVoiceRecommendationsRead {
    pub(crate) const PATH: &'static str = "/api/voice/play/page/rcmd";

    pub(crate) fn new(voice_id: &str) -> Result<Self> {
        if !canonical_vinyl_comment_id(voice_id) {
            return Err(Error::InvalidInput(
                "invalid vinyl voice recommendation seed",
            ));
        }
        Ok(Self(voice_id.into()))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (Self::PATH, json!({"voiceId":self.0}))
    }
}

/// Fixed optional account reads for a trusted native Daily presentation.
pub struct DailyNotificationRead(Option<crate::catalog::DailyRecommendationEndpoint>);

impl DailyNotificationRead {
    pub(crate) const SETTINGS: &'static str = "/api/user/setting";

    pub(crate) fn settings() -> Self {
        Self(None)
    }

    pub(crate) fn recommendations(endpoint: crate::catalog::DailyRecommendationEndpoint) -> Self {
        Self(Some(endpoint))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        match self.0 {
            None => (Self::SETTINGS, json!({})),
            Some(endpoint) => (endpoint.path(), json!({"limit":30})),
        }
    }
}

/// One fixed PC read for the current copyright notification candidate.
pub struct CopyrightNotificationRead(Option<Vec<String>>);

impl CopyrightNotificationRead {
    pub(crate) const POPUP: &str = "/api/pc/popup/copyright";
    pub(crate) const DETAILS: &str = "/api/v3/song/detail";
    pub(crate) const MAX_SONGS: usize = 500;

    pub(crate) fn popup() -> Self {
        Self(None)
    }

    pub(crate) fn details(ids: &[Value]) -> Result<Self> {
        if !(1..=Self::MAX_SONGS).contains(&ids.len()) {
            return Err(Error::Response(
                "copyright song list is outside the reviewed bounds",
            ));
        }
        let ids = ids.iter().map(identifier).collect::<Result<Vec<_>>>()?;
        if ids.iter().any(|id| id.len() > 32) {
            return Err(Error::Response("invalid copyright song identity"));
        }
        Ok(Self(Some(ids)))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        match self.0 {
            None => (Self::POPUP, json!({})),
            Some(ids) => {
                let rows: Vec<_> = ids.into_iter().map(|id| json!({"id":id,"v":0})).collect();
                (
                    Self::DETAILS,
                    json!({"c":Value::Array(rows).to_string(),"trialMode":-1}),
                )
            }
        }
    }
}

/// Fixed signed encyclopedia query for the native current-track identity.
pub struct VinylWikiRead(String);

impl VinylWikiRead {
    pub(crate) const PATH: &'static str = "/api/music/wiki/home/song/desktop/get";

    pub(crate) fn new(song_id: &str) -> Result<Self> {
        if !canonical_vinyl_comment_id(song_id) {
            return Err(Error::InvalidInput("invalid vinyl wiki seed"));
        }
        Ok(Self(song_id.into()))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        let sign = super::wire::desktop_signature::v1_md5(format!("songId={}&", self.0));
        (Self::PATH, json!({"songId":self.0,"_scver":1,"_sign":sign}))
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

/// Fixed same-account mutation from a previously observed subscribed row.
pub struct PodcastBadgeClear(String);

impl PodcastBadgeClear {
    pub(crate) const PATH: &'static str = "/api/djradio/newcount/clear/byradio";

    pub(crate) fn new(radio_id: &str) -> Result<Self> {
        if radio_id.len() > 64 {
            return Err(Error::InvalidInput("invalid podcast badge radio ID"));
        }
        Ok(Self(identifier(&json!(radio_id))?))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (Self::PATH, json!({"radioId": self.0}))
    }
}

pub type PodcastBadgePermission<'a> = dyn Fn() -> Result<()> + Send + Sync + 'a;

pub type EffectCataloguePermission<'a> = dyn Fn() -> Result<()> + Send + Sync + 'a;

#[derive(Clone, Copy)]
pub enum EffectCatalogueRead {
    LegacyRecommended,
    LegacyAll,
    HighlyRecommended,
    Recommended,
    More,
    PcMaterialDetail(crate::effects::catalog::EffectId),
    MaterialAuthorization(crate::effects::catalog::EffectId),
    MaterialRecycle(crate::effects::catalog::EffectId),
}

impl EffectCatalogueRead {
    pub fn into_request(self) -> (&'static str, Value) {
        use EffectCatalogueRead::*;
        match self {
            LegacyRecommended => ("/api/usertool/sound/mobile/promote", json!({})),
            LegacyAll => ("/api/usertool/sound/mobile/all", json!({})),
            PcMaterialDetail(id) => (
                "/api/usertool/home/sound/info",
                effect_detail_payload(id, 10),
            ),
            MaterialAuthorization(id) => (
                "/api/vipauth/app/interests/auth/query",
                effect_authorization_payload(id),
            ),
            MaterialRecycle(id) => (
                "/api/vipauth/app/interests/recyclequery",
                json!({"queryInfo":{id.interests_type().to_string():[id.value().to_string()]}}),
            ),
            HighlyRecommended | Recommended | More => {
                let expose_type = match self {
                    HighlyRecommended => 5,
                    Recommended => 6,
                    More => 7,
                    _ => unreachable!(),
                };
                (
                    "/api/usertool/sound/queryAllSound",
                    json!({"exposeType":expose_type}),
                )
            }
        }
    }
}

pub(crate) fn effect_detail_payload(
    id: crate::effects::catalog::EffectId,
    sound_type: u32,
) -> Value {
    let requests = json!([{"soundId":id.value().to_string(),"soundType":sound_type}]);
    json!({"reqListStr":requests.to_string()})
}

pub(crate) fn effect_authorization_payload(id: crate::effects::catalog::EffectId) -> Value {
    json!({"queryInfo":{"interestsMap":{id.interests_type().to_string():[id.value()]}}})
}

/// A fixed native registration for one ordinary material effect.
pub struct NativeEffectRegistration(crate::effects::catalog::EffectId);

impl NativeEffectRegistration {
    pub(crate) fn ordinary(id: crate::effects::catalog::EffectId) -> Result<Self> {
        if id.interests_type() != 127 {
            return Err(Error::InvalidInput(
                "unsupported native effect registration type",
            ));
        }
        Ok(Self(id))
    }

    pub fn into_request(self) -> (&'static str, Value) {
        (
            "/api/vipauth/app/interests/auth/set",
            json!({"interestsType":127,"interestsId":self.0.value().to_string(),"type":0}),
        )
    }
}

pub trait Api: Send + Sync + std::any::Any {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value>;

    /// Account-side revision for the ordinary catalogue, available even when
    /// the profile has not enabled the signed PC experiment transport.
    fn effect_catalogue_credential_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Fixed ordinary effect read. Native HTTP rechecks the account and
    /// the caller's scope at the actual dispatch boundary.
    fn request_effect_catalogue<'a>(
        &'a self,
        read: EffectCatalogueRead,
        _revision: Option<&'a str>,
        current: &'a EffectCataloguePermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            current()?;
            let (path, payload) = read.into_request();
            self.call(path, payload, false, &[200]).await
        })
    }

    /// Require an actual native signed transport; generic adapters have no
    /// implicit write fallback for an owned-source registration.
    fn request_native_effect_registration<'a>(
        &'a self,
        _write: NativeEffectRegistration,
        _revision: &'a str,
        _permission: &'a EffectCataloguePermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::RequestNotSent(Box::new(Error::State(
                "native effect registration transport unavailable",
            ))))
        })
    }

    fn lyric_bounty_credential_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// A native PC capability must check authorization at the one dispatch
    /// boundary. Generic adapters do not gain a fallback write.
    fn request_lyric_bounty<'a>(
        &'a self,
        _write: crate::lyrics::LyricBountyWrite,
        _revision: &'a str,
        _permission: &'a LyricBountyPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::RequestNotSent(Box::new(Error::State(
                "signed lyric bounty transport unavailable",
            ))))
        })
    }

    /// The count, clear and account profile endpoints must see the same native
    /// authenticated cookie scope. Generic adapters have no write authority.
    fn message_read_all_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Final authorization and the dispatch flag must run together immediately
    /// before the fixed backend call; no fallback transport may send it.
    fn request_message_read_all<'a>(
        &'a self,
        _revision: &'a str,
        _permission: &'a MessageReadAllPermission<'a>,
        _dispatched: &'a std::sync::atomic::AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("message read-all authority unavailable")) })
    }

    /// Local proof that account identity, liked membership, and the write use
    /// the same scoped credentials. A signed PC caller also binds its card.
    fn song_like_scope_revision(&self, _signed_pc: bool) -> Result<Option<String>> {
        Ok(None)
    }

    /// A native signed SongCard download uses the same account as the card,
    /// membership read, and ordinary download authorization endpoint.
    fn song_card_download_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn song_card_playlist_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Verified PC authority for the current SongCard and existing owned destination picker.
    fn song_card_collection_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Current native account across the downloaded-song picker and its write.
    fn download_collection_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Account scope shared by the local-batch picker, detail readback and add.
    fn local_collection_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Creation must share the current download picker account and cookie scope.
    fn download_collection_create_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn download_system_liked_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn request_download_system_liked<'a>(
        &'a self,
        _read: SystemLikedRead,
        _revision: &'a str,
        _permission: &'a PlaylistAddPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("download system-liked read is unavailable")) })
    }

    fn song_card_system_liked_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn request_song_card_system_liked<'a>(
        &'a self,
        _read: SongCardLikedRead,
        _revision: &'a str,
        _permission: &'a PlaylistAddPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("signed system-liked read is unavailable")) })
    }

    fn song_card_playlist_create_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn song_card_collection_create_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Only a native transport can make the final synchronous dispatch check.
    fn request_guarded_playlist_add<'a>(
        &'a self,
        _write: GuardedPlaylistAdd,
        _permission: &'a PlaylistAddPermission<'a>,
        _dispatched: &'a std::sync::atomic::AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("guarded playlist add is unavailable")) })
    }

    fn request_guarded_playlist_remove<'a>(
        &'a self,
        _write: GuardedPlaylistRemove,
        _revision: &'a str,
        _permission: &'a PlaylistAddPermission<'a>,
        _dispatched: &'a std::sync::atomic::AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("guarded mixed playlist remove is unavailable")) })
    }

    fn request_guarded_mixed_playlist_add<'a>(
        &'a self,
        _write: GuardedPlaylistAdd,
        _revision: &'a str,
        _permission: &'a PlaylistAddPermission<'a>,
        _dispatched: &'a std::sync::atomic::AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("guarded mixed playlist add is unavailable")) })
    }

    fn mixed_playlist_add_scope_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn request_guarded_playlist_create<'a>(
        &'a self,
        _write: GuardedPlaylistCreate,
        _permission: &'a PlaylistAddPermission<'a>,
        _dispatched: &'a std::sync::atomic::AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("guarded playlist create is unavailable")) })
    }

    /// Call permission and mark dispatched immediately before the transport
    /// starts the write. No generic request fallback can honor that boundary.
    fn request_guarded_song_like<'a>(
        &'a self,
        _write: GuardedSongLike,
        _permission: &'a SongLikePermission<'a>,
        _dispatched: &'a std::sync::atomic::AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("guarded song like is unavailable")) })
    }

    /// Local scope proof for the ordinary playback URL request. A generic
    /// transport without scoped credentials cannot authorize stock fallback.
    fn playback_source_credential_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Cheap native commit observation for actor scheduling. The revision is
    /// not a permission and must be re-proved off actor before dispatch.
    fn observed_playback_chorus_revision(&self) -> Option<String> {
        None
    }

    /// Admits a queue chorus read only with exact native account credentials.
    fn playback_chorus_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn request_playback_chorus<'a>(
        &'a self,
        _read: PlaybackChorusRead,
        _revision: &'a str,
        _permission: &'a PlaybackChorusPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("queue chorus read is unavailable")) })
    }

    /// Scoped native credential revision for the episode progress write.
    /// Ordinary adapters do not opt into automatic account writes.
    fn podcast_progress_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Same native PC account, assignment, and resume-setting cookie scope.
    /// An ordinary read still works when this optional presentation proof is absent.
    fn podcast_resume_observation_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn player_podcast_resume_write_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn request_player_podcast_resume_read<'a>(
        &'a self,
        _read: PlayerPodcastResumeRead,
        _revision: &'a str,
        _permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("current podcast resume read is unavailable")) })
    }

    fn request_player_podcast_resume_write<'a>(
        &'a self,
        _write: PlayerPodcastResumeWrite,
        _revision: &'a str,
        _permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::RequestNotSent(Box::new(Error::State(
                "current podcast resume setting is unavailable",
            ))))
        })
    }

    fn upload_podcast_progress<'a>(
        &'a self,
        _payload: Value,
        _revision: &'a str,
        _allowed: &'a PodcastProgressPermission,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async { Err(Error::State("podcast progress write is unavailable")) })
    }

    fn podcast_badge_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn clear_podcast_badge<'a>(
        &'a self,
        _write: PodcastBadgeClear,
        _revision: &'a str,
        _permission: &'a PodcastBadgePermission<'a>,
        _dispatched: &'a std::sync::atomic::AtomicBool,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async { Err(Error::State("podcast badge write is unavailable")) })
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

    /// Presentation-only signed PC capability. Anonymous transport support
    /// does not attest that the official anonymous registration has completed.
    fn pc_presentation_credential_revision(&self) -> Result<Option<String>> {
        self.verified_pc_experiment_revision()
    }

    fn pc_presentation_credential_changes(
        &self,
    ) -> Result<Option<tokio::sync::watch::Receiver<String>>> {
        Ok(None)
    }

    fn pc_presentation_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, super::pc_experiments::PcExperimentGroups> {
        self.verified_pc_experiment_groups(revision)
    }

    fn pc_anonymous_state(&self) -> Result<super::pc_experiments::PcAnonymousState> {
        Ok(super::pc_experiments::PcAnonymousState::Unsupported)
    }

    fn register_pc_anonymous(&self) -> ApiFuture<'_, super::pc_experiments::PcAnonymousState> {
        Box::pin(async { Err(Error::State("PC anonymous registration is unavailable")) })
    }

    fn pc_anonymous_store_matches(&self, _directory: &std::path::Path) -> Result<bool> {
        Ok(false)
    }

    fn isolated_pc_anonymous_transport(
        &self,
        _directory: &std::path::Path,
    ) -> Result<Option<std::sync::Arc<dyn Api>>> {
        Ok(None)
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

    fn request_vinyl_similar_songs<'a>(
        &'a self,
        _read: VinylSimilarSongsRead,
        _revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::State(
                "vinyl similar-song dispatch authority unavailable",
            ))
        })
    }

    fn request_vinyl_related_playlists<'a>(
        &'a self,
        _read: VinylRelatedPlaylistsRead,
        _revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::State(
                "vinyl related-playlist dispatch authority unavailable",
            ))
        })
    }

    /// Fixed signed PC detail read; neither a seed nor credentials come from the renderer.
    fn request_vinyl_voice_detail<'a>(
        &'a self,
        _read: VinylVoiceDetailRead,
        _revision: &'a str,
        _permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::State(
                "vinyl voice detail dispatch authority unavailable",
            ))
        })
    }

    /// Native local proof that the verified profile and signed fresh detail
    /// use the same scoped credentials; no request or generic fallback.
    fn vinyl_voice_action_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn request_vinyl_podcast_subscription_state<'a>(
        &'a self,
        _read: VinylPodcastSubscriptionRead,
        _revision: &'a str,
        _permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("signed vinyl podcast state unavailable")) })
    }

    fn request_vinyl_podcast_subscription_write<'a>(
        &'a self,
        _write: VinylPodcastSubscriptionWrite,
        _revision: &'a str,
        _permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::RequestNotSent(Box::new(Error::State(
                "signed vinyl podcast write unavailable",
            ))))
        })
    }

    /// Fixed native read tied to one live episode. No generic API fallback.
    fn request_vinyl_voice_songs<'a>(
        &'a self,
        _read: VinylVoiceSongsRead,
        _revision: &'a str,
        _permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::State(
                "vinyl episode songs dispatch authority unavailable",
            ))
        })
    }

    /// Fixed same-credential reads with a late native presentation fence.
    fn request_daily_notification<'a>(
        &'a self,
        _read: DailyNotificationRead,
        _revision: &'a str,
        _permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::State(
                "signed daily notification read is unavailable",
            ))
        })
    }

    /// Signed PC read for one live playback selection; no generic API fallback.
    fn request_copyright_notification<'a>(
        &'a self,
        _read: CopyrightNotificationRead,
        _revision: &'a str,
        _permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::State(
                "signed copyright notification read is unavailable",
            ))
        })
    }

    fn request_vinyl_wiki<'a>(
        &'a self,
        _read: VinylWikiRead,
        _revision: &'a str,
        _permission: &'a VinylWikiReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("vinyl wiki dispatch authority unavailable")) })
    }

    /// Only call permission at the dispatch boundary. It consumes the user's
    /// live authorization once. RequestNotSent is reserved for pre-backend failures.
    fn request_vinyl_voice_recommendations<'a>(
        &'a self,
        _read: VinylVoiceRecommendationsRead,
        _revision: &'a str,
        _permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::State(
                "vinyl voice recommendations require a native signed transport",
            ))
        })
    }

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

    /// Recheck an already observed Search Song before reading its public count.
    /// Native HTTP implementations also fence the queued request at dispatch.
    fn request_search_song_comment_information<'a>(
        &'a self,
        payload: Value,
        revision: Option<&'a str>,
        permission: &'a (dyn Fn() -> Result<()> + Send + Sync),
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            permission()?;
            if self.search_privilege_credential_revision()?.as_deref() != revision {
                return Err(Error::CredentialChanged);
            }
            self.call(COMMENT_INFORMATION_PATH, payload, false, &[200])
                .await
        })
    }

    /// Optional exact PC account scope for an ordinary Song share privilege read.
    fn search_song_share_credential_revision(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn request_search_song_share_privilege<'a>(
        &'a self,
        _id: &'a str,
        _revision: &'a str,
        _permission: &'a (dyn Fn() -> Result<()> + Send + Sync),
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { Err(Error::State("search song sharing authority unavailable")) })
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

    fn request_search_with_correction_channel<'a>(
        &'a self,
        _path: &'a str,
        _payload: Value,
        _revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(Error::State(
                "search correction context authority unavailable",
            ))
        })
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
