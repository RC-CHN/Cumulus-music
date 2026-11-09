//! Native FM chorus maps are independent of the ordinary playback queue.
use super::{FmQueue, FmSource};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    playback::chorus::{self, Points},
    protocol::api::{Api, PlaybackChorusRead},
    storage::StateStore,
};
use serde_json::Value;
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};

/// Only the source's last accepted loadMore revision, never chorus points.
pub(super) const LOAD_REVISION: &str = "fm-chorus-load-v1";
const DEADLINE: Duration = Duration::from_secs(5);
const MAX_LANES: usize = 8;
const MAX_TRACKS: usize = crate::playback::intelligence::MAX_QUEUE_TRACKS;

struct Scope {
    token: SessionToken,
    api: Arc<dyn Api>,
    credentials: String,
}

impl Scope {
    fn check(&self, session: &AccountSession) -> Result<()> {
        session.require_api(&self.token, &self.api)?;
        if self.api.playback_chorus_revision()?.as_deref() != Some(self.credentials.as_str()) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    fn matches(&self, token: &SessionToken, api: &Arc<dyn Api>) -> bool {
        self.token == *token && Arc::ptr_eq(&self.api, api)
    }

    fn same(&self, other: &Self) -> bool {
        self.matches(&other.token, &other.api) && self.credentials == other.credentials
    }
}

/// Records a real load even when there is no supported chorus transport.
pub(super) struct Load(Option<Scope>);

impl Load {
    pub(super) fn capture(session: &AccountSession, token: &SessionToken) -> Self {
        Self((|| {
            token.uid.as_ref()?;
            let api = session.native_api(token).ok()?;
            let credentials = api.observed_playback_chorus_revision()?;
            Some(Scope {
                token: token.clone(),
                api,
                credentials,
            })
        })())
    }
}

pub(crate) struct ChorusReceipt {
    scope: Scope,
    source: FmSource,
    key: String,
    revision: u64,
    done: watch::Receiver<bool>,
}

impl ChorusReceipt {
    pub(crate) async fn wait(&self) -> Result<()> {
        self.done
            .clone()
            .wait_for(|done| *done)
            .await
            .map_err(|_| Error::StaleResponse)?;
        Ok(())
    }
}

struct Lane {
    receipt: Arc<ChorusReceipt>,
    points: Points,
    task: JoinHandle<()>,
}

impl Drop for Lane {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) struct Host {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    lanes: Mutex<VecDeque<Lane>>,
}

impl Host {
    pub(super) fn new(session: Arc<AccountSession>, store: Arc<StateStore>) -> Arc<Self> {
        Arc::new(Self {
            session,
            store,
            lanes: Default::default(),
        })
    }

    pub(super) fn schedule(self: &Arc<Self>, key: String, queue: &FmQueue, load: Load) {
        let Some(scope) = load.0 else { return };
        if queue.tracks.is_empty()
            || queue.tracks.len() > MAX_TRACKS
            || queue.tracks.iter().any(|track| track.id.len() > 32)
        {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let Ok(mut lanes) = self.lanes.lock() else {
            return;
        };
        // The queue transaction releases its account lock before scheduling.
        // A delayed old-owner/Cookie callback must not evict a newer lane.
        if self.session.require_api(&scope.token, &scope.api).is_err()
            || scope.api.observed_playback_chorus_revision().as_deref()
                != Some(scope.credentials.as_str())
        {
            return;
        }
        let mut points = Points::new();
        if let Some(index) = lanes
            .iter()
            .position(|lane| lane.receipt.source == queue.source)
        {
            let current = &lanes[index];
            if current.receipt.scope.same(&scope) && current.receipt.revision >= queue.revision {
                return;
            }
            let mut old = lanes.remove(index).expect("existing FM chorus lane");
            if old.receipt.scope.same(&scope) {
                points = std::mem::take(&mut old.points);
            }
        }
        // Keep points for live members only. M86's request uses the complete
        // current FM list in one fixed FM bucket, independent of URL trial mode.
        let ids: BTreeSet<_> = queue.tracks.iter().map(|track| track.id.as_str()).collect();
        points.retain(|id, _| ids.contains(id.as_str()));
        let batches = queue
            .tracks
            .chunks(PlaybackChorusRead::LIMIT)
            .map(|chunk| {
                chunk
                    .iter()
                    .map(|track| track.song["id"].clone())
                    .collect::<Vec<Value>>()
            })
            .collect::<Vec<_>>();
        let (done, receiver) = watch::channel(false);
        let receipt = Arc::new(ChorusReceipt {
            scope,
            source: queue.source.clone(),
            key,
            revision: queue.revision,
            done: receiver,
        });
        let task = runtime.spawn(run(
            Arc::downgrade(self),
            self.session.clone(),
            receipt.clone(),
            batches,
            done,
        ));
        lanes.push_back(Lane {
            receipt,
            points,
            task,
        });
        while lanes.len() > MAX_LANES {
            lanes.pop_front();
        }
    }

    pub(super) fn receipt(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        source: &FmSource,
    ) -> Result<Option<Arc<ChorusReceipt>>> {
        let receipt = self
            .lanes
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .iter()
            .find(|lane| lane.receipt.source == *source && lane.receipt.scope.matches(token, api))
            .map(|lane| lane.receipt.clone());
        if let Some(receipt) = &receipt {
            self.require_scope(receipt)?;
        }
        Ok(receipt)
    }

    fn require_scope(&self, receipt: &ChorusReceipt) -> Result<()> {
        receipt.scope.check(&self.session)?;
        self.session.with_token(&receipt.scope.token, |owner| {
            let current = self.store.get(owner, LOAD_REVISION, &receipt.key)?;
            if current.as_ref().and_then(Value::as_u64) != Some(receipt.revision) {
                return Err(Error::StaleResponse);
            }
            Ok(())
        })
    }

    fn require(&self, receipt: &Arc<ChorusReceipt>) -> Result<()> {
        self.require_scope(receipt)?;
        let lanes = self.lanes.lock().map_err(|_| Error::LockPoisoned)?;
        if !lanes.iter().any(|lane| Arc::ptr_eq(&lane.receipt, receipt)) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(super) fn point(&self, receipt: &Arc<ChorusReceipt>, id: &str) -> Result<Option<f64>> {
        self.require_scope(receipt)?;
        let lanes = self.lanes.lock().map_err(|_| Error::LockPoisoned)?;
        let lane = lanes
            .iter()
            .find(|lane| Arc::ptr_eq(&lane.receipt, receipt))
            .ok_or(Error::StaleResponse)?;
        Ok(lane.points.get(id).copied())
    }
}

async fn run(
    host: Weak<Host>,
    session: Arc<AccountSession>,
    receipt: Arc<ChorusReceipt>,
    batches: Vec<Vec<Value>>,
    done: watch::Sender<bool>,
) {
    let Ok(mut account) = session.subscribe() else {
        return;
    };
    let permission = || host.upgrade().ok_or(Error::Cancelled)?.require(&receipt);
    let read = chorus::read(
        &receipt.scope.api,
        &receipt.scope.credentials,
        &batches,
        &permission,
    );
    tokio::select! {
        biased;
        _ = account.wait_for(|token| token != &receipt.scope.token) => {}
        result = tokio::time::timeout(DEADLINE, read) => {
            if let Ok(Ok(updates)) = result
                && let Some(host) = host.upgrade()
                && host.require_scope(&receipt).is_ok()
                && let Ok(mut lanes) = host.lanes.lock()
                && let Some(lane) = lanes.iter_mut().find(|lane| Arc::ptr_eq(&lane.receipt, &receipt))
            {
                // No partial result and no cross-source merge. A failed read
                // leaves already observed points for the same live membership.
                chorus::merge(&mut lane.points, updates);
            }
        }
    }
    done.send_replace(true);
}
