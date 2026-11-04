//! Explicit sign command (M/15/M/617) with a durable, account/day send barrier.
use super::{
    CARD, DETAIL, HISTORY, MemberSignCard, MemberSignDetail, MusicClient, card, detail, timestamp,
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    client::dto::AccountData,
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use time::{Date, OffsetDateTime, UtcOffset};

const SIGN: &str = "/api/vip-center-bff/task/sign";
const JOURNAL: &str = "member_sign";
const SERVICE_ZONE_SECONDS: i32 = 8 * 60 * 60;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MemberSignCommand {
    CheckIn { day_key: String },
    Reconcile,
}

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemberSignPhase {
    Ready,
    Signed,
    Unknown,
    Unavailable,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberSignAction {
    phase: MemberSignPhase,
    day_key: Option<String>,
}

impl MemberSignAction {
    pub(super) fn unavailable() -> Self {
        Self {
            phase: MemberSignPhase::Unavailable,
            day_key: None,
        }
    }

    fn for_day(phase: MemberSignPhase, day: Date) -> Self {
        Self {
            phase,
            day_key: Some(day.to_string()),
        }
    }
}

#[derive(Serialize)]
pub struct MemberSignOutcome {
    action: MemberSignAction,
    card: Option<MemberSignCard>,
    detail: Option<MemberSignDetail>,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Journal {
    Unknown,
    Confirmed,
}

struct Scope<'a> {
    session: &'a AccountSession,
    token: SessionToken,
    api: Arc<dyn Api>,
    store: Arc<StateStore>,
    owner: String,
}

impl<'a> Scope<'a> {
    fn new(client: &'a MusicClient, epoch: &str) -> Result<Self> {
        let session = &client.session;
        let token = client.member_token(epoch)?;
        let api = session.native_api(&token)?;
        let owner = session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        let store = session.persistent_store()?;
        Ok(Self {
            session,
            token,
            api,
            store,
            owner,
        })
    }

    fn clock(&self) -> Option<(Date, u64)> {
        let ms = self
            .api
            .observed_server_time_ms()
            .filter(|ms| *ms > 1_740_000_000_000)?;
        Some((service_day(ms)?, ms))
    }

    fn write_barrier(&self, day: Date, marker: Journal) -> Result<()> {
        self.session
            .save_result(&self.token, JOURNAL, &day.to_string(), &json!(marker))
    }

    fn marker(&self, day: Date) -> Result<Option<Journal>> {
        self.session.with_token(&self.token, |uid| {
            self.store
                .get(uid, JOURNAL, &day.to_string())?
                .map(serde_json::from_value)
                .transpose()
                .map_err(Into::into)
        })
    }

    async fn request(&self, endpoint: &'static str, args: Value) -> Result<Value> {
        self.session
            .run_for(&self.token, |api, _| async move {
                api.call(endpoint, args, false, &[200]).await
            })
            .await
    }

    async fn read_card(&self) -> Result<(MemberSignCard, Option<Date>)> {
        let mut card = card(&self.request(CARD, json!({"type":1})).await?["data"])?;
        let mut slots = card.days.iter().filter(|slot| slot.today);
        let today = slots.next().filter(|_| slots.next().is_none());
        let day = self.clock().map(|(day, _)| day).filter(|day| {
            today.and_then(|slot| slot.label.strip_suffix('日')?.trim().parse::<u8>().ok())
                == Some(day.day())
        });
        card.action = if let Some(day) = day {
            let marker = self.marker(day)?;
            let phase =
                if marker == Some(Journal::Confirmed) || today.is_some_and(|slot| slot.signed) {
                    MemberSignPhase::Signed
                } else if marker == Some(Journal::Unknown) {
                    MemberSignPhase::Unknown
                } else {
                    MemberSignPhase::Ready
                };
            MemberSignAction::for_day(phase, day)
        } else if today.is_some_and(|slot| slot.signed) {
            MemberSignAction {
                phase: MemberSignPhase::Signed,
                day_key: None,
            }
        } else {
            MemberSignAction::unavailable()
        };
        Ok((card, day))
    }

    fn optional<T>(&self, result: Result<T>) -> Result<Option<T>> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error)
                if error.is_service_code(301)
                    || matches!(
                        error.root(),
                        Error::StaleResponse | Error::CredentialChanged
                    ) =>
            {
                Err(error)
            }
            Err(_) => {
                self.session.check_generation(&self.token)?;
                Ok(None)
            }
        }
    }

    async fn recorded(&self, day: Date, hint: Option<u64>) -> Result<Option<(Value, u64)>> {
        let response = self.request(HISTORY, json!({})).await?;
        let rows = response["data"]
            .as_array()
            .filter(|list| list.len() <= 3660)
            .ok_or(Error::Response("invalid sign history list"))?;
        let mut same_day = rows.iter().filter_map(|row| {
            let time = timestamp(&row["dayTime"])?;
            (service_day(time)? == day).then_some((row, time))
        });
        let exact = hint.and_then(|hint| same_day.clone().find(|(_, time)| *time == hint));
        let record = exact.or_else(|| same_day.next().filter(|_| same_day.next().is_none()));
        record
            .map(|(row, time)| {
                let id = &row["recordId"];
                identifier(id).map_err(|_| Error::Response("invalid sign history record"))?;
                Ok((id.clone(), time))
            })
            .transpose()
    }

    async fn result_detail(&self, args: Value, fallback_time: u64) -> Result<MemberSignDetail> {
        detail(&self.request(DETAIL, args).await?["data"], fallback_time)
    }

    async fn reconcile(
        &self,
        day: Date,
        mut phase: MemberSignPhase,
        mut card: Option<MemberSignCard>,
    ) -> Result<MemberSignOutcome> {
        let hint = card
            .as_ref()
            .and_then(|card| card.days.iter().find(|slot| slot.today)?.sign_time);
        let mut detail = None;
        if matches!(phase, MemberSignPhase::Unknown | MemberSignPhase::Signed) {
            let record = self.optional(self.recorded(day, hint).await)?.flatten();
            if let Some((record_id, time)) = record {
                phase = MemberSignPhase::Signed;
                self.write_barrier(day, Journal::Confirmed)?;
                detail = self.optional(
                    self.result_detail(json!({"type":2,"recordId":record_id}), time)
                        .await,
                )?;
            } else if phase == MemberSignPhase::Signed {
                self.write_barrier(day, Journal::Confirmed)?;
            }
        }
        let action = MemberSignAction::for_day(phase, day);
        if let Some(card) = card.as_mut() {
            card.action = action.clone();
        }
        Ok(MemberSignOutcome {
            action,
            card,
            detail,
        })
    }

    async fn check_in(&self, day: Date, sign_time: u64) -> Result<MemberSignOutcome> {
        // If the process exits or the response is lost after this transaction,
        // the next process can only inspect the result; it cannot replay it.
        self.write_barrier(day, Journal::Unknown)?;
        let sign = match self.request(SIGN, json!({})).await {
            Err(error) if matches!(error.root(), Error::RequestNotSent(_)) => {
                self.session.with_token(&self.token, |uid| {
                    self.store.delete(uid, JOURNAL, &day.to_string())
                })?;
                return Err(error);
            }
            result => result,
        };
        let sign = self.optional(sign)?;
        let acknowledged = sign.is_some_and(|response| js_truthy(&response["data"]));
        let now = self
            .clock()
            .filter(|(current, _)| *current == day)
            .map(|(_, ms)| ms);
        if let Some(now) = now.filter(|_| acknowledged) {
            self.write_barrier(day, Journal::Confirmed)?;
            let detail = self.optional(
                self.result_detail(json!({"type":1,"signDayTime":now}), sign_time)
                    .await,
            )?;
            let mut card = self
                .optional(self.read_card().await)?
                .and_then(|(card, fresh)| (fresh == Some(day)).then_some(card));
            let action = MemberSignAction::for_day(MemberSignPhase::Signed, day);
            if let Some(card) = card.as_mut() {
                card.action = action.clone();
            }
            return Ok(MemberSignOutcome {
                action,
                card,
                detail,
            });
        }
        let card = self
            .optional(self.read_card().await)?
            .and_then(|(card, fresh)| (fresh == Some(day)).then_some(card));
        let phase = card
            .as_ref()
            .map_or(MemberSignPhase::Unknown, |card| card.action.phase);
        // An absent or unsigned response does not prove a write was rejected.
        self.reconcile(day, phase, card).await
    }

    fn finish<T>(&self, epoch: &str, value: T) -> Result<AccountData<T>> {
        self.session.with_token(&self.token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}

fn service_day(ms: u64) -> Option<Date> {
    let zone = UtcOffset::from_whole_seconds(SERVICE_ZONE_SECONDS).ok()?;
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .ok()
        .map(|date| date.to_offset(zone).date())
}

impl MusicClient {
    pub async fn member_sign_card(&self, epoch: &str) -> Result<AccountData<MemberSignCard>> {
        let scope = Scope::new(self, epoch)?;
        let (card, _) = scope.read_card().await?;
        scope.finish(epoch, card)
    }

    pub async fn member_sign_command(
        &self,
        epoch: &str,
        command: MemberSignCommand,
    ) -> Result<AccountData<MemberSignOutcome>> {
        let scope = Scope::new(self, epoch)?;
        let _lock = OperationLock::acquire(scope.store.directory(), &scope.owner, "member-sign")?;
        let (card, day) = scope.read_card().await?;
        let action = card.action.clone();
        if let MemberSignCommand::CheckIn { day_key } = &command
            && action.day_key.as_deref() != Some(day_key.as_str())
        {
            return Err(Error::QueueChanged);
        }
        let outcome = match day {
            Some(day)
                if matches!(command, MemberSignCommand::CheckIn { .. })
                    && action.phase == MemberSignPhase::Ready =>
            {
                let (current, ms) = scope.clock().ok_or(Error::QueueChanged)?;
                if current != day {
                    return Err(Error::QueueChanged);
                }
                scope.check_in(day, ms).await?
            }
            Some(day) => scope.reconcile(day, action.phase, Some(card)).await?,
            None => MemberSignOutcome {
                action,
                card: Some(card),
                detail: None,
            },
        };
        scope.finish(epoch, outcome)
    }
}
