use cumulus_core::{
    Error,
    account::AccountSession,
    playback::quality::{
        QualityRight,
        trials::{QualityTrial, QualityTrialPhase, QualityTrials, TrialConsent},
    },
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};
use tokio::sync::Notify;

#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<(String, Value)>>,
    state: AtomicU8, // eligible, active, denied, granted
    mode: AtomicU8,  // success, lost response, false receipt, rejected, held
    offer_ms: AtomicU64,
    read_failed: AtomicBool,
    entered: Notify,
    release: Notify,
}
impl Backend {
    fn writes(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("/auth/set"))
            .count()
    }
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            match path {
                "/api/vipauth/app/auth/query" => {
                    if self.read_failed.load(Ordering::SeqCst) {
                        return Err(Error::State("synthetic read failure"));
                    }
                    let state = self.state.load(Ordering::SeqCst);
                    Ok(
                        json!({"code":200,"data":[{"resourceId":0,"resourceType":payload["resourceTypeList"],
                        "canUse":state != 2,"userSrc":if state==3 {1}else{0},"trialStatus":if state==1 {2}else{1},
                        "leftTime":if state==0 {self.offer_ms.load(Ordering::SeqCst)}else{1000}}]}),
                    )
                }
                "/api/vipauth/app/auth/set" => {
                    self.entered.notify_one();
                    match self.mode.load(Ordering::SeqCst) {
                        1 => {
                            self.state.store(1, Ordering::SeqCst);
                            Err(Error::State("synthetic lost response"))
                        }
                        2 => Ok(json!({"code":200,"data":false})),
                        3 => Ok(json!({"code":403})),
                        mode => {
                            if mode == 4 {
                                self.release.notified().await;
                            }
                            self.state.store(1, Ordering::SeqCst);
                            if mode == 5 {
                                self.read_failed.store(true, Ordering::SeqCst);
                            }
                            Ok(json!({"code":200,"data":true}))
                        }
                    }
                }
                _ => panic!("unexpected quality trial endpoint {path}"),
            }
        })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    Arc<StateStore>,
    QualityTrials,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let backend = Arc::new(Backend::default());
    backend.offer_ms.store(60000, Ordering::SeqCst);
    let session = Arc::new(AccountSession::new(backend.clone(), Some(store.clone())));
    session
        .adopt(
            backend.clone(),
            json!({"userId":9}),
            &session.token().unwrap(),
        )
        .unwrap();
    let trials = QualityTrials::new(session.clone()).unwrap();
    (dir, backend, session, store, trials)
}
const RIGHT: QualityRight = QualityRight::JyMaster;
#[tokio::test]
async fn explicit_consent_and_the_reviewed_offer_are_required_before_a_quota_write() {
    let (_dir, backend, session, _store, trials) = fixture();
    let token = session.token().unwrap();
    assert!(
        trials
            .start_for(&token, RIGHT, Some(60000), TrialConsent::DoNotStart)
            .await
            .is_err()
    );
    assert!(backend.calls.lock().unwrap().is_empty());
    backend.offer_ms.store(30000, Ordering::SeqCst);
    assert!(matches!(
        trials
            .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
            .await,
        Err(Error::State("quality trial offer changed"))
    ));
    assert_eq!(backend.writes(), 0);
    assert!(trials.record_for(&token, RIGHT).unwrap().is_none());
}
#[tokio::test]
async fn acknowledged_start_requires_an_active_read_and_is_durable_without_automatic_replay() {
    let (dir, backend, session, _store, trials) = fixture();
    let token = session.token().unwrap();
    let record = trials
        .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
        .await
        .unwrap();
    assert!(record.acknowledged && record.can_use());
    assert_eq!(record.phase, QualityTrialPhase::Active);
    assert_eq!(backend.writes(), 1);
    let saved: QualityTrial = serde_json::from_value(
        StateStore::open(dir.path())
            .unwrap()
            .get("9", "quality-trials-v1", "jymaster")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved.operation, record.operation);
    let reopen = QualityTrials::new(session.clone()).unwrap();
    assert!(matches!(
        reopen
            .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
            .await,
        Err(Error::MutationUncertain)
    ));
    assert!(reopen.observe_for(&token, RIGHT).await.unwrap().can_use());
    assert_eq!(backend.writes(), 1);
    assert!(
        backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|(p, v)| if p.ends_with("/auth/set") {
                v == &json!({"resourceId":0,"resourceType":31})
            } else {
                v == &json!({"resourceIdList":0,"resourceTypeList":31})
            })
    );
}
#[tokio::test]
async fn a_lost_receipt_can_be_reconciled_but_never_fabricates_an_acknowledgement() {
    let (_dir, backend, session, _store, trials) = fixture();
    let token = session.token().unwrap();
    backend.mode.store(1, Ordering::SeqCst);
    let record = trials
        .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
        .await
        .unwrap();
    assert_eq!(record.phase, QualityTrialPhase::Uncertain);
    assert!(!record.acknowledged);
    let observed = trials.observe_for(&token, RIGHT).await.unwrap();
    assert!(observed.can_use() && !observed.acknowledged);
    assert_eq!(observed.operation, record.operation);
    assert_eq!(backend.writes(), 1);
}
#[tokio::test]
async fn an_eligible_or_denied_read_does_not_prove_a_lost_write_cannot_commit_later() {
    let (_dir, backend, session, _store, trials) = fixture();
    let token = session.token().unwrap();
    backend.mode.store(2, Ordering::SeqCst);
    assert_eq!(
        trials
            .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
            .await
            .unwrap()
            .phase,
        QualityTrialPhase::Uncertain
    );
    for state in [0, 2] {
        backend.state.store(state, Ordering::SeqCst);
        assert_eq!(
            trials.observe_for(&token, RIGHT).await.unwrap().phase,
            QualityTrialPhase::Uncertain
        );
        assert!(matches!(
            trials
                .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
                .await,
            Err(Error::MutationUncertain)
        ));
    }
    assert_eq!(backend.writes(), 1);
}
#[tokio::test]
async fn expired_records_allow_a_new_explicit_trial_and_archive_the_prior_operation() {
    let (_dir, backend, session, store, trials) = fixture();
    let token = session.token().unwrap();
    let old = trials
        .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
        .await
        .unwrap();
    backend.state.store(2, Ordering::SeqCst);
    assert_eq!(
        trials.observe_for(&token, RIGHT).await.unwrap().phase,
        QualityTrialPhase::Expired
    );
    backend.state.store(0, Ordering::SeqCst);
    let next = trials
        .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
        .await
        .unwrap();
    assert_ne!(next.operation, old.operation);
    assert_eq!(backend.writes(), 2);
    assert_eq!(
        store
            .get("9", "quality-trial-history-v1", &old.operation)
            .unwrap()
            .unwrap()["phase"],
        "expired"
    );
}
#[tokio::test]
async fn adoption_reads_current_rights_without_spending_another_trial() {
    let (_dir, backend, session, _store, trials) = fixture();
    let token = session.token().unwrap();
    assert!(trials.adopt_for(&token, RIGHT).await.is_err());
    backend.state.store(1, Ordering::SeqCst);
    let adopted = trials.adopt_for(&token, RIGHT).await.unwrap();
    assert!(adopted.can_use() && !adopted.acknowledged);
    assert_eq!(backend.writes(), 0);
    backend.read_failed.store(true, Ordering::SeqCst);
    let failed = trials.observe_for(&token, RIGHT).await.unwrap();
    assert_eq!(failed.phase, QualityTrialPhase::Active);
    assert!(!failed.can_use() && failed.read_failed);
    backend.read_failed.store(false, Ordering::SeqCst);
    assert!(trials.observe_for(&token, RIGHT).await.unwrap().can_use());
    assert_eq!(backend.writes(), 0);
}
#[tokio::test]
async fn cancellation_leaves_a_durable_sending_record_and_reconciliation_never_resends_it() {
    let (_dir, backend, session, _store, trials) = fixture();
    let token = session.token().unwrap();
    backend.mode.store(4, Ordering::SeqCst);
    let pending = tokio::spawn({
        let trials = trials.clone();
        let token = token.clone();
        async move {
            trials
                .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
                .await
        }
    });
    backend.entered.notified().await;
    assert_eq!(
        trials.record_for(&token, RIGHT).unwrap().unwrap().phase,
        QualityTrialPhase::Sending
    );
    assert!(matches!(
        trials.observe_for(&token, RIGHT).await,
        Err(Error::MutationBusy)
    ));
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert_eq!(
        trials.observe_for(&token, RIGHT).await.unwrap().phase,
        QualityTrialPhase::Uncertain
    );
    assert_eq!(backend.writes(), 1);
}
#[tokio::test(start_paused = true)]
async fn write_timeout_is_bounded_and_does_not_repeat_quota_consumption() {
    let (_dir, backend, session, _store, trials) = fixture();
    let token = session.token().unwrap();
    backend.mode.store(4, Ordering::SeqCst);
    let before = tokio::time::Instant::now();
    let record = trials
        .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
        .await
        .unwrap();
    assert_eq!(record.phase, QualityTrialPhase::Uncertain);
    assert_eq!(before.elapsed(), std::time::Duration::from_secs(8));
    assert_eq!(backend.writes(), 1);
}
#[tokio::test]
async fn a_late_positive_receipt_is_saved_only_in_the_original_account() {
    let (_dir, backend, session, store, trials) = fixture();
    let token = session.token().unwrap();
    backend.mode.store(4, Ordering::SeqCst);
    let pending = tokio::spawn({
        let trials = trials.clone();
        async move {
            trials
                .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
                .await
        }
    });
    backend.entered.notified().await;
    session
        .adopt(
            backend.clone(),
            json!({"userId":10}),
            &session.token().unwrap(),
        )
        .unwrap();
    backend.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    let old = store
        .get("9", "quality-trials-v1", "jymaster")
        .unwrap()
        .unwrap();
    assert_eq!(old["phase"], "acknowledged");
    assert_eq!(old["acknowledged"], true);
    assert!(
        trials
            .records_for(&session.token().unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(backend.writes(), 1);
}
#[tokio::test]
async fn malformed_journals_are_preserved_and_cannot_be_overwritten_by_a_new_start() {
    let (_dir, backend, session, store, trials) = fixture();
    let token = session.token().unwrap();
    let malformed = json!({"right":"jymaster","operation":"bad","revision":1,"phase":"active","acknowledged":true,"observed":null,"read_failed":false});
    store
        .put("9", "quality-trials-v1", "jymaster", &malformed)
        .unwrap();
    assert!(
        trials
            .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
            .await
            .is_err()
    );
    assert_eq!(
        store.get("9", "quality-trials-v1", "jymaster").unwrap(),
        Some(malformed)
    );
    assert_eq!(backend.writes(), 0);
}

#[tokio::test]
async fn a_positive_receipt_without_a_successful_rights_read_does_not_authorize_playback() {
    let (_dir, backend, session, _store, trials) = fixture();
    let token = session.token().unwrap();
    backend.mode.store(5, Ordering::SeqCst);
    let pending = trials
        .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
        .await
        .unwrap();
    assert!(pending.acknowledged && pending.read_failed && !pending.can_use());
    assert_eq!(pending.phase, QualityTrialPhase::Acknowledged);
    backend.read_failed.store(false, Ordering::SeqCst);
    assert!(trials.observe_for(&token, RIGHT).await.unwrap().can_use());
    assert_eq!(backend.writes(), 1);
}

#[tokio::test]
async fn a_receipt_storage_failure_keeps_sending_uncertain_and_cannot_replay_start() {
    let (_dir, backend, session, store, trials) = fixture();
    let token = session.token().unwrap();
    // Permit Sending, reject only subsequent receipts/observations.
    rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap().execute_batch(
        "CREATE TRIGGER deny_quality_receipt BEFORE INSERT ON state WHEN NEW.kind='quality-trials-v1' AND json_extract(NEW.value,'$.revision')>1 BEGIN SELECT RAISE(FAIL,'synthetic receipt storage failure'); END;"
    ).unwrap();
    assert!(matches!(
        trials
            .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
            .await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        trials.record_for(&token, RIGHT).unwrap().unwrap().phase,
        QualityTrialPhase::Sending
    );
    assert!(matches!(
        trials
            .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
            .await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(backend.writes(), 1);
}

#[tokio::test]
async fn an_explicit_service_rejection_is_kept_distinct_from_a_lost_response() {
    let (_dir, backend, session, store, trials) = fixture();
    let token = session.token().unwrap();
    backend.mode.store(3, Ordering::SeqCst);
    let error = trials
        .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
        .await
        .unwrap_err();
    assert!(error.is_service_code(403));
    let denied = trials.record_for(&token, RIGHT).unwrap().unwrap();
    assert_eq!(denied.phase, QualityTrialPhase::Rejected);
    assert!(!denied.acknowledged);
    assert_eq!(backend.writes(), 1);
    backend.mode.store(0, Ordering::SeqCst);
    let next = trials
        .start_for(&token, RIGHT, Some(60000), TrialConsent::ExplicitStart)
        .await
        .unwrap();
    assert!(next.can_use());
    assert_ne!(next.operation, denied.operation);
    assert_eq!(
        store
            .get("9", "quality-trial-history-v1", &denied.operation)
            .unwrap()
            .unwrap()["phase"],
        "rejected"
    );
    assert_eq!(backend.writes(), 2);
}
