use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials, settings::SettingsPhase},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    relay::{RelayService, RelaySettings},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;
#[derive(Clone, Copy)]
enum Mode {
    Apply,
    Reject,
    LostApplied,
    LostUnapplied,
    AckUnapplied,
    ReadFails,
}
struct Backend {
    device: Mutex<Option<String>>,
    enabled: Mutex<Option<bool>>,
    mode: Mutex<Mode>,
    eligible: AtomicBool,
    writes: AtomicUsize,
    fail_read: AtomicBool,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
    targets: Mutex<Vec<String>>,
}
impl Default for Backend {
    fn default() -> Self {
        Self {
            device: Mutex::new(Some("PRIVATE-local-device".into())),
            enabled: Mutex::new(Some(false)),
            mode: Mutex::new(Mode::Apply),
            eligible: AtomicBool::new(true),
            writes: AtomicUsize::new(0),
            fail_read: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            started: Notify::new(),
            release: Notify::new(),
            targets: Mutex::default(),
        }
    }
}
impl Api for Backend {
    fn local_device_id(&self) -> Result<Option<String>> {
        Ok(self.device.lock().unwrap().clone())
    }
    fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            Ok(match path {
                "/api/rtrs/abt/front/expinfo/list" => {
                    json!({"code":200,"data":[{"expName":"device_connect","expGroupName":if self.eligible.load(Ordering::SeqCst){"t2"}else{"control"}}]})
                }
                "/api/relay/config/get" => {
                    json!({"code":200,"data":{"enable":self.enabled.lock().unwrap().unwrap_or(false)}})
                }
                "/api/link/position/show/resource" => {
                    json!({"code":200,"data":{"commonResourceList":[]}})
                }
                "/api/relay/setting/show" => {
                    assert_eq!(p, json!({"all":false,"settingKey":"RELAY_SWITCH"}));
                    if self.fail_read.load(Ordering::SeqCst) {
                        return Err(Error::Transport("synthetic read failure".into()));
                    }
                    json!({"code":200,"data":{"groups":[{"deviceSettingList":[{"enable":*self.enabled.lock().unwrap()}]}]}})
                }
                "/api/relay/setting/update" => {
                    assert_eq!(p["settingKey"], "RELAY_SWITCH");
                    self.targets
                        .lock()
                        .unwrap()
                        .push(p["targetDeviceId"].as_str().unwrap().into());
                    self.writes.fetch_add(1, Ordering::SeqCst);
                    if self.hold.load(Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    let mode = *self.mode.lock().unwrap();
                    match mode {
                        Mode::Reject => return Ok(json!({"code":403})),
                        Mode::Apply | Mode::LostApplied | Mode::ReadFails => {
                            *self.enabled.lock().unwrap() = p["enable"].as_bool()
                        }
                        _ => (),
                    }
                    if matches!(mode, Mode::LostApplied | Mode::LostUnapplied) {
                        return Err(Error::Transport("synthetic lost reply".into()));
                    }
                    if matches!(mode, Mode::ReadFails) {
                        self.fail_read.store(true, Ordering::SeqCst);
                    }
                    json!({"code":200})
                }
                _ => {
                    return Err(Error::InvalidInput(
                        "unexpected relay setting test endpoint",
                    ));
                }
            })
        })
    }
}
struct World {
    _root: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<Backend>,
    session: Arc<AccountSession>,
    service: Arc<RelaySettings>,
}
impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(Backend::default());
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let service = Arc::new(RelaySettings::new(session.clone()).unwrap());
        Self {
            _root: root,
            store,
            api,
            session,
            service,
        }
    }
    fn token(&self) -> cumulus_core::account::SessionToken {
        self.session.token().unwrap()
    }
    async fn revision(&self) -> String {
        self.service
            .refresh_for(&self.token())
            .await
            .unwrap()
            .revision
    }
    fn writes(&self) -> usize {
        self.api.writes.load(Ordering::SeqCst)
    }
}
fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
#[tokio::test]
async fn switch_binds_local_device_reads_back_and_repeated_intent_is_inspection_only() {
    let w = World::new();
    let revision = w.revision().await;
    let id = new_id();
    let op = w
        .service
        .change_for(&w.token(), &id, &revision, true)
        .await
        .unwrap();
    assert_eq!(op.phase, SettingsPhase::Confirmed);
    assert_eq!(op.before, Some(false));
    assert_eq!(op.observed, Some(true));
    assert_eq!(*w.api.targets.lock().unwrap(), vec!["PRIVATE-local-device"]);
    assert_eq!(
        w.service.snapshot_for(&w.token()).unwrap().unwrap().enabled,
        Some(true)
    );
    let service = RelaySettings::new(w.session.clone()).unwrap();
    assert_eq!(
        service
            .change_for(&w.token(), &id, &revision, true)
            .await
            .unwrap()
            .phase,
        SettingsPhase::Confirmed
    );
    assert!(matches!(
        service.change_for(&w.token(), &id, &revision, false).await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(w.writes(), 1);
}
#[tokio::test]
async fn lost_write_replies_reconcile_without_replay_and_unapplied_requires_explicit_acceptance() {
    for mode in [Mode::LostApplied, Mode::LostUnapplied, Mode::AckUnapplied] {
        let w = World::new();
        let revision = w.revision().await;
        let id = new_id();
        *w.api.mode.lock().unwrap() = mode;
        let _ = w.service.change_for(&w.token(), &id, &revision, true).await;
        assert!(matches!(
            w.service
                .change_for(&w.token(), &new_id(), &revision, true)
                .await,
            Err(Error::MutationUncertain)
        ));
        let reopened = RelaySettings::new(w.session.clone()).unwrap();
        let op = reopened.reconcile_for(&w.token(), &id).await.unwrap();
        if matches!(mode, Mode::LostApplied) {
            assert_eq!(op.phase, SettingsPhase::Confirmed);
        } else {
            assert_eq!(op.phase, SettingsPhase::Uncertain);
            let read_revision = op.read_revision.unwrap();
            let op = reopened
                .accept_current_for(&w.token(), &id, &read_revision)
                .await
                .unwrap();
            assert_eq!(op.phase, SettingsPhase::AcceptedCurrentState);
            assert_eq!(op.observed, Some(false));
        }
        assert_eq!(w.writes(), 1);
    }
}
#[tokio::test]
async fn failed_read_after_ack_remains_pending_until_readonly_recovery() {
    let w = World::new();
    let revision = w.revision().await;
    let id = new_id();
    *w.api.mode.lock().unwrap() = Mode::ReadFails;
    assert!(
        w.service
            .change_for(&w.token(), &id, &revision, true)
            .await
            .is_err()
    );
    let service = RelaySettings::new(w.session.clone()).unwrap();
    assert_eq!(
        service.operations_for(&w.token()).unwrap()[0].phase,
        SettingsPhase::Pending
    );
    assert_eq!(
        service
            .change_for(&w.token(), &id, &revision, true)
            .await
            .unwrap()
            .phase,
        SettingsPhase::Pending
    );
    w.api.fail_read.store(false, Ordering::SeqCst);
    assert_eq!(
        service.reconcile_for(&w.token(), &id).await.unwrap().phase,
        SettingsPhase::Confirmed
    );
    assert_eq!(w.writes(), 1);
}
#[tokio::test]
async fn cancelled_delayed_calls_and_definite_denials_are_not_replayed() {
    let w = World::new();
    let revision = w.revision().await;
    let first = new_id();
    assert_eq!(
        w.service
            .cancel_unseen_for(&w.token(), &first, &revision, true)
            .unwrap()
            .phase,
        SettingsPhase::Cancelled
    );
    assert_eq!(
        w.service
            .change_for(&w.token(), &first, &revision, true)
            .await
            .unwrap()
            .phase,
        SettingsPhase::Cancelled
    );
    assert_eq!(w.writes(), 0);
    let next = new_id();
    *w.api.mode.lock().unwrap() = Mode::Reject;
    assert!(
        w.service
            .change_for(&w.token(), &next, &revision, true)
            .await
            .unwrap_err()
            .is_service_code(403)
    );
    assert_eq!(
        w.service
            .reconcile_for(&w.token(), &next)
            .await
            .unwrap()
            .phase,
        SettingsPhase::Rejected
    );
    assert_eq!(w.writes(), 1);
}
#[tokio::test]
async fn missing_identity_setting_experiment_or_changed_preflight_never_writes() {
    for problem in ["identity", "missing", "experiment", "changed"] {
        let w = World::new();
        if problem == "missing" {
            *w.api.enabled.lock().unwrap() = None;
        }
        let revision = w.revision().await;
        match problem {
            "identity" => *w.api.device.lock().unwrap() = None,
            "experiment" => w.api.eligible.store(false, Ordering::SeqCst),
            "changed" => *w.api.enabled.lock().unwrap() = Some(true),
            _ => (),
        }
        assert!(
            w.service
                .change_for(&w.token(), &new_id(), &revision, true)
                .await
                .is_err(),
            "{problem}"
        );
        assert_eq!(w.writes(), 0);
    }
}
#[tokio::test]
async fn device_replacement_isolated_from_old_pending_intents_and_ids_cannot_be_rebound() {
    let w = World::new();
    let revision = w.revision().await;
    let old = new_id();
    *w.api.mode.lock().unwrap() = Mode::LostUnapplied;
    let _ = w
        .service
        .change_for(&w.token(), &old, &revision, true)
        .await;
    *w.api.device.lock().unwrap() = Some("PRIVATE-new-local-device".into());
    assert!(w.service.snapshot_for(&w.token()).unwrap().is_none());
    assert!(w.service.operations_for(&w.token()).unwrap().is_empty());
    let revision = w.revision().await;
    assert!(matches!(
        w.service
            .change_for(&w.token(), &old, &revision, true)
            .await,
        Err(Error::InvalidInput(_))
    ));
    *w.api.mode.lock().unwrap() = Mode::Apply;
    w.service
        .change_for(&w.token(), &new_id(), &revision, true)
        .await
        .unwrap();
    assert_eq!(w.writes(), 2);
    assert_eq!(
        w.store
            .get("9", "relay-setting-intents-v1", &old)
            .unwrap()
            .unwrap()["phase"],
        "uncertain"
    );
}
#[tokio::test]
async fn setting_and_relay_share_lock_and_account_switch_retains_original_responsibility() {
    let w = World::new();
    let revision = w.revision().await;
    let id = new_id();
    let copy = id.clone();
    w.api.hold.store(true, Ordering::SeqCst);
    let service = w.service.clone();
    let token = w.token();
    let task =
        tokio::spawn(async move { service.change_for(&token, &copy, &revision, true).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), w.api.started.notified())
        .await
        .unwrap();
    assert!(matches!(
        RelayService::new(w.session.clone())
            .unwrap()
            .refresh_for(&w.token())
            .await,
        Err(Error::MutationBusy)
    ));
    w.session
        .adopt(w.api.clone(), json!({"userId":10}), &w.token())
        .unwrap();
    w.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert!(w.service.operations_for(&w.token()).unwrap().is_empty());
    assert_eq!(
        w.store
            .get("9", "relay-setting-intents-v1", &id)
            .unwrap()
            .unwrap()["phase"],
        "uncertain"
    );
}
#[test]
fn production_transport_identity_is_the_same_native_credential_device_used_by_eapi() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let credentials = Arc::new(Credentials::open(store, ApiOrigin::Pc).unwrap());
    let expected = credentials.device_id().unwrap();
    let client = MusicHttpClient::new(credentials, ClientProfile::default()).unwrap();
    assert_eq!(
        client.local_device_id().unwrap().as_deref(),
        Some(expected.as_str())
    );
}

#[tokio::test]
async fn client_projects_setting_state_without_device_identity_and_reinitializes_after_update() {
    use cumulus_core::{
        account::profiles::ProfileRegistry,
        client::{MusicClient, dto::RelaySettingCommand},
    };
    let w = World::new();
    let registry = ProfileRegistry::open(w._root.path(), ApiOrigin::Pc).unwrap();
    let client = MusicClient::with_session(w.session.clone(), registry);
    let epoch = w.token().generation.to_string();
    let page = client.relay_view(&epoch, true).await.unwrap().value;
    assert!(!serde_json::to_string(&page).unwrap().contains("PRIVATE"));
    let setting = page.settings.unwrap();
    assert!(setting.can_change);
    assert_eq!(setting.enabled, Some(false));
    let page = client
        .relay_setting_command(
            &epoch,
            RelaySettingCommand::SettingChange {
                id: new_id(),
                revision: setting.revision,
                enabled: true,
            },
        )
        .await
        .unwrap()
        .value;
    assert_eq!(
        page.availability,
        cumulus_core::relay::RelayAvailability::Available
    );
    let setting = page.settings.unwrap();
    assert_eq!(setting.enabled, Some(true));
    assert_eq!(setting.operations[0].phase, SettingsPhase::Confirmed);
    assert!(!serde_json::to_string(&setting).unwrap().contains("PRIVATE"));
    assert!(serde_json::from_value::<RelaySettingCommand>(json!({"kind":"setting_change","id":new_id(),"revision":setting.revision,"enabled":true,"targetDeviceId":"remote"})).is_err());
}
