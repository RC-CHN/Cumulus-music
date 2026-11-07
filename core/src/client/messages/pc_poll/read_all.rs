//! M3090: explicit confirmed account-wide read-all. The count actor is also
//! the mutation's ordering boundary: no pre-write read may publish after it.
use super::{REQUEST_TIMEOUT, State};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    client::{
        dto::AccountData,
        messages::{MessageCounts, read_counts},
    },
    protocol::api::Api,
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use std::{
    future::{Future, pending},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::oneshot, time::Instant};
use uuid::Uuid;

const INTENT_TTL: Duration = Duration::from_secs(30);
const JOURNAL: &str = "message-read-all-v1";
const KEY: &str = "latest";

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessageReadAllAction {
    Status,
    Prepare,
    Cancel { intent: String },
    Confirm { intent: String },
    Check { operation: String },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MessageReadAllState {
    Idle,
    Unavailable,
    Ready { intent: String },
    Pending { operation: String },
    Uncertain { operation: String },
    Confirmed { operation: String },
    NotSent { operation: String },
    CheckedClear { operation: String },
    CheckInconclusive { operation: String },
    CheckUnavailable { operation: String },
}

pub(super) type Reply = oneshot::Sender<Result<AccountData<MessageReadAllState>>>;

pub(super) type Completion = (Reply, Result<AccountData<MessageReadAllState>>);

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    operation: String,
    phase: Phase,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Uncertain,
    Confirmed,
    NotSent,
    CheckedClear,
}

impl Record {
    fn view(self) -> MessageReadAllState {
        let operation = self.operation;
        match self.phase {
            Phase::Uncertain => MessageReadAllState::Uncertain { operation },
            Phase::Confirmed => MessageReadAllState::Confirmed { operation },
            Phase::NotSent => MessageReadAllState::NotSent { operation },
            Phase::CheckedClear => MessageReadAllState::CheckedClear { operation },
        }
    }
}

fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn decode(value: Option<serde_json::Value>) -> Result<Option<Record>> {
    let result: Option<Record> = value.map(serde_json::from_value).transpose()?;
    if result
        .as_ref()
        .is_some_and(|record| !valid_id(&record.operation))
    {
        return Err(Error::State("invalid message read-all journal"));
    }
    Ok(result)
}

fn get(store: &StateStore, uid: &str) -> Result<Option<Record>> {
    decode(store.get(uid, JOURNAL, KEY)?)
}

fn admit(store: &StateStore, uid: &str, operation: &str) -> Result<Record> {
    store.transaction(|tx| {
        if let Some(saved) = decode(tx.get(uid, JOURNAL, KEY)?)?
            && saved.phase == Phase::Uncertain
        {
            return Ok(saved);
        }
        let record = Record {
            operation: operation.into(),
            phase: Phase::Uncertain,
        };
        tx.put(uid, JOURNAL, KEY, &serde_json::to_value(&record)?)?;
        Ok(record)
    })
}

fn settle(store: &StateStore, uid: &str, record: &Record) -> Result<()> {
    store.transaction(|tx| {
        let previous = decode(tx.get(uid, JOURNAL, KEY)?)?;
        if !previous.is_some_and(|previous| {
            previous.operation == record.operation && previous.phase == Phase::Uncertain
        }) {
            return Err(Error::StaleResponse);
        }
        tx.put(uid, JOURNAL, KEY, &serde_json::to_value(record)?)
    })
}

fn has_unread(counts: &MessageCounts) -> bool {
    match [
        counts.private,
        counts.comments,
        counts.forwards,
        counts.notices,
    ] {
        [Some(private), Some(comments), Some(forwards), Some(notices)] => {
            private.max(comments).max(forwards).max(notices) > 0
        }
        _ => false,
    }
}

pub(super) fn zero_counts() -> MessageCounts {
    MessageCounts {
        private: Some(0),
        comments: Some(0),
        forwards: Some(0),
        notices: Some(0),
    }
}

pub(super) struct Scope {
    api: Arc<dyn Api>,
    revision: String,
}

impl Scope {
    fn capture(session: &AccountSession, token: &SessionToken) -> Result<Self> {
        let api = session.native_api(token)?;
        session.with_verified_profile(token, &api, |_, _| Ok(()))?;
        let revision = api
            .message_read_all_scope_revision()?
            .ok_or(Error::CredentialChanged)?;
        Ok(Self { api, revision })
    }

    fn verify(&self, session: &AccountSession, token: &SessionToken) -> Result<()> {
        session.with_verified_profile(token, &self.api, |_, _| Ok(()))?;
        if self.api.message_read_all_scope_revision()?.as_deref() != Some(self.revision.as_str()) {
            return Err(Error::CredentialChanged);
        }
        Ok(())
    }
}

pub(super) struct CountSample {
    pub(super) counts: MessageCounts,
    scope: Option<Scope>,
}

impl CountSample {
    pub(super) async fn read(session: &AccountSession, token: &SessionToken) -> Result<Self> {
        let scope = Scope::capture(session, token).ok();
        let counts = read_counts(session, token).await?;
        let scope = scope.filter(|scope| scope.verify(session, token).is_ok());
        Ok(Self { counts, scope })
    }
}

struct Observation {
    scope: Scope,
    observed: Instant,
}

struct Intent {
    id: String,
    expires: Instant,
}

#[derive(Default)]
pub(super) struct ReadAll {
    observed: Option<Observation>,
    intent: Option<Intent>,
    operation: Option<String>,
    pub(super) work: Option<Work>,
}

impl ReadAll {
    pub(super) fn revoke_intent(&mut self) {
        self.intent = None;
    }

    pub(super) fn observe(&mut self, sample: Option<&mut CountSample>) {
        self.revoke_intent();
        self.observed = sample.and_then(|sample| {
            Some(Observation {
                scope: sample.scope.take()?,
                observed: Instant::now(),
            })
        });
    }
}

pub(super) type Work = Pin<Box<dyn Future<Output = WorkResult> + Send>>;

pub(super) struct WorkResult {
    token: SessionToken,
    record: Record,
    store: Arc<StateStore>,
    _lock: OperationLock,
    reply: Reply,
    outcome: Outcome,
}

enum Outcome {
    Write(Phase),
    Check(Result<CountSample>),
}

pub(super) async fn wait_for_work(work: &mut Option<Work>) -> WorkResult {
    match work {
        Some(work) => work.await,
        None => pending().await,
    }
}

impl State {
    pub(super) fn read_all_action(
        &mut self,
        generation: u64,
        action: MessageReadAllAction,
        reply: Reply,
    ) -> Option<Completion> {
        if reply.is_closed() {
            return None;
        }
        match self.prepare_read_all(generation, action) {
            Ok(Admission::Return(state)) => Some((reply, Ok(self.read_all_data(state)))),
            Ok(Admission::Start(start)) => {
                // Cancellation happens only once a write or explicit fresh
                // readback has been admitted. While it runs, periodic/manual
                // reads cannot overtake it or overwrite its acknowledgement.
                self.read = None;
                self.view.count_read_in_flight = matches!(start.task, Task::Check(_));
                self.read_all.observe(None);
                self.read_all.operation = Some(start.record.operation.clone());
                self.read_all.work =
                    Some(start.run(self.session.clone(), self.token.clone(), reply));
                None
            }
            Err(error) => Some((reply, Err(error))),
        }
    }

    fn read_all_data(&self, value: MessageReadAllState) -> AccountData<MessageReadAllState> {
        AccountData {
            epoch: self.token.generation.to_string(),
            value,
        }
    }

    fn prepare_read_all(
        &mut self,
        generation: u64,
        action: MessageReadAllAction,
    ) -> Result<Admission> {
        use MessageReadAllAction::{Cancel, Check, Confirm, Prepare, Status};
        use MessageReadAllState::{Idle, Pending, Ready, Unavailable};
        if !self.is_current_account(generation) {
            return Err(Error::StaleResponse);
        }
        if let Cancel { intent } = action {
            if self
                .read_all
                .intent
                .as_ref()
                .is_none_or(|pending| pending.id != intent)
            {
                return Err(Error::StaleResponse);
            }
            self.read_all.revoke_intent();
            return Ok(Admission::Return(Idle));
        }
        let store = self.session.persistent_store()?;
        if let Status | Prepare = action {
            if let Some(operation) = &self.read_all.operation {
                return Ok(Admission::Return(Pending {
                    operation: operation.clone(),
                }));
            }
            let saved = self
                .session
                .with_token(&self.token, |uid| get(&store, uid))?;
            if matches!(action, Status)
                || saved
                    .as_ref()
                    .is_some_and(|record| record.phase == Phase::Uncertain)
            {
                return Ok(Admission::Return(saved.map_or(Idle, Record::view)));
            }
            let now = Instant::now();
            let Some(observed) = self.read_all.observed.as_ref().filter(|observed| {
                self.drawer
                    && self.view.counts.as_ref().is_some_and(has_unread)
                    && now < observed.observed + INTENT_TTL
                    && observed.scope.verify(&self.session, &self.token).is_ok()
            }) else {
                return Ok(Admission::Return(Unavailable));
            };
            let intent = Uuid::new_v4().simple().to_string();
            self.read_all.intent = Some(Intent {
                id: intent.clone(),
                expires: observed.observed + INTENT_TTL,
            });
            return Ok(Admission::Return(Ready { intent }));
        }
        if !self.drawer {
            return Err(Error::StaleResponse);
        }
        if self.read_all.work.is_some() {
            return Err(Error::MutationBusy);
        }
        let (operation, task) = match action {
            Confirm { intent } => {
                let pending = self
                    .read_all
                    .intent
                    .take_if(|pending| pending.id == intent)
                    .filter(|pending| pending.expires > Instant::now())
                    .ok_or(Error::StaleResponse)?;
                let observed = self.read_all.observed.take().ok_or(Error::StaleResponse)?;
                observed.scope.verify(&self.session, &self.token)?;
                let operation = Uuid::new_v4().simple().to_string();
                (
                    operation,
                    Task::Write {
                        scope: observed.scope,
                        expires: pending.expires,
                    },
                )
            }
            Check { operation } if valid_id(&operation) => (
                operation,
                Task::Check(Scope::capture(&self.session, &self.token)?),
            ),
            _ => return Err(Error::StaleResponse),
        };
        let uid = self.token.uid.as_deref().ok_or(Error::StaleResponse)?;
        let lock = OperationLock::acquire(store.directory(), JOURNAL, uid)?;
        let record = self.session.with_token(&self.token, |uid| match &task {
            Task::Write { .. } => admit(&store, uid, &operation),
            Task::Check(_) => get(&store, uid)?
                .filter(|record| record.operation == operation)
                .ok_or(Error::StaleResponse),
        })?;
        if record.operation != operation || record.phase != Phase::Uncertain {
            return Ok(Admission::Return(record.view()));
        }
        Ok(Admission::Start(Start {
            store,
            lock,
            record,
            task,
        }))
    }

    pub(super) fn finish_read_all(&mut self, result: WorkResult) -> Completion {
        self.read_all.work = None;
        self.read_all.operation = None;
        self.view.count_read_in_flight = false;
        let WorkResult {
            token,
            mut record,
            store,
            _lock,
            reply,
            outcome,
        } = result;
        let result = self
            .session
            .with_token(&token, |uid| match outcome {
                Outcome::Write(phase) => {
                    if phase != Phase::Uncertain {
                        record.phase = phase;
                        settle(&store, uid, &record)?;
                    }
                    if record.phase == Phase::Confirmed {
                        self.view.counts = Some(zero_counts());
                        self.view.last_count_read_failed = false;
                    }
                    Ok(record.view())
                }
                Outcome::Check(Ok(mut sample)) if sample.scope.is_some() => {
                    let cleared = sample.counts == zero_counts();
                    if cleared {
                        record.phase = Phase::CheckedClear;
                        settle(&store, uid, &record)?;
                    }
                    self.read_all.observe(Some(&mut sample));
                    self.view.counts = Some(sample.counts);
                    self.view.last_count_read_failed = false;
                    Ok(if cleared {
                        record.view()
                    } else {
                        MessageReadAllState::CheckInconclusive {
                            operation: record.operation,
                        }
                    })
                }
                Outcome::Check(_) => Ok(MessageReadAllState::CheckUnavailable {
                    operation: record.operation,
                }),
            })
            .map(|state| self.read_all_data(state));
        (reply, result)
    }
}

enum Admission {
    Return(MessageReadAllState),
    Start(Start),
}

struct Start {
    store: Arc<StateStore>,
    lock: OperationLock,
    record: Record,
    task: Task,
}

enum Task {
    Write { scope: Scope, expires: Instant },
    Check(Scope),
}

impl Start {
    fn run(self, session: Arc<AccountSession>, token: SessionToken, reply: Reply) -> Work {
        Box::pin(async move {
            let outcome = match self.task {
                Task::Write { scope, expires } => {
                    let dispatched = AtomicBool::new(false);
                    let permission = || {
                        if Instant::now() >= expires {
                            return Err(Error::StaleResponse);
                        }
                        scope.verify(&session, &token)
                    };
                    let result = tokio::time::timeout(
                        REQUEST_TIMEOUT,
                        scope.api.request_message_read_all(
                            &scope.revision,
                            &permission,
                            &dispatched,
                        ),
                    )
                    .await;
                    Outcome::Write(if !dispatched.load(Ordering::Acquire) {
                        Phase::NotSent
                    } else if result.is_ok_and(|result| {
                        result.is_ok_and(|reply| reply["code"].as_i64() == Some(200))
                    }) && scope.verify(&session, &token).is_ok()
                    {
                        Phase::Confirmed
                    } else {
                        Phase::Uncertain
                    })
                }
                Task::Check(scope) => {
                    let result = async {
                        scope.verify(&session, &token)?;
                        let counts = read_counts(&session, &token).await?;
                        scope.verify(&session, &token)?;
                        Ok(CountSample {
                            counts,
                            scope: Some(scope),
                        })
                    };
                    Outcome::Check(
                        tokio::time::timeout(REQUEST_TIMEOUT, result)
                            .await
                            .unwrap_or(Err(Error::State("message read-all check timed out"))),
                    )
                }
            };
            WorkResult {
                token,
                record: self.record,
                store: self.store,
                _lock: self.lock,
                reply,
                outcome,
            }
        })
    }
}
