//! Native startup/tick arbitration. Request permission is rechecked at dispatch.
use super::{
    Content, DailyNotificationKind, DailyNotificationSchedule, DailyNotificationView, KIND,
    MusicClient, OWNER, Offer, PRESENTATION_LIMIT, READ_LIMIT, copyright, history,
    notification_credential_revision, now_ms,
    policy::{Attempt, Trigger},
    server_ms, songs,
};
use crate::{
    Error, Result,
    account::SessionStatus,
    catalog::{CatalogApi, DailyRecommendationEndpoint},
    client::{
        dto::TrackView,
        recommendations::{can_use_daily_cache, track_page},
    },
    playback::controller::PlaybackStatus,
    protocol::api::{DailyNotificationRead, validate_response},
};
use rand::prelude::IndexedRandom;
use tokio::time::Instant;

impl MusicClient {
    /// At app startup only. Does not fetch/refresh experiments or begin playback.
    pub fn daily_notification_startup_plan(&self) -> Result<Option<DailyNotificationSchedule>> {
        let account = self.session.snapshot()?;
        if account.status != SessionStatus::Authenticated || self.login_pending()? {
            return Ok(None);
        }
        let owner = account.token;
        let api = self.session.native_api(&owner)?;
        let Some(credentials) = notification_credential_revision(api.as_ref())? else {
            return Ok(None);
        };
        let Some(policy) = self.notification_policy(&owner, &api, &credentials)? else {
            return Ok(None);
        };
        self.session.require_api(&owner, &api)?;
        Ok(Some(DailyNotificationSchedule {
            owner,
            api,
            credentials,
            policy,
            started: Instant::now(),
        }))
    }

    pub(super) fn notification_wall_ms(&self) -> i64 {
        #[cfg(test)]
        if let Some(clock) = &self.daily_notification.clock {
            return clock();
        }
        now_ms()
    }

    pub(super) fn notification_scope(&self, schedule: &DailyNotificationSchedule) -> Result<()> {
        self.session.require_api(&schedule.owner, &schedule.api)?;
        // QR identity holds its own login-state lock while adopting a session.
        // Never consult the pending login inside the session commit lease.
        if self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        self.require_notification_policy(schedule)
    }

    // Also called in the account commit lease; do not re-enter the session.
    pub(super) fn require_notification_policy(
        &self,
        schedule: &DailyNotificationSchedule,
    ) -> Result<()> {
        if notification_credential_revision(schedule.api.as_ref())?.as_deref()
            != Some(&schedule.credentials)
            || self
                .notification_policy(&schedule.owner, &schedule.api, &schedule.credentials)?
                .as_ref()
                != Some(&schedule.policy)
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(super) fn can_offer_notification(
        &self,
        schedule: &DailyNotificationSchedule,
        kind: DailyNotificationKind,
        attempt: Attempt,
        foreground: &impl Fn() -> bool,
    ) -> Result<bool> {
        self.notification_scope(schedule)?;
        if !foreground() {
            return Ok(false);
        }
        let eligible = match kind {
            DailyNotificationKind::Copyright => schedule.policy.copyright,
            DailyNotificationKind::Daily => {
                schedule.policy.daily
                    && !self.player.controller.get().is_some_and(|controller| {
                        let current = controller.snapshot();
                        current.startup_played || current.state == PlaybackStatus::Playing
                    })
            }
        };
        if !eligible {
            return Ok(false);
        }
        let now = self.notification_wall_ms();
        let synchronized = server_ms(schedule.api.as_ref(), now);
        self.session
            .with_verified_profile(&schedule.owner, &schedule.api, |_, _| {
                self.registry.store().transaction(|tx| {
                    let state = history(tx.get(OWNER, KIND, kind.key())?)?;
                    let sibling = history(tx.get(OWNER, KIND, kind.other().key())?)?;
                    Ok(schedule
                        .policy
                        .eligible(&state, &sibling, attempt, now, synchronized))
                })
            })
    }

    /// The desktop supplies live, trusted main-window foreground/fullscreen
    /// facts. This is not a renderer-authorized timer or generic notification IPC.
    pub async fn prepare_daily_notification(
        &self,
        schedule: &DailyNotificationSchedule,
        foreground: impl Fn() -> bool + Sync,
    ) -> Result<Option<DailyNotificationView>> {
        if schedule.started.elapsed() < schedule.policy.delay {
            return Ok(None);
        }
        self.prepare_notification(
            schedule,
            Trigger::Startup,
            schedule.policy.copyright,
            &foreground,
        )
        .await
    }

    /// One tick from the native half-hour worker. Its priority and account/API
    /// are captured at startup; credentials and policy must be fresh at each
    /// tick. This passive read does not request another experiment assignment.
    pub async fn prepare_recurring_daily_notification(
        &self,
        startup: &DailyNotificationSchedule,
        foreground: impl Fn() -> bool + Sync,
    ) -> Result<Option<DailyNotificationView>> {
        self.session.require_api(&startup.owner, &startup.api)?;
        if startup.started.elapsed() < startup.policy.delay || self.login_pending()? {
            return Ok(None);
        }
        let Some(credentials) = notification_credential_revision(startup.api.as_ref())? else {
            return Ok(None);
        };
        let Some(policy) = self.notification_policy(&startup.owner, &startup.api, &credentials)?
        else {
            return Ok(None);
        };
        let current = DailyNotificationSchedule {
            credentials,
            policy,
            ..startup.clone()
        };
        self.prepare_notification(
            &current,
            Trigger::HalfHour,
            startup.policy.copyright,
            &foreground,
        )
        .await
    }

    async fn prepare_notification(
        &self,
        schedule: &DailyNotificationSchedule,
        trigger: Trigger,
        copyright_first: bool,
        foreground: &(impl Fn() -> bool + Sync),
    ) -> Result<Option<DailyNotificationView>> {
        use DailyNotificationKind::{Copyright, Daily};
        use copyright::Candidate;
        self.notification_scope(schedule)?;
        {
            let mut slot = self
                .daily_notification
                .offer
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            self.retire_notification(&mut slot)?;
            if slot.is_some() {
                return Ok(None);
            }
        }
        if !foreground() {
            return Ok(None);
        }
        let attempt = || Attempt {
            trigger,
            local_ms: self.notification_wall_ms(),
        };
        let copyright_attempt = attempt();
        if copyright_first
            && self.can_offer_notification(schedule, Copyright, copyright_attempt, foreground)?
        {
            match self
                .copyright_candidate(schedule, copyright_attempt, foreground)
                .await
            {
                Ok(Candidate::Available(copyright)) => {
                    if !self.can_offer_notification(
                        schedule,
                        Copyright,
                        copyright_attempt,
                        foreground,
                    )? {
                        return Ok(None);
                    }
                    let rows = copyright
                        .rows
                        .iter()
                        .take(3)
                        .map(|row| row.track.clone())
                        .collect();
                    let description = copyright.description.clone();
                    return self.publish_notification(
                        schedule,
                        copyright_attempt,
                        Content::Copyright(copyright),
                        rows,
                        description,
                    );
                }
                Ok(Candidate::NoPopupIds) if trigger == Trigger::HalfHour => return Ok(None),
                Err(error)
                    if matches!(
                        error.root(),
                        Error::StaleResponse | Error::CredentialChanged | Error::Cancelled
                    ) =>
                {
                    return Err(error);
                }
                // A real read failure or no renderable detail is different from
                // a successful periodic popup whose list has no IDs (M/128).
                Ok(Candidate::NoPopupIds | Candidate::NoRenderableRows) | Err(_) => (),
            }
        }
        if !foreground() {
            return Ok(None);
        }
        let daily_attempt = attempt();
        let allowed = || self.can_offer_notification(schedule, Daily, daily_attempt, foreground);
        if !allowed()? {
            return Ok(None);
        }
        let permission = || {
            if allowed()? {
                Ok(())
            } else {
                Err(Error::StaleResponse)
            }
        };
        let api = &schedule.api;
        let revision = &schedule.credentials;
        let settings = tokio::time::timeout(
            READ_LIMIT,
            api.request_daily_notification(
                DailyNotificationRead::settings(),
                revision,
                &permission,
            ),
        )
        .await
        .map_err(|_| Error::State("daily notification settings read timed out"))??;
        let settings = validate_response(DailyNotificationRead::SETTINGS, settings, &[200])?;
        // Recovered code rejects 2. Unknown/missing must not authorize
        // optional personalized traffic in this host.
        if settings["setting"]["personalServiceSetting"].as_i64() != Some(0) || !allowed()? {
            return Ok(None);
        }
        let rows = self.daily_notification_cached_preview(&schedule.owner)?;
        let rows = if rows.is_empty() {
            let endpoint = if schedule.policy.daily_v3 {
                DailyRecommendationEndpoint::V3
            } else {
                DailyRecommendationEndpoint::V1
            };
            let read = async {
                let reply = api
                    .request_daily_notification(
                        DailyNotificationRead::recommendations(endpoint),
                        revision,
                        &permission,
                    )
                    .await?;
                let reply = validate_response(endpoint.path(), reply, &[200])?;
                CatalogApi::parse_daily_reply(reply, endpoint)
            };
            match tokio::time::timeout(READ_LIMIT, read).await {
                Ok(Ok(reply)) => track_page(&reply.songs)?.tracks,
                Ok(Err(error)) if can_use_daily_cache(&error) => {
                    self.daily_notification_persisted_preview(&schedule.owner)?
                }
                Ok(Err(error)) => return Err(error),
                Err(_) => return Ok(None),
            }
        } else {
            rows
        };
        if rows.is_empty() || !allowed()? {
            return Ok(None);
        }
        let description = schedule
            .policy
            .descriptions
            .choose(&mut rand::rng())
            .cloned()
            .unwrap_or_default();
        self.publish_notification(schedule, daily_attempt, Content::Daily, rows, description)
    }

    pub(super) fn retire_notification(&self, slot: &mut Option<Offer>) -> Result<()> {
        let Some(offer) = slot else {
            return Ok(());
        };
        let stale = if offer.issued.elapsed() >= PRESENTATION_LIMIT
            || !offer.attempt.still_today(self.notification_wall_ms())
        {
            true
        } else {
            match self.notification_scope(&offer.schedule) {
                Ok(()) => false,
                Err(error)
                    if matches!(
                        error.root(),
                        Error::StaleResponse | Error::CredentialChanged | Error::Cancelled
                    ) =>
                {
                    true
                }
                Err(error) => return Err(error),
            }
        };
        if stale {
            *slot = None;
        }
        Ok(())
    }

    fn publish_notification(
        &self,
        schedule: &DailyNotificationSchedule,
        attempt: Attempt,
        content: Content,
        rows: Vec<TrackView>,
        description: String,
    ) -> Result<Option<DailyNotificationView>> {
        use chrono::Datelike;
        let (kind, playlist_id) = match &content {
            Content::Daily => (DailyNotificationKind::Daily, None),
            Content::Copyright(value) => (DailyNotificationKind::Copyright, value.playlist.clone()),
        };
        let view = DailyNotificationView {
            epoch: schedule.owner.generation.to_string(),
            receipt: uuid::Uuid::new_v4().simple().to_string(),
            kind,
            playlist_id,
            day: chrono::DateTime::from_timestamp_millis(attempt.local_ms)
                .ok_or(Error::StaleResponse)?
                .with_timezone(&chrono::Local)
                .day(),
            description,
            songs: songs(rows),
            remaining_ms: PRESENTATION_LIMIT.as_millis() as u32,
            shown: false,
        };
        let mut slot = self
            .daily_notification
            .offer
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        self.retire_notification(&mut slot)?;
        self.notification_scope(schedule)?;
        if slot.is_some() || !attempt.still_today(self.notification_wall_ms()) {
            return Ok(None);
        }
        self.session
            .with_verified_profile(&schedule.owner, &schedule.api, |_, _| {
                self.require_notification_policy(schedule)?;
                *slot = Some(Offer {
                    attempt,
                    schedule: schedule.clone(),
                    view: view.clone(),
                    issued: Instant::now(),
                    shown: false,
                    content,
                });
                Ok(Some(view))
            })
    }
}
