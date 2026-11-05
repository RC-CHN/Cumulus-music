//! Host-independent UI use cases. Native records and credentials never cross
//! this boundary; account epochs also reject delayed frontend results.
mod account_avatar;
mod account_binding_qr;
mod account_profile;
pub mod artist_onboarding;
mod auth;
mod external_import;
mod external_import_sidebar;
pub use auth::LoginCaptchaProvider;
mod catalog;
mod cloud;
mod cold_offline;
pub use cold_offline::{ColdOfflinePageView, ColdOfflinePlayerView};
mod collections;
mod comments;
mod community;
mod content;
mod daily_experiment;
pub mod discovery_home;
mod downloads;
pub mod dto;
mod effects;
mod vertical_zone;
pub use effects::{NativeParametricActivation, NativeParametricActivationAttempt};
mod error;
mod external_import_auth;
pub use external_import_auth::{
    ExternalImportAuthAction, ExternalImportAuthState, ExternalImportAuthView,
};
mod fm;
mod fm_discovery;
mod focus;
mod history;
mod likes;
mod local;
mod login_device_removal;
mod login_devices;
mod mapping;
mod media_preferences;
mod member;
pub mod messages;
pub mod mini;
mod mini_preferences;
mod mv;
pub mod native_media;
mod ordinary_essential;
pub mod pc_experiments;
mod player;
mod playlist_cover;
mod playlist_creator;
mod playlist_essential;
mod playlist_square;
mod playlist_tags_browse;
mod playlists;
mod podcast;
mod profile_blacklist;
mod profile_follow;
mod profile_play_record;
mod profile_relations;
mod profile_report;
mod quality_experiment;
mod rankings;
#[allow(
    dead_code,
    reason = "no production recognition capture entry yet; exercised by closed core fixtures"
)]
mod recognition_results;
mod recommendations;
mod relay;
mod remote_skins;
mod report_receipt;
mod scene_fm;
mod search;
mod settings;
mod shortcut_preferences;
pub mod style_home;
pub mod style_tags;
mod together;
mod window;
pub use player::{PlaybackSubscription, VinylCommentPageView};

use crate::{
    Result,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    protocol::{
        api::Api,
        context::RequestContext,
        http::{
            ApiOrigin, ClientProfile, HttpBackend, MusicHttpClient, OFFICIAL_WINDOWS_USER_AGENT,
            RequestOptions, ReqwestBackend, RoutingPolicy,
        },
    },
    storage::StateStore,
};
pub use error::ClientError;
use std::{path::Path, sync::Arc};

pub struct MusicClient {
    external_import: external_import::ExternalImportHost,
    external_import_sidebar: bool,
    member_welfare: member::MemberWelfareHost,
    remote_skins: remote_skins::RemoteSkinHost,
    session: Arc<AccountSession>,
    registry: Arc<ProfileRegistry>,
    auth: auth::AuthHost,
    external_import_auth: external_import_auth::ExternalImportAuthHost,
    artist_onboarding: artist_onboarding::Host,
    player: player::PlayerHost,
    output: Arc<crate::playback::output::OutputRuntime>,
    downloads: downloads::DownloadHost,
    cold_offline: Arc<cold_offline::Host>,
    cold_limiter: Option<Arc<crate::playback::engine::ColdLimiterConfig>>,
    cloud: cloud::CloudHost,
    mv: mv::MvHost,
    focus: Arc<focus::MediaFocus>,
    local: local::LocalHost,
    together: together::TogetherHost,
    scene_fm: scene_fm::SceneFmHost,
    playlist_cover: playlist_cover::CoverHost,
    account_avatar: account_avatar::AvatarHost,
    effect_material_backend: Option<Arc<dyn crate::download::transfer::MediaBackend>>,
    login_device_rows: login_devices::DeviceRows,
    search: search::experiment::SearchHost,
    podcast_discovery: podcast::PodcastDiscoveryHost,
    podcast_recent: podcast::PodcastRecentHost,
    audiobook_skip: podcast::AudiobookSkipHost,
    community: community::CommunityHost,
    community_topics: community::topic::TopicHost,
    profile_notes: community::CommunityHost,
    published_notes: community::CommunityHost,
    playlist_square: playlist_square::PlaylistSquareHost,
    playlist_essential: playlist_essential::PlaylistEssentialHost,
    profile_relations: profile_relations::ProfileRelationHost,
    profile_report: report_receipt::ReportHost,
    comment_report: report_receipt::ReportHost,
    community_report: report_receipt::ReportHost,
    profile_rank_removal: profile_play_record::RemovalHost,
    message_deletion: messages::DeletionHost,
    message_reply: messages::ReplyHost,
    message_poll: messages::PcMessagePollHost,
    member_offer: member::MemberOfferHost,
    follow_latest: catalog::FollowLatestHost,
    daily_recommendations: Arc<std::sync::Mutex<recommendations::RecommendationSnapshot>>,
    style_home: style_home::StyleHome,
    seeded_style_recommendations: Arc<std::sync::Mutex<recommendations::RecommendationSnapshot>>,
    style_recommendations: Arc<std::sync::Mutex<recommendations::RecommendationSnapshot>>,
    pc_experiments: pc_experiments::PcExperimentHost,
    vertical_zone: vertical_zone::ZoneHost,
    vertical_zone_detail: vertical_zone::detail::DetailHost,
    quality_experiment: quality_experiment::QualityExperiment,
    daily_experiment: daily_experiment::DailyExperiment,
    latest_releases: catalog::LatestReleasesHost,
}

impl MusicClient {
    /// `existing_profile` is an explicit native host option for an already
    /// authorized PC profile. Never search references or expand Cookie scope.
    pub fn open(data_dir: &Path, existing_profile: Option<&Path>) -> Result<Self> {
        Self::open_with_native_profile(data_dir, existing_profile, ClientProfile::default(), None)
    }

    /// Native dependency injection for startup/lifecycle integration checks.
    /// Production wire preparation and credential ownership remain unchanged.
    pub fn open_with_http_backend(
        data_dir: &Path,
        existing_profile: Option<&Path>,
        backend: Arc<dyn HttpBackend>,
    ) -> Result<Self> {
        Self::open_with_native_profile(
            data_dir,
            existing_profile,
            ClientProfile::default(),
            Some(backend),
        )
    }

    /// Explicit host identity for both the active transport and future login
    /// candidates. It never replaces credentials.device_id or broadens cookies.
    pub fn open_with_native_profile(
        data_dir: &Path,
        existing_profile: Option<&Path>,
        profile: ClientProfile,
        backend: Option<Arc<dyn HttpBackend>>,
    ) -> Result<Self> {
        Self::open_with_request_context(data_dir, existing_profile, profile, backend, None)
    }

    /// One installation's native PC policy and fresh token provider follow
    /// every isolated candidate; credentials remain in each profile's store.
    pub fn open_with_request_context(
        data_dir: &Path,
        existing_profile: Option<&Path>,
        profile: ClientProfile,
        backend: Option<Arc<dyn HttpBackend>>,
        context: Option<Arc<RequestContext>>,
    ) -> Result<Self> {
        profile.validate()?;
        let registry = ProfileRegistry::open(data_dir, ApiOrigin::Pc)?;
        let credentials = if let Some(path) = existing_profile {
            if !path.is_dir() || !path.join("core.sqlite3").is_file() {
                return Err(crate::Error::InvalidInput(
                    "existing native profile is missing",
                ));
            }
            let store = Arc::new(StateStore::open(path)?);
            Arc::new(Credentials::open(store, ApiOrigin::Pc)?)
        } else if let Some(selected) = registry
            .current(profile.clone())?
            .filter(|selected| selected.selection.auto_login)
        {
            selected.credentials
        } else {
            let store = Arc::new(StateStore::open(data_dir.join("guest"))?);
            Arc::new(Credentials::open(store, ApiOrigin::Pc)?)
        };
        let backend = match backend {
            Some(value) => value,
            None => Arc::new(ReqwestBackend::new(std::time::Duration::from_secs(20))?),
        };
        // Promote only the native profile/context combination exercised in the
        // real official PC roundtrip. Other hosts retain their verified routes.
        let options = RequestOptions {
            routing: if context.is_some()
                && profile.user_agent == OFFICIAL_WINDOWS_USER_AGENT
                && profile
                    .client_sign
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
            {
                RoutingPolicy::OfficialPcPlaylists
            } else {
                RoutingPolicy::VerifiedCompatibility
            },
            ..Default::default()
        };
        let api: Arc<dyn Api> = Arc::new(MusicHttpClient::with_backend(
            credentials,
            profile.clone(),
            options,
            context.clone(),
            backend.clone(),
        )?);
        let mut client = Self::with_session(
            Arc::new(AccountSession::new(api, Some(registry.store()))),
            registry,
        )
        .with_login_providers(Some(backend), None)
        .with_relay_publication()
        .with_queue_recommendations();
        client.auth.profile = profile;
        client.auth.context = context;
        client.auth.options = options;
        Ok(client)
    }

    /// The desktop host enables native experiment lookups; IPC has no flag.
    pub fn with_official_quality_experiments(mut self) -> Self {
        self.quality_experiment.enabled = true;
        self
    }

    /// Explicit native-only cold offline capability; no desktop IPC exposes it.
    pub fn grant_cold_offline(&self) -> Result<crate::account::profiles::ColdOfflineGrant> {
        self.registry.grant_cold_offline(self.session.clone())
    }

    /// Native dependency injection for non-Tauri hosts and contract tests.
    pub fn with_session(session: Arc<AccountSession>, registry: Arc<ProfileRegistry>) -> Self {
        let focus = Arc::new(focus::MediaFocus::default());
        let output = crate::playback::output::OutputRuntime::new(registry.store());
        Self {
            external_import: Default::default(),
            external_import_sidebar: false,
            member_welfare: Default::default(),
            remote_skins: Default::default(),
            playlist_cover: playlist_cover::CoverHost::default(),
            artist_onboarding: Default::default(),
            account_avatar: account_avatar::AvatarHost::default(),
            effect_material_backend: None,
            login_device_rows: login_devices::DeviceRows::default(),
            search: search::experiment::SearchHost::default(),
            podcast_discovery: Default::default(),
            podcast_recent: Default::default(),
            audiobook_skip: Default::default(),
            playlist_square: Default::default(),
            community: Default::default(),
            community_topics: Default::default(),
            profile_notes: Default::default(),
            published_notes: Default::default(),
            playlist_essential: Default::default(),
            profile_relations: Default::default(),
            profile_report: Default::default(),
            comment_report: Default::default(),
            community_report: Default::default(),
            profile_rank_removal: Default::default(),
            message_deletion: Default::default(),
            message_reply: Default::default(),
            message_poll: Default::default(),
            member_offer: Default::default(),
            follow_latest: Default::default(),
            daily_recommendations: Default::default(),
            style_home: Default::default(),
            seeded_style_recommendations: Default::default(),
            style_recommendations: Default::default(),
            pc_experiments: Default::default(),
            vertical_zone: Default::default(),
            vertical_zone_detail: Default::default(),
            quality_experiment: quality_experiment::QualityExperiment::default(),
            daily_experiment: daily_experiment::DailyExperiment::default(),
            latest_releases: catalog::LatestReleasesHost::default(),
            together: together::TogetherHost::default(),
            scene_fm: scene_fm::SceneFmHost::default(),
            local: local::LocalHost::new(registry.store()),
            cloud: cloud::CloudHost::new(session.clone()),
            cold_offline: Arc::default(),
            cold_limiter: None,
            downloads: downloads::DownloadHost::new(session.clone(), registry.store()),
            player: player::PlayerHost::new(
                session.clone(),
                registry.store(),
                focus.clone(),
                output.clone(),
            ),
            mv: mv::MvHost::new(session.clone(), focus.clone(), output.clone()),
            output,
            focus,
            session,
            registry,
            auth: auth::AuthHost::default(),
            external_import_auth: Default::default(),
        }
    }
}

mod desktop_lyric_preferences;

pub mod desktop_lyrics;
