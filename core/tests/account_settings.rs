mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, SessionToken, settings::*},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};
struct World {
    root: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<ScriptApi>,
    session: Arc<AccountSession>,
    service: Arc<AccountSettings>,
    token: SessionToken,
}
impl World {
    async fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(ScriptApi::new(
            std::iter::once(json!({"code":200,"profile":{"userId":1}}).into()).chain(replies),
        ));
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session.refresh().await.unwrap();
        let token = session.token().unwrap();
        let service = Arc::new(AccountSettings::new(session.clone()).unwrap());
        Self {
            root,
            store,
            api,
            session,
            service,
            token,
        }
    }
    async fn read(&self) -> SettingsSnapshot {
        self.service.refresh_for(&self.token).await.unwrap()
    }
    fn ops(&self) -> Vec<SettingsMutation> {
        self.service.mutations_for(&self.token).unwrap()
    }
    fn writes(&self) -> usize {
        self.api
            .calls()
            .iter()
            .filter(|c| c.path == "/api/user/setting/update")
            .count()
    }
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn baseline() -> Value {
    json!({"shareSetting":0,"playRecordSetting":0,"personalServiceSetting":0,"allowSubscriptionNotify":true,"allowLikedNotify":true,"allowNewFollowerNotify":true,"allowPeopleCanSeeMyPlayRecord":true,"allowFollowedCanSeeMyPlayRecord":true,"socialPlaylistSetting":0,"nativeOnly":{"token":"synthetic-private"}})
}
fn read(value: Value) -> Reply {
    json!({"code":200,"setting":value}).into()
}
fn change(key: SettingKey, value: SettingValue) -> SettingChange {
    SettingChange { key, value }
}
fn private() -> SettingChange {
    change(SettingKey::PlayRecordSetting, SettingValue::Number(2))
}
fn liked() -> SettingChange {
    change(SettingKey::AllowLikedNotify, SettingValue::Boolean(false))
}
fn lost() -> Reply {
    Reply::Ready(Err(Error::Transport(
        "synthetic lost acknowledgement".into(),
    )))
}

#[tokio::test]
async fn full_linked_readback_narrow_patch_and_exact_idempotent_intent() {
    let before = baseline();
    let mut after = before.clone();
    after["playRecordSetting"] = json!(2);
    after["allowPeopleCanSeeMyPlayRecord"] = json!(false);
    after["allowFollowedCanSeeMyPlayRecord"] = json!(false);
    after["socialPlaylistSetting"] = json!(2);
    let w = World::new([
        read(before.clone()),
        read(before),
        json!({"code":200}).into(),
        read(after.clone()),
    ])
    .await;
    let snapshot = w.read().await;
    let id = id();
    let op = w
        .service
        .change_for(&w.token, &id, &snapshot.revision, private())
        .await
        .unwrap();
    assert_eq!(op.phase, SettingsPhase::Confirmed);
    assert_eq!(op.before, Some(SettingValue::Number(0)));
    assert_eq!(
        w.service.snapshot_for(&w.token).unwrap().unwrap().values,
        after
    );
    assert_eq!(
        w.store.get("1", "resources", "settings").unwrap(),
        Some(after)
    );
    assert_eq!(w.api.calls()[3].payload, json!({"playRecordSetting":2}));
    assert_eq!(
        w.service
            .change_for(&w.token, &id, &snapshot.revision, private())
            .await
            .unwrap()
            .phase,
        SettingsPhase::Confirmed
    );
    assert!(matches!(
        w.service
            .change_for(&w.token, &id, &snapshot.revision, liked())
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(w.api.calls().len(), 5);
    assert_eq!(w.writes(), 1);
}
#[tokio::test]
async fn another_devices_change_updates_cache_and_prevents_overwriting_unseen_state() {
    let before = baseline();
    let mut other = before.clone();
    other["shareSetting"] = json!(1);
    let w = World::new([read(before), read(other.clone())]).await;
    let snapshot = w.read().await;
    assert!(matches!(
        w.service
            .change_for(&w.token, &id(), &snapshot.revision, private())
            .await,
        Err(Error::State("account settings changed"))
    ));
    assert_eq!(
        w.service.snapshot_for(&w.token).unwrap().unwrap().values,
        other
    );
    assert_eq!(w.writes(), 0);
    assert!(w.ops().is_empty());
}
#[tokio::test]
async fn unknown_or_absent_value_and_invalid_patch_are_never_defaulted() {
    for value in [json!(3), json!(1.0), json!("0"), Value::Null, json!(false)] {
        let mut before = baseline();
        before["playRecordSetting"] = value;
        let w = World::new([read(before)]).await;
        let snapshot = w.read().await;
        assert!(matches!(
            w.service
                .change_for(&w.token, &id(), &snapshot.revision, private())
                .await,
            Err(Error::Response(_))
        ));
        assert_eq!(w.api.calls().len(), 2);
        assert_eq!(w.writes(), 0);
    }
    assert!(
        change(SettingKey::ShareSetting, SettingValue::Number(2))
            .validate()
            .is_err()
    );
    assert!(
        change(SettingKey::PersonalServiceSetting, SettingValue::Number(1))
            .validate()
            .is_err()
    );
    assert!(
        change(SettingKey::AllowLikedNotify, SettingValue::Number(0))
            .validate()
            .is_err()
    );
    assert!(
        serde_json::from_value::<SettingChange>(
            json!({"key":"personalizedRecommend","value":true})
        )
        .is_err()
    );
}
#[tokio::test]
async fn already_applied_value_is_a_persistent_noop() {
    let before = baseline();
    let w = World::new([read(before.clone()), read(before)]).await;
    let snapshot = w.read().await;
    let op = w
        .service
        .change_for(
            &w.token,
            &id(),
            &snapshot.revision,
            change(SettingKey::ShareSetting, SettingValue::Number(0)),
        )
        .await
        .unwrap();
    assert_eq!(op.phase, SettingsPhase::Unchanged);
    assert_eq!(w.writes(), 0);
    assert_eq!(w.ops().len(), 1);
}
#[tokio::test]
async fn lost_reply_survives_restart_blocks_all_linked_settings_and_reconciles_without_write() {
    let before = baseline();
    let mut after = before.clone();
    after["playRecordSetting"] = json!(2);
    let w = World::new([read(before.clone()), read(before), lost()]).await;
    let snapshot = w.read().await;
    let op_id = id();
    assert!(
        w.service
            .change_for(&w.token, &op_id, &snapshot.revision, private())
            .await
            .is_err()
    );
    assert_eq!(w.ops()[0].phase, SettingsPhase::Uncertain);
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":1}}).into(),
        read(after),
    ]));
    let session = Arc::new(AccountSession::new(
        api.clone(),
        Some(Arc::new(StateStore::open(w.root.path()).unwrap())),
    ));
    session.refresh().await.unwrap();
    let token = session.token().unwrap();
    let recovered = AccountSettings::new(session).unwrap();
    assert!(matches!(
        recovered
            .change_for(&token, &id(), &snapshot.revision, liked())
            .await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        recovered
            .change_for(&token, &op_id, &snapshot.revision, private())
            .await
            .unwrap()
            .phase,
        SettingsPhase::Uncertain
    );
    assert_eq!(
        recovered.reconcile_for(&token, &op_id).await.unwrap().phase,
        SettingsPhase::Confirmed
    );
    assert_eq!(api.calls().len(), 2);
    assert_eq!(w.writes(), 1);
}
#[tokio::test]
async fn acknowledgement_without_readback_stays_pending_and_cancel_cannot_erase_it() {
    let before = baseline();
    let w = World::new([
        read(before.clone()),
        read(before),
        json!({"code":200}).into(),
        lost(),
    ])
    .await;
    let snapshot = w.read().await;
    let op_id = id();
    assert!(
        w.service
            .change_for(&w.token, &op_id, &snapshot.revision, private())
            .await
            .is_err()
    );
    assert_eq!(w.ops()[0].phase, SettingsPhase::Pending);
    assert_eq!(
        w.service
            .cancel_unseen_for(&w.token, &op_id, &snapshot.revision, private())
            .unwrap()
            .phase,
        SettingsPhase::Pending
    );
    assert!(matches!(
        w.service
            .change_for(&w.token, &id(), &snapshot.revision, liked())
            .await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(w.writes(), 1);
}
#[tokio::test]
async fn explicit_rejection_preserves_confirmed_values_and_does_not_claim_changed_state() {
    let before = baseline();
    let w = World::new([
        read(before.clone()),
        read(before.clone()),
        json!({"code":403}).into(),
    ])
    .await;
    let snapshot = w.read().await;
    assert!(
        w.service
            .change_for(&w.token, &id(), &snapshot.revision, private())
            .await
            .unwrap_err()
            .is_service_code(403)
    );
    let op = w.ops().pop().unwrap();
    assert_eq!(op.phase, SettingsPhase::Rejected);
    assert_eq!(op.service_code, Some(403));
    assert_eq!(
        w.service
            .reconcile_for(&w.token, &op.id)
            .await
            .unwrap()
            .phase,
        SettingsPhase::Rejected
    );
    assert_eq!(
        w.service.snapshot_for(&w.token).unwrap().unwrap().values,
        before
    );
    assert_eq!(w.api.calls().len(), 4);
}
#[tokio::test]
async fn accepting_current_state_requires_prior_reconciliation_and_a_still_matching_full_snapshot()
{
    let before = baseline();
    let mut other = before.clone();
    other["shareSetting"] = json!(1);
    let w = World::new([
        read(before.clone()),
        read(before.clone()),
        lost(),
        read(before),
        read(other.clone()),
        read(other.clone()),
        read(other),
    ])
    .await;
    let snapshot = w.read().await;
    let op_id = id();
    let _ = w
        .service
        .change_for(&w.token, &op_id, &snapshot.revision, private())
        .await;
    assert!(
        w.service
            .accept_current_for(&w.token, &op_id, &snapshot.revision)
            .await
            .is_err()
    );
    assert_eq!(
        w.service
            .reconcile_for(&w.token, &op_id)
            .await
            .unwrap()
            .phase,
        SettingsPhase::Uncertain
    );
    assert!(matches!(
        w.service
            .accept_current_for(&w.token, &op_id, &snapshot.revision)
            .await,
        Err(Error::State("account settings changed"))
    ));
    let op = w.service.reconcile_for(&w.token, &op_id).await.unwrap();
    assert_eq!(
        w.service
            .accept_current_for(&w.token, &op_id, op.read_revision.as_deref().unwrap())
            .await
            .unwrap()
            .phase,
        SettingsPhase::AcceptedCurrentState
    );
    assert_eq!(w.writes(), 1);
}
#[tokio::test]
async fn cancelling_an_unseen_id_fences_a_delayed_original_request() {
    let w = World::new([read(baseline())]).await;
    let snapshot = w.read().await;
    let op_id = id();
    assert_eq!(
        w.service
            .cancel_unseen_for(&w.token, &op_id, &snapshot.revision, private())
            .unwrap()
            .phase,
        SettingsPhase::Cancelled
    );
    assert_eq!(
        w.service
            .change_for(&w.token, &op_id, &snapshot.revision, private())
            .await
            .unwrap()
            .phase,
        SettingsPhase::Cancelled
    );
    assert!(
        w.service
            .cancel_unseen_for(&w.token, &op_id, &snapshot.revision, liked())
            .is_err()
    );
    assert_eq!(w.api.calls().len(), 2);
    assert_eq!(w.writes(), 0);
}
#[tokio::test]
async fn coupled_keys_are_mutually_exclusive_and_account_switch_preserves_original_responsibility()
{
    let (pending, release) = gate();
    let before = baseline();
    let w = World::new([read(before.clone()), read(before), pending]).await;
    let snapshot = w.read().await;
    let op_id = id();
    let work = {
        let service = w.service.clone();
        let token = w.token.clone();
        let op_id = op_id.clone();
        let revision = snapshot.revision.clone();
        tokio::spawn(async move {
            service
                .change_for(&token, &op_id, &revision, private())
                .await
        })
    };
    w.api.wait_for_calls(4).await;
    assert!(matches!(
        w.service
            .change_for(&w.token, &id(), &snapshot.revision, liked())
            .await,
        Err(Error::MutationBusy)
    ));
    let new_api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":2}}).into()
    ]));
    w.session.attach(new_api.clone()).unwrap();
    w.session.refresh().await.unwrap();
    release.send(Ok(json!({"code":200}))).unwrap();
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        w.store
            .get("1", "account-settings-intents-v1", &op_id)
            .unwrap()
            .unwrap()["phase"],
        "uncertain"
    );
    assert!(
        w.service
            .snapshot_for(&w.session.token().unwrap())
            .unwrap()
            .is_none()
    );
    assert!(
        w.service
            .mutations_for(&w.session.token().unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(new_api.calls().len(), 1);
    assert!(matches!(
        w.service.refresh_for(&w.token).await,
        Err(Error::StaleResponse)
    ));
}
#[tokio::test]
async fn journal_failure_prevents_any_remote_write() {
    let before = baseline();
    let w = World::new([read(before.clone()), read(before)]).await;
    let snapshot = w.read().await;
    let db = rusqlite::Connection::open(w.root.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_settings BEFORE INSERT ON state WHEN NEW.kind='account-settings-intents-v1' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").unwrap();
    assert!(matches!(
        w.service
            .change_for(&w.token, &id(), &snapshot.revision, private())
            .await,
        Err(Error::Storage(_))
    ));
    assert_eq!(w.writes(), 0);
}
#[tokio::test]
async fn old_per_field_journal_is_migrated_once_and_still_blocks_other_settings() {
    let before = baseline();
    let w = World::new([read(before.clone()), read(before)]).await;
    w.store.put("1","resource-mutations","settings:allowLikedNotify",&json!({"kind":"settings","key":"allowLikedNotify","before":true,"desired":false,"created_at":1.5,"phase":"uncertain"})).unwrap();
    let snapshot = w.read().await;
    let old = w.ops().pop().unwrap();
    assert_eq!(old.created_at, 1500);
    assert_eq!(old.phase, SettingsPhase::Uncertain);
    assert!(
        w.store
            .get("1", "resource-mutations", "settings:allowLikedNotify")
            .unwrap()
            .is_none()
    );
    assert_eq!(w.ops().len(), 1);
    assert_eq!(w.ops()[0].id, old.id);
    assert!(matches!(
        w.service
            .change_for(&w.token, &id(), &snapshot.revision, private())
            .await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        w.service
            .reconcile_for(&w.token, &old.id)
            .await
            .unwrap()
            .phase,
        SettingsPhase::Uncertain
    );
    assert_eq!(w.writes(), 0);
}
