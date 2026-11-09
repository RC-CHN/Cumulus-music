//! Shared native startup and half-hour arbitration and receipt-bound actions for PC notifications.
mod copyright;
mod policy;
mod runtime;
mod timing;

pub use timing::DailyNotificationTimer;

#[cfg(test)]
mod tests;

use super::{MusicClient, dto::TrackView, recommendations::daily_source};
use crate::{
    Error, Result,
    account::SessionToken,
    protocol::{api::Api, mconfig::PcConfig},
    storage::{StateStore, StateTransaction},
};
use chrono::{SecondsFormat, Utc};
use policy::{Attempt, History, MAX_DAYS, Policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;

const OWNER: &str = "@device";
const KIND: &str = "daily-notification-v1";

const PRESENTATION_LIMIT: Duration = Duration::from_secs(30);
const READ_LIMIT: Duration = Duration::from_secs(4);

#[derive(Default)]
pub(super) struct Host {
    config: Option<Arc<PcConfig>>,
    offer: Mutex<Option<Offer>>,
    #[cfg(test)]
    clock: Option<Arc<dyn Fn() -> i64 + Send + Sync>>,
}

#[derive(Clone)]
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
    attempt: Attempt,
    schedule: DailyNotificationSchedule,
    view: DailyNotificationView,
    issued: Instant,
    shown: bool,
    content: Content,
}

enum Content {
    Daily,
    Copyright(copyright::CopyrightOffer),
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DailyNotificationKind {
    Daily,
    Copyright,
}

impl DailyNotificationKind {
    fn key(self) -> &'static str {
        match self {
            Self::Daily => "history",
            Self::Copyright => "copyright-history",
        }
    }
    fn other(self) -> Self {
        match self {
            Self::Daily => Self::Copyright,
            Self::Copyright => Self::Daily,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyNotificationView {
    pub epoch: String,
    pub receipt: String,
    pub kind: DailyNotificationKind,
    pub playlist_id: Option<String>,
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
    DisabledDaily,
    DisabledCopyright,
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

fn notification_credential_revision(api: &dyn Api) -> Result<Option<String>> {
    match api.verified_pc_experiment_revision() {
        // The native adapter reports a withdrawn/absent signed PC context
        // as State. It is no longer authority, not an occupied receipt.
        Err(error) if matches!(error.root(), Error::State(_)) => Ok(None),
        revision => revision,
    }
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

fn edit_history(
    store: &StateStore,
    kind: DailyNotificationKind,
    change: impl FnOnce(&mut History, &History) -> Result<()>,
) -> Result<()> {
    store.transaction(|tx: &mut StateTransaction<'_>| {
        let mut record = history(tx.get(OWNER, KIND, kind.key())?)?;
        let other = history(tx.get(OWNER, KIND, kind.other().key())?)?;
        change(&mut record, &other)?;
        tx.put(OWNER, KIND, kind.key(), &serde_json::to_value(record)?)
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

    pub fn current_daily_notification(&self, epoch: &str) -> Result<Option<DailyNotificationView>> {
        let token = self.player_token(epoch)?;
        let mut slot = self
            .daily_notification
            .offer
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        self.retire_notification(&mut slot)?;
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

    fn copyright_notification_preference_available(&self, token: &SessionToken) -> Result<bool> {
        let api = self.session.native_api(token)?;
        if token.uid.is_none() || self.login_pending()? {
            return Ok(false);
        }
        let Some(revision) = api.verified_pc_experiment_revision().ok().flatten() else {
            return Ok(false);
        };
        let enabled = self
            .pc_experiments
            .observed_group(token, &api, &revision, "PH-PC-push2")?
            .as_deref()
            == Some("t1");
        self.session.require_api(token, &api)?;
        Ok(enabled)
    }

    pub fn daily_notification_enabled(
        &self,
        epoch: &str,
        kind: DailyNotificationKind,
    ) -> Result<Option<bool>> {
        let token = self.player_token(epoch)?;
        if kind == DailyNotificationKind::Copyright
            && !self.copyright_notification_preference_available(&token)?
        {
            return Ok(None);
        }
        self.session.with_local_owner(&token, |_| {
            Ok(Some(
                history(self.registry.store().get(OWNER, KIND, kind.key())?)?.enabled,
            ))
        })
    }

    pub fn set_daily_notification_enabled(
        &self,
        epoch: &str,
        kind: DailyNotificationKind,
        enabled: bool,
    ) -> Result<bool> {
        let token = self.player_token(epoch)?;
        if kind == DailyNotificationKind::Copyright
            && !self.copyright_notification_preference_available(&token)?
        {
            return Err(Error::StaleResponse);
        }
        let mut slot = self
            .daily_notification
            .offer
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        self.session.with_local_owner(&token, |_| {
            edit_history(&self.registry.store(), kind, |state, _| {
                state.enabled = enabled;
                if enabled {
                    state.snoozed_at = None;
                }
                Ok(())
            })
        })?;
        if !enabled && slot.as_ref().is_some_and(|offer| offer.view.kind == kind) {
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
            self.retire_notification(&mut slot)?;
            let offer = slot
                .as_mut()
                .filter(|offer| offer.schedule.owner == owner && offer.view.receipt == receipt)
                .ok_or(Error::StaleResponse)?;
            self.notification_scope(&offer.schedule)?;
            if matches!(action, Command::Shown) {
                if offer.shown {
                    return Ok(Outcome::Shown);
                }
                if !self.can_offer_notification(
                    &offer.schedule,
                    offer.view.kind,
                    offer.attempt,
                    &foreground,
                )? {
                    *slot = None;
                    return Err(Error::StaleResponse);
                }
                let now = self.notification_wall_ms();
                let synchronized = server_ms(offer.schedule.api.as_ref(), now);
                let policy = &offer.schedule.policy;
                self.session
                    .with_verified_profile(&owner, &offer.schedule.api, |_, _| {
                        self.require_notification_policy(&offer.schedule)?;
                        edit_history(&self.registry.store(), offer.view.kind, |state, sibling| {
                            if !policy.eligible(state, sibling, offer.attempt, now, synchronized) {
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
            if matches!(action, Command::Details)
                && matches!(&offer.content, Content::Copyright(value) if value.playlist.is_none())
            {
                // The renderer cannot turn Play-only authority into navigation
                // or a write. Leave this receipt available for genuine actions.
                return Err(Error::State(
                    "copyright notification has no existing playlist",
                ));
            }
            if matches!(action, Command::Snooze | Command::Disable) {
                self.session.with_token(&owner, |_| {
                    edit_history(&self.registry.store(), offer.view.kind, |state, _| {
                        if matches!(action, Command::Disable) {
                            state.enabled = false;
                        } else {
                            state.snoozed_at = Some(self.notification_wall_ms());
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
            Command::Disable => Ok(match consumed.view.kind {
                DailyNotificationKind::Daily => Outcome::DisabledDaily,
                DailyNotificationKind::Copyright => Outcome::DisabledCopyright,
            }),
            Command::Play => {
                if let Content::Copyright(value) = consumed.content {
                    self.notification_scope(&consumed.schedule)?;
                    if !foreground() {
                        return Err(Error::StaleResponse);
                    }
                    if !value.rows.first().is_some_and(|row| row.playable) {
                        return Err(Error::State(
                            "first returning song is not currently playable",
                        ));
                    }
                    let rows = value
                        .rows
                        .into_iter()
                        .filter(|row| row.playable)
                        .map(|row| row.track)
                        .collect();
                    self.play_copyright_notification(
                        &owner,
                        rows,
                        self.copyright_play_authority(&consumed.schedule),
                    )
                    .await?;
                    return Ok(Outcome::Played);
                }
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
