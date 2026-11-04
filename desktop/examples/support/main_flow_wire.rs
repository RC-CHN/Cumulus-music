//! Closed EAPI transport for the synthetic main-flow QR and business requests.
use super::{artist_onboarding_fixture, fixture::Service};
use cumulus_core::{
    Error, Result,
    protocol::{
        api::{Api, ApiFuture},
        context::{RequestContext, TokenProvider},
        http::{ApiOrigin, BodyStatus, HttpBackend, HttpReply, RequestPlan, WireRoute},
        wire::eapi,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;

const ACCOUNT: &str = "/api/w/nuser/account/get";
const TICKET: &str = "/api/login/qrcode/unikey";
const POLL: &str = "/api/login/qrcode/client/login";
const QR_KEY: &str = "cumulus-synthetic-main-flow-qr-test-only";
const COOKIE: &str = "cumulus-synthetic-main-flow-only";

#[derive(Default)]
struct Progress {
    guest_identity_reads: u64,
    tickets: u64,
    polls: u64,
    approved: bool,
    authorized: bool,
    authenticated_identity_reads: u64,
    offline_relay_experiment_attempts: u64,
    offline_business_attempts: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Qr,
    Reopen,
}
impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Qr => "qr",
            Self::Reopen => "reopen",
        }
    }
}

pub struct Wire {
    mode: Mode,
    service: Arc<Service>,
    progress: Mutex<Progress>,
    confirmation: Notify,
}

struct NoToken;
impl TokenProvider for NoToken {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async { Err(Error::State("no token provider in synthetic main flow")) })
    }
}

impl Wire {
    pub fn new(service: Arc<Service>) -> Self {
        Self::with_mode(service, Mode::Qr)
    }

    pub fn reopen(service: Arc<Service>) -> Self {
        Self::with_mode(service, Mode::Reopen)
    }

    fn with_mode(service: Arc<Service>, mode: Mode) -> Self {
        Self {
            mode,
            service,
            progress: Mutex::new(Progress::default()),
            confirmation: Notify::new(),
        }
    }

    pub fn context() -> Arc<RequestContext> {
        Arc::new(RequestContext::new(
            json!({}),
            ApiOrigin::Pc.as_str().into(),
            Arc::new(NoToken),
        ))
    }

    pub fn approve(&self) -> Result<()> {
        if self.mode != Mode::Qr {
            return Err(Error::State("synthetic reopen does not allow QR approval"));
        }
        let mut progress = self.progress.lock().map_err(|_| Error::LockPoisoned)?;
        if progress.tickets != 1 || !(1..=2).contains(&progress.polls) || progress.approved {
            return Err(Error::State("synthetic QR is not awaiting approval"));
        }
        progress.approved = true;
        self.confirmation.notify_one();
        Ok(())
    }

    pub fn facts(&self) -> Value {
        let progress = self.progress.lock().expect("synthetic QR state");
        json!({
            "mode": self.mode.label(),
            "guestIdentityReads": progress.guest_identity_reads,
            "tickets": progress.tickets,
            "polls": progress.polls,
            "approved": progress.approved,
            "authorized": progress.authorized,
            "authenticatedIdentityReads": progress.authenticated_identity_reads,
            "offlineRelayExperimentAttempts": progress.offline_relay_experiment_attempts,
            "offlineBusinessAttempts": progress.offline_business_attempts,
        })
    }

    async fn qr_poll(&self, data: &Value) -> Result<HttpReply> {
        if data["type"] != "3" || data["key"] != QR_KEY {
            return Err(Error::InvalidInput("invalid synthetic QR poll"));
        }
        let approved = {
            let mut progress = self.progress.lock().map_err(|_| Error::LockPoisoned)?;
            if progress.tickets != 1 || progress.polls >= 2 {
                return Err(Error::State("unexpected synthetic QR poll"));
            }
            progress.polls += 1;
            if progress.polls == 1 {
                return reply(json!({"code": 802}), false);
            }
            progress.approved
        };
        if !approved {
            tokio::time::timeout(Duration::from_secs(12), self.confirmation.notified())
                .await
                .map_err(|_| Error::State("synthetic QR approval did not arrive"))?;
        }
        let mut progress = self.progress.lock().map_err(|_| Error::LockPoisoned)?;
        if !progress.approved || progress.authorized {
            return Err(Error::State("unexpected synthetic QR authorization"));
        }
        progress.authorized = true;
        reply(json!({"code": 803}), true)
    }
}

fn reply(body: Value, issue_cookie: bool) -> Result<HttpReply> {
    Ok(HttpReply {
        status: 200,
        body: serde_json::to_vec(&body)?,
        body_status: BodyStatus::Complete,
        set_cookies: issue_cookie
            .then(|| format!("MUSIC_U={COOKIE}; Path=/; Secure; HttpOnly"))
            .into_iter()
            .collect(),
        refresh_token: None,
        metadata: Default::default(),
    })
}

impl HttpBackend for Wire {
    fn post(&self, request: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            ApiOrigin::Pc.require_url(&request.url)?;
            if request.route != WireRoute::Eapi {
                return Err(Error::Response("synthetic main flow requires EAPI"));
            }
            let params = std::str::from_utf8(&request.body)
                .ok()
                .and_then(|form| form.strip_prefix("params="))
                .ok_or(Error::Response("invalid synthetic EAPI form"))?;
            let (path, mut data) = eapi::unwrap(params)?;
            if ApiOrigin::Pc.request_url(WireRoute::Eapi, &path)?.path() != request.url.path() {
                return Err(Error::Response("synthetic EAPI path mismatch"));
            }
            if self.service.is_offline() {
                let mut progress = self.progress.lock().map_err(|_| Error::LockPoisoned)?;
                if path == "/api/rtrs/abt/front/expinfo/list"
                    && data["expNames"] == "device_connect"
                {
                    progress.offline_relay_experiment_attempts += 1;
                } else {
                    progress.offline_business_attempts += 1;
                }
                return Err(Error::Transport("synthetic EAPI is disconnected".into()));
            }
            let mut cookies = request
                .headers
                .get("Cookie")
                .into_iter()
                .flat_map(|value| value.split(';'))
                .filter_map(|entry| entry.trim().strip_prefix("MUSIC_U="));
            let authenticated = match (cookies.next(), cookies.next()) {
                (None, None) => false,
                (Some(COOKIE), None) => true,
                _ => return Err(Error::Response("unexpected synthetic session")),
            };
            if self.mode == Mode::Reopen {
                if matches!(path.as_str(), TICKET | POLL) {
                    return Err(Error::State("synthetic reopen does not allow QR"));
                }
                if !matches!(
                    path.as_str(),
                    ACCOUNT
                        | "/api/rtrs/abt/front/expinfo/list"
                        | "/api/relay/setting/show"
                        | "/api/song/like/get"
                        | "/api/user/playlist"
                        | "/api/v3/discovery/recommend/songs"
                ) {
                    return Err(Error::Response(
                        "synthetic reopen allows only bootstrap reads",
                    ));
                }
                if authenticated && path == ACCOUNT {
                    let body = self.service.request(ACCOUNT, data, false).await?;
                    if body["code"] != 200 || body["profile"]["userId"] != 2 {
                        return Err(Error::Response("synthetic reopen identity mismatch"));
                    }
                    let mut progress = self.progress.lock().map_err(|_| Error::LockPoisoned)?;
                    progress.authorized = true;
                    progress.authenticated_identity_reads += 1;
                    return reply(body, false);
                }
                if !authenticated && path != ACCOUNT {
                    return Err(Error::State(
                        "synthetic reopen requires the persisted session",
                    ));
                }
            }
            match path.as_str() {
                ACCOUNT if !authenticated => {
                    let mut progress = self.progress.lock().map_err(|_| Error::LockPoisoned)?;
                    progress.guest_identity_reads += 1;
                    return reply(json!({"code": 301}), false);
                }
                TICKET if !authenticated => {
                    let mut progress = self.progress.lock().map_err(|_| Error::LockPoisoned)?;
                    if progress.guest_identity_reads == 0
                        || progress.tickets != 0
                        || data["type"] != "3"
                    {
                        return Err(Error::State("unexpected synthetic QR ticket request"));
                    }
                    progress.tickets += 1;
                    return reply(json!({"code": 200, "unikey": QR_KEY}), false);
                }
                POLL if !authenticated => return self.qr_poll(&data).await,
                "/api/rtrs/abt/front/expinfo/list" | "/api/relay/setting/show"
                    if !authenticated => {}
                _ if !authenticated => {
                    return Err(Error::Response(
                        "synthetic business request before QR adoption",
                    ));
                }
                _ => {
                    let mut progress = self.progress.lock().map_err(|_| Error::LockPoisoned)?;
                    if !progress.authorized {
                        return Err(Error::State("synthetic cookie predates QR authorization"));
                    }
                    if path == ACCOUNT {
                        progress.authenticated_identity_reads += 1;
                    } else if progress.authenticated_identity_reads == 0 {
                        return Err(Error::State(
                            "synthetic business request predates identity read",
                        ));
                    }
                }
            }
            // EAPI stringifies top-level fields. The direct Service retains its
            // exact internal payload assertions; normalize only checked values.
            match path.as_str() {
                "/api/playlist/create" if data["privacy"] == "10" => data["privacy"] = json!(10),
                "/api/v1/playlist/manipulate/tracks" if data["imme"] == "true" => {
                    data["imme"] = json!(true);
                }
                _ => {}
            }
            let artist_mode = artist_onboarding_fixture::Mode::active();
            if authenticated
                && let Some(mode) = artist_mode
                && let Some(response) = artist_onboarding_fixture::response(mode, &path, &data)
            {
                return reply(response?, false);
            }
            let mut body = self.service.request(&path, data, false).await?;
            if authenticated
                && path == ACCOUNT
                && let Some(mode) = artist_mode
            {
                mode.extend_identity(&mut body);
            }
            reply(body, false)
        })
    }
}
