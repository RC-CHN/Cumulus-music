use super::{
    ClientProfile, HttpBackend, RequestInput, RequestOptions, ReqwestBackend, RoutingPolicy,
    WireRoute, build_request, select_route_with_policy,
};
use crate::{
    Error, Result,
    account::credentials::Credentials,
    protocol::{
        api::{Api, ApiFuture},
        context::RequestContext,
        wire::eapi,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
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
            registration_user: Mutex::new(None),
        })
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

    pub(crate) async fn request_jsk_if_revision(
        &self,
        path: &str,
        payload: Value,
        expected: &str,
    ) -> Result<Value> {
        self.execute(path, payload, false, Some(expected), Jsk::Lifecycle, None)
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
        let route = select_route_with_policy(
            origin,
            path,
            &payload,
            if jsk == Jsk::None {
                self.options.routing
            } else {
                RoutingPolicy::OfficialEapi
            },
        )?;
        // Async gate serializes cookie rotation for one candidate/profile;
        // no blocking mutex is held during network I/O.
        let _guard = self.gate.lock().await;
        if path == "/api/w/register/cellphone" {
            *self
                .registration_user
                .lock()
                .map_err(|_| Error::LockPoisoned)? = None;
        }
        let target = origin.request_url(route, path)?;
        let snapshot = self.credentials.request_credentials(&target)?;
        if expected.is_some_and(|revision| revision != snapshot.revision) {
            return Err(Error::CredentialChanged);
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
            let response = wire.backend.post(plan).await?;
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
        if jsk != Jsk::QuickEligibility
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

impl Api for MusicHttpClient {
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
