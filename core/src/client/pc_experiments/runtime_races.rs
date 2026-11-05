//! Deterministic native-capability lifecycle races. No ordinary API grant or
//! network request is available. Signed EAPI is covered by integration tests.
use super::*;
use crate::{
    account::{AccountSession, profiles::ProfileRegistry},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::sync::{Mutex, atomic::AtomicUsize};
use tokio::sync::Notify;

#[derive(Clone, Copy)]
enum CommitAt {
    AfterFinalReadFence,
    DuringResultSubscription,
    AfterResultRevisionSample,
}

struct NativeRace {
    revision: Mutex<String>,
    commits: watch::Sender<String>,
    reads: AtomicUsize,
    completed_stage: AtomicUsize,
    armed: AtomicBool,
    rotated: AtomicBool,
    point: CommitAt,
    read_started: Notify,
}

impl NativeRace {
    fn new(point: CommitAt) -> Self {
        Self {
            revision: Mutex::new("registered-r1".into()),
            commits: watch::channel("registered-r1".into()).0,
            reads: AtomicUsize::new(0),
            completed_stage: AtomicUsize::new(0),
            armed: AtomicBool::new(false),
            rotated: AtomicBool::new(false),
            point,
            read_started: Notify::new(),
        }
    }

    fn rotate(&self) {
        if !self.rotated.swap(true, Ordering::AcqRel) {
            *self.revision.lock().unwrap() = "external-r2".into();
            self.commits.send_replace("external-r2".into());
        }
    }

    async fn wait_for_second_read(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while self.reads.load(Ordering::Acquire) < 2 {
                self.read_started.notified().await;
            }
        })
        .await
        .expect("bound r1 reply or its pending notification was incorrectly accepted as r2");
    }
}

impl Api for NativeRace {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("lifecycle unit fixture cannot make ordinary API requests") })
    }

    fn pc_presentation_credential_revision(&self) -> Result<Option<String>> {
        let observed = self.revision.lock().unwrap().clone();
        if self.reads.load(Ordering::Acquire) == 1 {
            let stage = self.completed_stage.fetch_add(1, Ordering::AcqRel) + 1;
            // After a native group reply the existing commit path checks its
            // revision, snapshots, then checks once more after persistence.
            // Preserve all three old-r1 answers and commit immediately after
            // that last returned check: a real distinct actor can commit at
            // precisely this boundary before the runtime receives the result.
            if stage == 3 {
                match self.point {
                    CommitAt::AfterFinalReadFence => self.rotate(),
                    CommitAt::DuringResultSubscription => self.armed.store(true, Ordering::Release),
                    CommitAt::AfterResultRevisionSample => (),
                }
            } else if stage == 4 && matches!(self.point, CommitAt::AfterResultRevisionSample) {
                self.rotate();
            }
        }
        Ok(Some(observed))
    }

    fn pc_presentation_credential_changes(&self) -> Result<Option<watch::Receiver<String>>> {
        let receiver = self.commits.subscribe();
        if self.armed.swap(false, Ordering::AcqRel) {
            self.rotate();
        }
        Ok(Some(receiver))
    }

    fn pc_anonymous_state(&self) -> Result<PcAnonymousState> {
        Ok(PcAnonymousState::Registered {
            uid: "9001".into(),
            revision: self.revision.lock().unwrap().clone(),
        })
    }

    fn pc_anonymous_store_matches(&self, _: &std::path::Path) -> Result<bool> {
        Ok(true)
    }

    fn pc_presentation_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, PcExperimentGroups> {
        Box::pin(async move {
            if self.revision.lock().unwrap().as_str() != revision {
                return Err(Error::CredentialChanged);
            }
            let previous = self.reads.fetch_add(1, Ordering::AcqRel);
            assert!(
                previous < 2,
                "one external rotation cannot create a third read"
            );
            self.completed_stage.store(0, Ordering::Release);
            self.read_started.notify_one();
            PcExperimentGroups::refreshed(&json!({"code":200,"data":[{
                "expName":"PH-PC-search-result", "expGroupName":if previous == 0 {"t1"} else {"c"}
            }]}))
        })
    }
}

#[tokio::test]
async fn anonymous_result_validates_its_bound_revision_and_cannot_swallow_a_new_commit() {
    for point in [
        CommitAt::AfterFinalReadFence,
        CommitAt::DuringResultSubscription,
        CommitAt::AfterResultRevisionSample,
    ] {
        let root = tempfile::tempdir().unwrap();
        let api = Arc::new(NativeRace::new(point));
        let transport: Arc<dyn Api> = api.clone();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let session = Arc::new(AccountSession::new(
            transport.clone(),
            Some(registry.store()),
        ));
        session
            .clear_current(&session.token().unwrap(), &transport, || Ok(()))
            .unwrap();
        let client = MusicClient::with_session(session, registry);
        let lease = client.start_pc_presentation_runtime().unwrap();
        let mut states = lease.subscribe_status();
        api.wait_for_second_read().await;
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while *states.borrow_and_update() != PcPresentationRuntimeStatus::Ready {
                states.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(api.rotated.load(Ordering::Acquire));
        assert_eq!(api.reads.load(Ordering::Acquire), 2);
        assert!(!client.pc_presentation().unwrap().search_footer);
        tokio::task::yield_now().await;
        assert_eq!(api.reads.load(Ordering::Acquire), 2);
        lease.shutdown().await.unwrap();
    }
}
