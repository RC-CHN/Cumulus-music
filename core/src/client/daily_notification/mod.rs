//! Native-only scheduling and receipt-bound actions for the traditional startup Daily notification.
mod policy;

use super::{
    MusicClient,
    dto::TrackView,
    recommendations::{can_use_daily_cache, daily_source, track_page},
};
use crate::{
    Error, Result,
    account::{AccountApi, SessionStatus, SessionToken},
    catalog::{CatalogApi, DailyRecommendationEndpoint},
    playback::controller::PlaybackStatus,
    protocol::{api::Api, mconfig::PcConfig},
    storage::{StateStore, StateTransaction},
};
use chrono::{SecondsFormat, Utc};
use policy::{History, MAX_DAYS, Policy};
use rand::prelude::IndexedRandom;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const OWNER: &str = "@device";
const KIND: &str = "daily-notification-v1";
const KEY: &str = "history";
const PRESENTATION_LIMIT: Duration = Duration::from_secs(30);
const READ_LIMIT: Duration = Duration::from_secs(4);

#[derive(Default)]
pub(super) struct Host {
    config: Option<Arc<PcConfig>>,
    offer: Mutex<Option<Offer>>,
}

pub struct DailyNotificationSchedule {
    owner: SessionToken,
    api: Arc<dyn Api>,
    credentials: String,
    policy: Policy,
    started: Instant,
}

impl DailyNotificationSchedule {
    pub fn delay(&self) -> Duration {
        self.policy.delay
    }
}

struct Offer {
    schedule: DailyNotificationSchedule,
    view: DailyNotificationView,
    issued: Instant,
    shown: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyNotificationView {
    pub epoch: String,
    pub receipt: String,
    pub day: u32,
    pub description: String,
    pub songs: Vec<DailyNotificationSong>,
    pub remaining_ms: u32,
    pub shown: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyNotificationSong {
    name: String,
    artists: String,
    cover_url: Option<String>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum DailyNotificationCommand {
    Shown,
    Close,
    Snooze,
    Disable,
    Settings,
    Details,
    Play,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DailyNotificationOutcome {
    Shown,
    Closed,
    Settings,
    Details,
    Played,
}

pub(in crate::client) struct PlaybackOrigin {
    pub day: String,
}
impl PlaybackOrigin {
    pub fn source(self, revision: &str, uid: &str) -> Value {
        let mut source = daily_source(revision);
        source["scene"] = json!("dailyPush");
        source["sourceData"] = json!({"id":uid,"day":self.day});
        source
    }
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn server_ms(api: &dyn Api, local_ms: i64) -> i64 {
    api.observed_server_time_ms()
        .and_then(|ms| ms.try_into().ok())
        .filter(|ms| chrono::DateTime::from_timestamp_millis(*ms).is_some())
        .unwrap_or(local_ms)
}

fn history(value: Option<Value>) -> Result<History> {
    value
        .map(serde_json::from_value)
        .transpose()
        .map(|value| value.unwrap_or_default())
        .map_err(Into::into)
}

fn edit_history(store: &StateStore, change: impl FnOnce(&mut History) -> Result<()>) -> Result<()> {
    store.transaction(|tx: &mut StateTransaction<'_>| {
        let mut record = history(tx.get(OWNER, KIND, KEY)?)?;
        change(&mut record)?;
        tx.put(OWNER, KIND, KEY, &serde_json::to_value(record)?)
    })
}

fn songs(rows: Vec<TrackView>) -> Vec<DailyNotificationSong> {
    rows.into_iter()
        .take(3)
        .map(|row| DailyNotificationSong {
            name: row.name,
            artists: row
                .artists
                .into_iter()
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>()
                .join("/"),
            cover_url: row.cover_url,
        })
        .collect()
}

impl MusicClient {
    /// Explicit trusted host dependency, used by the isolated WebView proof.
    /// Real desktop configuration is consumed from its verified request context.
    pub fn with_native_daily_notification_config(mut self, config: Arc<PcConfig>) -> Self {
        self.daily_notification.config = Some(config);
        self
    }

    fn notification_policy(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        revision: &str,
    ) -> Result<Option<Policy>> {
        let snapshot = match self.auth.context.as_ref() {
            Some(context) => context.pc_snapshot()?,
            None => self
                .daily_notification
                .config
                .as_ref()
                .map(|config| config.snapshot())
                .transpose()?,
        };
        let Some(data) = snapshot.and_then(|snapshot| snapshot.data) else {
            return Ok(None);
        };
        let groups = self.pc_experiments.observed_groups(token, api, revision)?;
        Ok(groups.and_then(|groups| Policy::read(&groups, &data)))
    }

    /// At app startup only. Does not fetch/refresh experiments or begin playback.
    pub fn daily_notification_startup_plan(&self) -> Result<Option<DailyNotificationSchedule>> {
        let account = self.session.snapshot()?;
        if account.status != SessionStatus::Authenticated || self.login_pending()? {
            return Ok(None);
        }
        let owner = account.token;
        let api = self.session.native_api(&owner)?;
        let Some(credentials) = api.verified_pc_experiment_revision()? else {
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

    fn notification_scope(&self, schedule: &DailyNotificationSchedule) -> Result<()> {
        self.session.require_api(&schedule.owner, &schedule.api)?;
        if self.login_pending()?
            || schedule.api.verified_pc_experiment_revision()?.as_deref()
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

    fn can_offer_daily_notification(
        &self,
        schedule: &DailyNotificationSchedule,
        foreground: &impl Fn() -> bool,
    ) -> Result<bool> {
        self.notification_scope(schedule)?;
        if !foreground()
            || self.player.controller.get().is_some_and(|controller| {
                let current = controller.snapshot();
                current.startup_played || current.state == PlaybackStatus::Playing
            })
        {
            return Ok(false);
        }
        let local_ms = now_ms();
        let state = history(self.registry.store().get(OWNER, KIND, KEY)?)?;
        Ok(schedule.policy.first_exposure(
            &state,
            local_ms,
            server_ms(schedule.api.as_ref(), local_ms),
        ))
    }

    /// The desktop supplies live, trusted main-window foreground/fullscreen
    /// facts. This is not a renderer-authorized timer or generic notification IPC.
    pub async fn prepare_daily_notification(
        &self,
        schedule: DailyNotificationSchedule,
        foreground: impl Fn() -> bool,
    ) -> Result<Option<DailyNotificationView>> {
        if schedule.started.elapsed() < schedule.policy.delay
            || !self.can_offer_daily_notification(&schedule, &foreground)?
        {
            return Ok(None);
        }
        let settings =
            tokio::time::timeout(READ_LIMIT, AccountApi::new(schedule.api.clone()).settings())
                .await
                .map_err(|_| Error::State("daily notification settings read timed out"))??;
        // The recovered branch rejects 2; a missing/unknown field cannot
        // positively authorize optional personalized work in this host.
        if settings["personalServiceSetting"].as_i64() != Some(0)
            || !self.can_offer_daily_notification(&schedule, &foreground)?
        {
            return Ok(None);
        }
        let rows = self.daily_notification_cached_preview(&schedule.owner)?;
        let rows = if rows.is_empty() {
            let endpoint = if schedule.policy.daily_v3 {
                DailyRecommendationEndpoint::V3
            } else {
                DailyRecommendationEndpoint::V1
            };
            let api = CatalogApi::new(schedule.api.clone());
            match tokio::time::timeout(READ_LIMIT, api.daily_for(endpoint)).await {
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
        if rows.is_empty() || !self.can_offer_daily_notification(&schedule, &foreground)? {
            return Ok(None);
        }
        use chrono::Datelike;
        let view = DailyNotificationView {
            epoch: schedule.owner.generation.to_string(),
            receipt: uuid::Uuid::new_v4().simple().to_string(),
            day: chrono::Local::now().day(),
            description: schedule
                .policy
                .descriptions
                .choose(&mut rand::rng())
                .cloned()
                .unwrap_or_default(),
            songs: songs(rows),
            remaining_ms: PRESENTATION_LIMIT.as_millis() as u32,
            shown: false,
        };
        let mut slot = self
            .daily_notification
            .offer
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if slot.is_some() {
            return Ok(None);
        }
        *slot = Some(Offer {
            schedule,
            view: view.clone(),
            issued: Instant::now(),
            shown: false,
        });
        Ok(Some(view))
    }

    pub fn current_daily_notification(&self, epoch: &str) -> Result<Option<DailyNotificationView>> {
        let token = self.player_token(epoch)?;
        let slot = self
            .daily_notification
            .offer
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        let Some(offer) = slot.as_ref().filter(|offer| offer.schedule.owner == token) else {
            return Ok(None);
        };
        let Some(remaining) = PRESENTATION_LIMIT.checked_sub(offer.issued.elapsed()) else {
            return Ok(None);
        };
        self.notification_scope(&offer.schedule)?;
        let mut view = offer.view.clone();
        view.shown = offer.shown;
        view.remaining_ms = remaining.as_millis() as u32;
        Ok(Some(view))
    }

    pub fn daily_notification_enabled(&self, epoch: &str) -> Result<bool> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            Ok(history(self.registry.store().get(OWNER, KIND, KEY)?)?.enabled)
        })
    }

    pub fn set_daily_notification_enabled(&self, epoch: &str, enabled: bool) -> Result<bool> {
        let token = self.player_token(epoch)?;
        let mut slot = self
            .daily_notification
            .offer
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        self.session.with_local_owner(&token, |_| {
            edit_history(&self.registry.store(), |state| {
                state.enabled = enabled;
                if enabled {
                    state.snoozed_at = None;
                }
                Ok(())
            })
        })?;
        if !enabled {
            *slot = None;
        }
        Ok(enabled)
    }

    pub async fn daily_notification_command(
        &self,
        epoch: &str,
        receipt: &str,
        action: DailyNotificationCommand,
        foreground: impl Fn() -> bool,
    ) -> Result<DailyNotificationOutcome> {
        use DailyNotificationCommand as Command;
        use DailyNotificationOutcome as Outcome;
        let owner = self.player_token(epoch)?;
        let consumed = {
            let mut slot = self
                .daily_notification
                .offer
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            let offer = slot
                .as_mut()
                .filter(|offer| {
                    offer.schedule.owner == owner
                        && offer.view.receipt == receipt
                        && offer.issued.elapsed() < PRESENTATION_LIMIT
                })
                .ok_or(Error::StaleResponse)?;
            self.notification_scope(&offer.schedule)?;
            if matches!(action, Command::Shown) {
                if offer.shown {
                    return Ok(Outcome::Shown);
                }
                if !self.can_offer_daily_notification(&offer.schedule, &foreground)? {
                    *slot = None;
                    return Err(Error::StaleResponse);
                }
                let now = now_ms();
                let synchronized = server_ms(offer.schedule.api.as_ref(), now);
                let policy = &offer.schedule.policy;
                self.session.with_token(&owner, |_| {
                    edit_history(&self.registry.store(), |state| {
                        if !policy.first_exposure(state, now, synchronized) {
                            return Err(Error::StaleResponse);
                        }
                        state.shown.push(synchronized);
                        let excess = state.shown.len().saturating_sub(MAX_DAYS);
                        state.shown.drain(..excess);
                        Ok(())
                    })
                })?;
                offer.shown = true;
                return Ok(Outcome::Shown);
            }
            if !matches!(action, Command::Close) && (!offer.shown || !foreground()) {
                return Err(Error::StaleResponse);
            }
            if matches!(action, Command::Snooze | Command::Disable) {
                self.session.with_token(&owner, |_| {
                    edit_history(&self.registry.store(), |state| {
                        if matches!(action, Command::Disable) {
                            state.enabled = false;
                        } else {
                            state.snoozed_at = Some(now_ms());
                        }
                        Ok(())
                    })
                })?;
            }
            slot.take().expect("checked daily notification")
        };
        match action {
            Command::Details => Ok(Outcome::Details),
            Command::Settings => Ok(Outcome::Settings),
            Command::Play => {
                // A fresh explicit play-all obtains the complete list, not
                // the three display rows in this receipt.
                let page = self.daily_songs().await?;
                self.notification_scope(&consumed.schedule)?;
                if page.epoch != epoch || !foreground() {
                    return Err(Error::StaleResponse);
                }
                let revision = page.value.revision.ok_or(Error::StaleResponse)?;
                let quality = self
                    .player
                    .controller
                    .get()
                    .map(|c| c.snapshot().quality_preferences.preferences.quality)
                    .unwrap_or(crate::membership::Quality::Standard);
                self.play_daily_notification_selection(
                    epoch,
                    revision,
                    quality,
                    PlaybackOrigin {
                        day: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                    },
                )
                .await?;
                Ok(Outcome::Played)
            }
            _ => Ok(Outcome::Closed),
        }
    }
}
