use super::{
    ClientProfile, HttpBackend, RequestInput, RequestOptions, ReqwestBackend, RoutingPolicy,
    WireRoute, build_request, select_route_with_policy,
};
use crate::{
    Error, Result,
    account::credentials::Credentials,
    protocol::{
        api::{Api, ApiFuture, VinylCommentRead, VinylCommentWrite, VinylCommentWritePermission},
        context::RequestContext,
        wire::eapi,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone)]
pub struct ResponseInfo {
    pub status: u16,
    pub bytes: usize,
    pub decrypted: bool,
    pub metadata: BTreeMap<String, String>,
}

pub struct MusicHttpClient {
    credentials: Arc<Credentials>,
    profile: ClientProfile,
    options: RequestOptions,
    context: Option<Arc<RequestContext>>,
    backend: Arc<dyn HttpBackend>,
    gate: tokio::sync::Mutex<()>,
    last_response: Mutex<Option<ResponseInfo>>,
    server_clock: Mutex<Option<(u64, Instant)>>,
    registration_user: Mutex<Option<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Jsk {
    None,
    Lifecycle,
    Login,
    QuickEligibility,
    GuestQuickExchange,
    ActiveSwitchExchange,
}

impl Jsk {
    fn quick_exchange(self) -> bool {
        matches!(self, Self::GuestQuickExchange | Self::ActiveSwitchExchange)
    }
}

#[derive(Clone, Copy)]
enum RequestBinding<'a> {
    Login(&'a super::JskLoginContext),
    VinylComment,
    VinylCommentWrite {
        permission: &'a VinylCommentWritePermission<'a>,
        dispatched: &'a AtomicBool,
    },
    PodcastProgress(&'a crate::protocol::api::PodcastProgressPermission),
    LegacyRenewal {
        permission: &'a (dyn Fn() -> Result<()> + Sync),
        dispatched: &'a AtomicBool,
    },
    Switch {
        source: &'a MusicHttpClient,
        revision: &'a str,
    },
}

impl MusicHttpClient {
    pub(crate) fn credentials(&self) -> Arc<Credentials> {
        self.credentials.clone()
    }
    pub fn new(credentials: Arc<Credentials>, profile: ClientProfile) -> Result<Self> {
        Self::with_backend(
            credentials,
            profile,
            RequestOptions::default(),
            None,
            Arc::new(ReqwestBackend::new(Duration::from_secs(20))?),
        )
    }

    pub fn with_backend(
        credentials: Arc<Credentials>,
        profile: ClientProfile,
        options: RequestOptions,
        context: Option<Arc<RequestContext>>,
        backend: Arc<dyn HttpBackend>,
    ) -> Result<Self> {
        profile.validate()?;
        if context
            .as_ref()
            .is_some_and(|c| c.api_host() != credentials.origin().as_str())
        {
            return Err(Error::InvalidInput("request context origin mismatch"));
        }
        Ok(Self {
            credentials,
            profile,
            options,
            context,
            backend,
            gate: tokio::sync::Mutex::new(()),
            last_response: Mutex::new(None),
            server_clock: Mutex::new(None),
            registration_user: Mutex::new(None),
        })
    }

    fn observe_server_date(&self, header: Option<&str>) {
        use time::{OffsetDateTime, format_description::well_known::Rfc2822};

        // M47 ignores dates before this cutoff. Advance the latest accepted
        // header with a monotonic clock rather than depending on wall-clock edits.
        let Some(server_ms) = header
            .filter(|value| value.len() <= 64)
            .and_then(|value| OffsetDateTime::parse(value, &Rfc2822).ok())
            .and_then(|date| u64::try_from(date.unix_timestamp_nanos() / 1_000_000).ok())
            .filter(|ms| *ms > 1_740_000_000_000)
        else {
            return;
        };
        let anchor = Instant::now();
        if let Ok(mut clock) = self.server_clock.lock() {
            let floor = clock.map_or(0, |(ms, at)| advance_server_clock(ms, at, anchor));
            *clock = Some((server_ms.max(floor), anchor));
        }
    }

    pub fn response_info(&self) -> Result<Option<ResponseInfo>> {
        Ok(self
            .last_response
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .clone())
    }

    /// Candidate-local proof that the force-register response actually issued
    /// the effective authorization Cookie. No header value leaves native code.
    pub(crate) fn registration_authorized(&self) -> Result<bool> {
        let issued = self
            .registration_user
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .clone();
        Ok(issued.is_some() && self.credentials.login_credentials()?.token == issued)
    }

    pub(crate) fn pc_experiment_revision(&self) -> Result<String> {
        self.pc_experiment_revision_for(true)
    }

    fn pc_experiment_revision_for(&self, require_account: bool) -> Result<String> {
        use crate::protocol::pc_experiments::ASSIGNMENT_PATH;
        if self.credentials.origin() != super::ApiOrigin::Pc
            || self.profile.user_agent != super::OFFICIAL_WINDOWS_USER_AGENT
            || self
                .profile
                .client_sign
                .as_deref()
                .is_none_or(str::is_empty)
            || self
                .context
                .as_ref()
                .and_then(|context| context.pc_snapshot().ok().flatten())
                .is_none()
        {
            return Err(Error::State("signed PC experiment request is unavailable"));
        }
        let target = super::ApiOrigin::Pc.request_url(WireRoute::Eapi, ASSIGNMENT_PATH)?;
        let credentials = self.credentials.request_credentials(&target)?;
        if credentials.device_id.is_empty()
            || (require_account && credentials.unique_music_u().is_none())
        {
            return Err(Error::State(
                "signed PC experiment requires its original PC credentials",
            ));
        }
        Ok(credentials.revision)
    }

    pub(crate) async fn pc_experiment_groups(
        &self,
        revision: &str,
    ) -> Result<crate::protocol::pc_experiments::PcExperimentGroups> {
        self.pc_experiment_groups_for(revision, true).await
    }

    async fn pc_experiment_groups_for(
        &self,
        revision: &str,
        require_account: bool,
    ) -> Result<crate::protocol::pc_experiments::PcExperimentGroups> {
        use crate::protocol::pc_experiments::{
            ASSIGNMENT_PATH, PcExperimentGroups, assignment_arguments,
        };
        if self.pc_experiment_revision_for(require_account)? != revision {
            return Err(Error::CredentialChanged);
        }
        let response = self
            .execute(
                ASSIGNMENT_PATH,
                assignment_arguments(),
                false,
                Some(revision),
                Jsk::None,
                None,
            )
            .await?;
        let response = crate::protocol::api::validate_response(ASSIGNMENT_PATH, response, &[200])?;
        PcExperimentGroups::refreshed(&response)
    }

    pub(crate) async fn request_jsk_if_revision(
        &self,
        path: &str,
        payload: Value,
        expected: &str,
    ) -> Result<Value> {
        self.execute(path, payload, false, Some(expected), Jsk::Lifecycle, None)
            .await
    }

    pub(crate) async fn request_legacy_periodic_refresh(
        &self,
        revision: &str,
        permission: &(dyn Fn() -> Result<()> + Sync),
        dispatched: &AtomicBool,
    ) -> Result<Value> {
        self.execute(
            crate::account::refresh::LEGACY_PERIODIC_PATH,
            json!({}),
            false,
            Some(revision),
            Jsk::None,
            Some(RequestBinding::LegacyRenewal {
                permission,
                dispatched,
            }),
        )
        .await
    }

    pub(crate) async fn request_jsk(&self, path: &str, payload: Value) -> Result<Value> {
        self.execute(path, payload, false, None, Jsk::Lifecycle, None)
            .await
    }

    pub(crate) async fn request_jsk_login(
        &self,
        path: &str,
        payload: Value,
        context: &super::JskLoginContext,
    ) -> Result<Value> {
        if self.credentials.origin() != super::ApiOrigin::Pc {
            return Err(Error::InvalidInput("JSK login requires the PC origin"));
        }
        self.execute(
            path,
            payload,
            false,
            None,
            Jsk::Login,
            Some(RequestBinding::Login(context)),
        )
        .await
    }

    pub(crate) async fn request_jsk_quick_eligibility(
        &self,
        payload: Value,
        expected_revision: &str,
    ) -> Result<Value> {
        if self.credentials.origin() != super::ApiOrigin::Pc {
            return Err(Error::InvalidInput(
                "JSK quick login requires the PC origin",
            ));
        }
        self.execute(
            "/api/login/getquickloginlist",
            payload,
            false,
            Some(expected_revision),
            Jsk::QuickEligibility,
            None,
        )
        .await
    }

    pub(crate) async fn request_jsk_guest_quick(&self, payload: Value) -> Result<Value> {
        if self.credentials.origin() != super::ApiOrigin::Pc {
            return Err(Error::InvalidInput(
                "JSK quick login requires the PC origin",
            ));
        }
        let before = self.credentials.login_credentials()?;
        if before.token.is_some() || before.refresh_token.is_some() {
            return Err(Error::State(
                "server quick login requires a fresh candidate",
            ));
        }
        self.execute(
            "/api/login/quicklogin",
            payload,
            false,
            Some(&before.revision),
            Jsk::GuestQuickExchange,
            None,
        )
        .await
    }

    pub(crate) async fn request_jsk_active_switch(
        &self,
        source: &Self,
        revision: &str,
        payload: Value,
    ) -> Result<Value> {
        if std::ptr::eq(self, source)
            || self.credentials.origin() != super::ApiOrigin::Pc
            || source.credentials.origin() != super::ApiOrigin::Pc
        {
            return Err(Error::InvalidInput(
                "JSK switch requires separate PC source and candidate",
            ));
        }
        let candidate = self.credentials.login_credentials()?;
        if candidate.token.is_some() || candidate.refresh_token.is_some() {
            return Err(Error::State("server switch requires a fresh candidate"));
        }
        self.execute(
            "/api/login/switchuser",
            payload,
            false,
            Some(&candidate.revision),
            Jsk::ActiveSwitchExchange,
            Some(RequestBinding::Switch { source, revision }),
        )
        .await
    }

    async fn execute(
        &self,
        path: &str,
        mut payload: Value,
        cache: bool,
        expected: Option<&str>,
        jsk: Jsk,
        binding: Option<RequestBinding<'_>>,
    ) -> Result<Value> {
        if jsk != Jsk::None {
            let allowed = match jsk {
                Jsk::Login => super::login::jsk_login_path(path),
                Jsk::Lifecycle => matches!(
                    path,
                    "/api/login/token/refresh/v1" | "/api/middle/account/token/refresh"
                ),
                Jsk::QuickEligibility => path == "/api/login/getquickloginlist",
                Jsk::GuestQuickExchange => path == "/api/login/quicklogin",
                Jsk::ActiveSwitchExchange => path == "/api/login/switchuser",
                Jsk::None => false,
            };
            if !allowed {
                return Err(Error::InvalidInput("unsupported JSK lifecycle endpoint"));
            }
            payload
                .as_object_mut()
                .ok_or(Error::InvalidInput("JSK payload must be object"))?
                .insert("fromPage".into(), json!("JSK"));
        }
        let origin = self.credentials.origin();
        let legacy_renewal = matches!(binding, Some(RequestBinding::LegacyRenewal { .. }));
        let route = select_route_with_policy(
            origin,
            path,
            &payload,
            if jsk == Jsk::None && !legacy_renewal {
                self.options.routing
            } else {
                RoutingPolicy::OfficialEapi
            },
        )?;
        // Async gate serializes cookie rotation for one candidate/profile;
        // no blocking mutex is held during network I/O.
        let _guard = self.gate.lock().await;
        if let Some(RequestBinding::PodcastProgress(allowed)) = binding
            && !allowed()?
        {
            return Err(Error::Cancelled);
        }
        if path == "/api/w/register/cellphone" {
            *self
                .registration_user
                .lock()
                .map_err(|_| Error::LockPoisoned)? = None;
        }
        let target = origin.request_url(route, path)?;
        let snapshot = self.credentials.request_credentials(&target)?;
        if expected.is_some_and(|revision| revision != snapshot.revision)
            || (matches!(binding, Some(RequestBinding::PodcastProgress(_)))
                && snapshot.unique_music_u().is_none())
            || (legacy_renewal && snapshot.unique_music_u().is_none())
        {
            return Err(Error::CredentialChanged);
        }
        if matches!(
            binding,
            Some(RequestBinding::VinylComment | RequestBinding::VinylCommentWrite { .. })
        ) {
            // A single revision covers all URL scopes. The same revision
            // alone does not prove both endpoints see the same MUSIC_U.
            let assignment_target = super::ApiOrigin::Pc.request_url(
                WireRoute::Eapi,
                crate::protocol::pc_experiments::ASSIGNMENT_PATH,
            )?;
            let assignment = self.credentials.request_credentials(&assignment_target)?;
            if origin != super::ApiOrigin::Pc
                || route != WireRoute::Eapi
                || assignment.revision != snapshot.revision
                || assignment.device_id != snapshot.device_id
                || snapshot.unique_music_u().is_none()
                || assignment.unique_music_u() != snapshot.unique_music_u()
            {
                return Err(Error::CredentialChanged);
            }
        }
        // M402 sends same-origin request credentials. An active switch borrows
        // the source's scoped wire snapshot, never its mutable credential sink.
        let (wire, source_snapshot) =
            if let Some(RequestBinding::Switch { source, revision }) = binding {
                let scoped = source.credentials.request_credentials(&target)?;
                if scoped.revision != revision
                    || scoped.device_id != snapshot.device_id
                    || scoped.unique_music_u().is_none()
                {
                    return Err(Error::CredentialChanged);
                }
                (source, Some(scoped))
            } else {
                (self, None)
            };
        let mut extra_headers = BTreeMap::new();
        if origin == super::ApiOrigin::Pc
            && wire.options.routing == RoutingPolicy::OfficialPcPlaylists
            && super::origin::official_pc_playlist(path, &payload)
        {
            let prepared = async {
                wire.context
                    .as_ref()
                    .ok_or(Error::State("official PC playlist requires native context"))?
                    .prepare_configured(path, &payload)
                    .await
            }
            .await
            .map_err(|error| Error::RequestNotSent(Box::new(error)))?;
            payload = prepared.payload;
            extra_headers = prepared.headers;
        } else if route == WireRoute::Eapi
            && let Some(context) = &wire.context
        {
            let prepared = context.prepare(path, &payload).await.map_err(|error| {
                if expected.is_some()
                    || jsk == Jsk::Login
                    || super::login::authentication_write(path)
                {
                    Error::RequestNotSent(Box::new(error))
                } else {
                    error
                }
            })?;
            payload = prepared.payload;
            extra_headers = prepared.headers;
        } else if let Some(context) = &wire.context {
            extra_headers = context.metadata_headers()?;
        }
        if self.credentials.request_credentials(&target)?.revision != snapshot.revision {
            return Err(Error::CredentialChanged);
        }
        if jsk != Jsk::None {
            // M/1368 PcPlatformAdapter.fetch -> M/290 nativeRequest passes this
            // header through M/402. M/290 getCurrentEnv supplies no chain ID;
            // preserve the empty value instead of fabricating a native chain.
            extra_headers.insert("x-login-chain-id".into(), String::new());
            if let Some(RequestBinding::Login(context)) = binding
                && let Some(ticket) = context.ticket()?
            {
                extra_headers.insert("x-music-tu".into(), ticket);
            }
        }
        // M/92 pre-request middleware runs after M/402 EAPI serialization;
        // automatic MConfig negotiation is an HTTP header, not encrypted data.
        let mconfig_header = extra_headers.remove("MConfig-Info");
        let mut plan = build_request(RequestInput {
            origin,
            path,
            payload,
            credentials: source_snapshot.as_ref().unwrap_or(&snapshot),
            profile: &wire.profile,
            extra_headers: &extra_headers,
            options: RequestOptions {
                cache,
                // Freeze the selected wire before token injection adds fields
                // to a metadata batch. No later payload/header can reroute it.
                routing: if route == WireRoute::Eapi {
                    RoutingPolicy::OfficialEapi
                } else {
                    RoutingPolicy::VerifiedCompatibility
                },
                ..wire.options
            },
        })?;
        if let Some(value) = mconfig_header {
            plan.headers.insert("MConfig-Info".into(), value);
        }
        let response_url = plan.url.clone();
        let encrypted_response = plan.decrypt_response;
        let check_wire_source = || -> Result<()> {
            if let Some(source) = &source_snapshot {
                let current = wire.credentials.request_credentials(&target)?;
                if current.revision != source.revision
                    || current.unique_music_u() != source.unique_music_u()
                {
                    return Err(Error::CredentialChanged);
                }
            }
            Ok(())
        };
        let response = {
            // The candidate gate above is always taken first. The separate
            // source gate fences ordinary source HTTP rotations during
            // dispatch; take it after token preparation, which may await a
            // provider. Neither gate leases another process's store writes.
            let _source_gate = if source_snapshot.is_some() {
                Some(wire.gate.lock().await)
            } else {
                None
            };
            check_wire_source()?;
            if let Some(RequestBinding::PodcastProgress(allowed)) = binding
                && !allowed()?
            {
                return Err(Error::Cancelled);
            }
            if let Some(RequestBinding::VinylCommentWrite {
                permission,
                dispatched,
            }) = binding
            {
                permission()?;
                // No await or fallible work follows before the backend call.
                dispatched.store(true, Ordering::Relaxed);
            }
            if let Some(RequestBinding::LegacyRenewal {
                permission,
                dispatched,
            }) = binding
            {
                permission()?;
                dispatched.store(true, Ordering::Relaxed);
            }
            let response = wire.backend.post(plan).await?;
            if let Some(RequestBinding::LegacyRenewal { permission, .. }) = binding {
                permission()?;
            }
            check_wire_source()?;
            response
        };
        if (300..400).contains(&response.status) {
            return Err(Error::Transport("API redirect rejected".into()));
        }
        let mut decrypted = false;
        let parsed: Result<Value> = if response.body.len() > super::backend::MAX_RESPONSE_BYTES
            || response.body_status == super::BodyStatus::TooLarge
        {
            Err(Error::Response("API response exceeds size limit"))
        } else if response.body_status == super::BodyStatus::Interrupted {
            Err(Error::Transport("response body transfer failed".into()))
        } else if response.body.is_empty() && route == WireRoute::Weapi {
            Ok(json!({"http_status":response.status,"empty_body":true}))
        } else {
            match serde_json::from_slice(&response.body) {
                Ok(value) => Ok(value),
                Err(_) if encrypted_response => {
                    eapi::decrypt_response(&response.body).and_then(|plain| {
                        let value = serde_json::from_slice(&plain)?;
                        decrypted = true;
                        Ok(value)
                    })
                }
                Err(_) => Err(Error::Response("API returned non-JSON data")),
            }
        };
        let login_path =
            jsk.quick_exchange() || super::login::completes_login(path, &response.set_cookies);
        let authorized = parsed.as_ref().is_ok_and(|v| {
            (if jsk.quick_exchange() {
                crate::error::code_matches(&v["code"], 200)
            } else {
                [Some(200), Some(803)].contains(&v["code"].as_i64())
            }) && v["data"]["success"] != false
                && v["data"] != false
                && (path != "/api/middle/account/uplinkcaptcha/verify"
                    || crate::protocol::wire::serialization::js_truthy(&v["data"]))
        });
        let acknowledged = (200..300).contains(&response.status) && login_path && authorized;
        let refresh = if acknowledged {
            // The SDK treats an empty header as absent (`value || ""`).
            response.refresh_token.as_deref().filter(|s| !s.is_empty())
        } else {
            None
        };
        // Even an unreadable body may have issued a valid Cookie. Persist it
        // for explicit account reconciliation, without replaying credentials.
        // M/1368 prepareAndSaveLoginInfo supplies Date.now() for a completed
        // JSK login. Do not reset this on resource reads, legacy requests or
        // refresh itself. Commit it atomically with the candidate credentials.
        let last_login_at = (acknowledged && (jsk == Jsk::Login || jsk.quick_exchange()))
            .then(|| SystemTime::now().duration_since(UNIX_EPOCH).ok())
            .flatten()
            .map(|time| time.as_secs());
        let legacy_acknowledged = legacy_renewal
            && (200..300).contains(&response.status)
            && parsed
                .as_ref()
                .is_ok_and(|body| crate::error::code_matches(&body["code"], 200));
        if legacy_renewal {
            if legacy_acknowledged {
                self.credentials.record_legacy_renewal_response(
                    &response_url,
                    &response.set_cookies,
                    &snapshot.revision,
                )?;
            }
        } else if jsk != Jsk::QuickEligibility
            && (!jsk.quick_exchange() || (200..300).contains(&response.status))
        {
            self.credentials.record_login_response(
                &response_url,
                &response.set_cookies,
                refresh,
                &snapshot.revision,
                last_login_at,
            )?;
        }
        if path == "/api/w/register/cellphone" && (200..300).contains(&response.status) {
            let effective = self.credentials.login_credentials()?.token;
            let target = origin.request_url(WireRoute::Eapi, "/api/login/token/refresh/v1")?;
            let issued = response.set_cookies.iter().any(|value| {
                cookie_store::Cookie::parse(value.as_str(), &response_url).is_ok_and(|cookie| {
                    cookie.name() == "MUSIC_U"
                        && !cookie.value().is_empty()
                        && !cookie.is_expired()
                        && cookie.matches(&target)
                        && effective.as_deref() == Some(cookie.value())
                })
            });
            if issued {
                *self
                    .registration_user
                    .lock()
                    .map_err(|_| Error::LockPoisoned)? = effective;
            }
        }
        if (200..300).contains(&response.status)
            && super::login::can_emit_verification(path)
            && parsed
                .as_ref()
                .is_ok_and(|value| matches!(value["code"].as_i64(), Some(8830 | 8860 | 8831)))
            && let Some(RequestBinding::Login(context)) = binding
        {
            context.observe_challenge(&response.set_cookies)?;
        }
        if let Some(context) = &self.context {
            context.observe_response(response.metadata.clone());
        }
        if (200..300).contains(&response.status) {
            self.observe_server_date(response.metadata.get("date").map(String::as_str));
        }
        *self.last_response.lock().map_err(|_| Error::LockPoisoned)? = Some(ResponseInfo {
            status: response.status,
            bytes: response.body.len(),
            decrypted,
            metadata: response.metadata,
        });
        if !(200..300).contains(&response.status) {
            return Err(Error::HttpStatus {
                endpoint: Some(path.into()),
                status: response.status,
                body: parsed.ok().map(Box::new),
            });
        }
        parsed
    }
}

fn advance_server_clock(ms: u64, at: Instant, now: Instant) -> u64 {
    ms.saturating_add(
        u64::try_from(now.saturating_duration_since(at).as_millis()).unwrap_or(u64::MAX),
    )
}

impl Api for MusicHttpClient {
    fn podcast_progress_revision(&self) -> Result<Option<String>> {
        use crate::podcast::progress::PLAYRECORD_PATH;
        let origin = self.credentials.origin();
        let route =
            select_route_with_policy(origin, PLAYRECORD_PATH, &json!({}), self.options.routing)?;
        let target = origin.request_url(route, PLAYRECORD_PATH)?;
        let snapshot = self.credentials.request_credentials(&target)?;
        Ok(snapshot.unique_music_u().map(|_| snapshot.revision.clone()))
    }

    fn upload_podcast_progress<'a>(
        &'a self,
        payload: Value,
        revision: &'a str,
        allowed: &'a crate::protocol::api::PodcastProgressPermission,
    ) -> ApiFuture<'a, ()> {
        use crate::podcast::progress::PLAYRECORD_PATH;
        Box::pin(async move {
            let response = self
                .execute(
                    PLAYRECORD_PATH,
                    payload,
                    false,
                    Some(revision),
                    Jsk::None,
                    Some(RequestBinding::PodcastProgress(allowed)),
                )
                .await?;
            crate::protocol::api::validate_response(PLAYRECORD_PATH, response, &[200])?;
            Ok(())
        })
    }

    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        self.pc_experiment_revision().map(Some)
    }

    fn verified_pc_experiment_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> crate::protocol::api::ApiFuture<'a, crate::protocol::pc_experiments::PcExperimentGroups>
    {
        Box::pin(async move { self.pc_experiment_groups(revision).await })
    }

    fn request_vinyl_comment_page<'a>(
        &'a self,
        read: VinylCommentRead,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.pc_experiment_revision()? != revision {
                return Err(Error::CredentialChanged);
            }
            let (path, payload) = read.into_request();
            self.execute(
                &path,
                payload,
                false,
                Some(revision),
                Jsk::None,
                Some(RequestBinding::VinylComment),
            )
            .await
        })
    }

    fn request_vinyl_comment_write<'a>(
        &'a self,
        write: VinylCommentWrite,
        revision: &'a str,
        permission: &'a VinylCommentWritePermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let dispatched = AtomicBool::new(false);
            let result = async {
                if self.pc_experiment_revision()? != revision {
                    return Err(Error::CredentialChanged);
                }
                let (path, payload) = write.into_request();
                self.execute(
                    path,
                    payload,
                    false,
                    Some(revision),
                    Jsk::None,
                    Some(RequestBinding::VinylCommentWrite {
                        permission,
                        dispatched: &dispatched,
                    }),
                )
                .await
            }
            .await;
            match result {
                Err(error) if !dispatched.load(Ordering::Relaxed) => {
                    if matches!(error.root(), Error::RequestNotSent(_)) {
                        Err(error)
                    } else {
                        Err(Error::RequestNotSent(Box::new(error)))
                    }
                }
                Err(error) if matches!(error.root(), Error::RequestNotSent(_)) => {
                    // A backend result cannot prove this scoped intent never
                    // crossed our dispatch boundary.
                    Err(Error::MutationUncertain)
                }
                other => other,
            }
        })
    }

    fn search_audio_toplist_assignment(&self) -> crate::protocol::api::ApiFuture<'_, bool> {
        Box::pin(async {
            let revision = self.pc_experiment_revision()?;
            let groups = self.pc_experiment_groups(&revision).await?;
            if self.pc_experiment_revision()? != revision {
                return Err(Error::CredentialChanged);
            }
            Ok(groups.search_audio_toplist_assigned())
        })
    }

    fn membership_credential_revision(&self) -> Result<Option<String>> {
        Ok(Some(self.credentials.login_credentials()?.revision))
    }

    fn search_privilege_credential_revision(&self) -> Result<Option<String>> {
        Ok(Some(self.credentials.login_credentials()?.revision))
    }

    fn search_default_context_revision(&self) -> Result<Option<String>> {
        Ok(Some(self.credentials.login_credentials()?.revision))
    }

    fn request_search_with_default_context<'a>(
        &'a self,
        path: &'a str,
        payload: Value,
        revision: &'a str,
    ) -> crate::protocol::api::ApiFuture<'a, Value> {
        Box::pin(async move {
            if !matches!(
                path,
                "/api/search/pc/complex/page" | "/api/search/pc/complex/page/v3"
            ) || payload["channel"] != "defaultquery"
            {
                return Err(Error::InvalidInput("invalid contextual search request"));
            }
            self.execute(path, payload, false, Some(revision), Jsk::None, None)
                .await
        })
    }

    fn playback_power_experiment(
        &self,
    ) -> crate::protocol::api::ApiFuture<
        '_,
        Option<crate::protocol::pc_experiments::PcExperimentGroups>,
    > {
        Box::pin(async {
            let revision = self.pc_experiment_revision_for(false)?;
            let groups = self.pc_experiment_groups_for(&revision, false).await?;
            if self.pc_experiment_revision_for(false)? != revision {
                return Err(Error::CredentialChanged);
            }
            Ok(Some(groups))
        })
    }

    fn queue_more_experiment_groups(
        &self,
    ) -> crate::protocol::api::ApiFuture<'_, crate::protocol::pc_experiments::PcExperimentGroups>
    {
        Box::pin(async {
            let revision = self.pc_experiment_revision()?;
            self.pc_experiment_groups(&revision).await
        })
    }

    fn observed_server_time_ms(&self) -> Option<u64> {
        self.server_clock
            .lock()
            .ok()?
            .map(|(ms, at)| advance_server_clock(ms, at, Instant::now()))
    }

    fn local_device_id(&self) -> Result<Option<String>> {
        self.credentials.device_id().map(Some)
    }
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.execute(path, payload, cache, None, Jsk::None, None)
                .await
        })
    }
}
