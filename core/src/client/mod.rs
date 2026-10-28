//! Host-independent UI use cases. Native records and credentials never cross
//! this boundary; account epochs also reject delayed frontend results.
mod auth;
pub use auth::LoginCaptchaProvider;
mod catalog;
mod cloud;
mod collections;
mod comments;
mod content;
mod downloads;
pub mod dto;
mod error;
mod fm;
mod fm_discovery;
mod focus;
mod history;
mod local;
mod mapping;
mod member;
mod mv;
mod player;
mod playlist_cover;
mod playlists;
mod podcast;
mod rankings;
mod relay;
mod scene_fm;
mod settings;
mod together;
pub use player::PlaybackSubscription;

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
    session: Arc<AccountSession>,
    registry: Arc<ProfileRegistry>,
    auth: auth::AuthHost,
    player: player::PlayerHost,
    downloads: downloads::DownloadHost,
    cloud: cloud::CloudHost,
    mv: mv::MvHost,
    focus: Arc<focus::MediaFocus>,
    local: local::LocalHost,
    together: together::TogetherHost,
    scene_fm: scene_fm::SceneFmHost,
    playlist_cover: playlist_cover::CoverHost,
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
        .with_relay_publication();
        client.auth.profile = profile;
        client.auth.context = context;
        client.auth.options = options;
        Ok(client)
    }

    /// Native dependency injection for non-Tauri hosts and contract tests.
    pub fn with_session(session: Arc<AccountSession>, registry: Arc<ProfileRegistry>) -> Self {
        let focus = Arc::new(focus::MediaFocus::default());
        Self {
            playlist_cover: playlist_cover::CoverHost::default(),
            together: together::TogetherHost::default(),
            scene_fm: scene_fm::SceneFmHost::default(),
            local: local::LocalHost::new(registry.store()),
            cloud: cloud::CloudHost::new(session.clone()),
            downloads: downloads::DownloadHost::new(session.clone(), registry.store()),
            player: player::PlayerHost::new(session.clone(), registry.store(), focus.clone()),
            mv: mv::MvHost::new(session.clone(), focus.clone()),
            focus,
            session,
            registry,
            auth: auth::AuthHost::default(),
        }
    }
}
