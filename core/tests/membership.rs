mod support;
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    membership::{
        MembershipManager, OfflineDenial, OfflineSource, Quality,
        clock::{ClockReading, MembershipClock},
        membership_state, offline_access,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use support::{Reply, ScriptApi, gate};

struct Clock(Mutex<ClockReading>);
impl MembershipClock for Clock {
    fn now(&self) -> Result<ClockReading> {
        Ok(*self.0.lock().unwrap())
    }
}
fn clock() -> Arc<Clock> {
    Arc::new(Clock(Mutex::new(ClockReading {
        wall_seconds: 1000.0,
        monotonic_seconds: 40.0,
    })))
}
fn response(now: f64, expiry: f64) -> Reply {
    json!({"code":200,"data":{"userId":"1","now":now,"associator":{"expireTime":expiry,"vipLevel":7}}}).into()
}
async fn session(
    store: Arc<StateStore>,
    replies: impl IntoIterator<Item = Reply>,
) -> (Arc<AccountSession>, Arc<ScriptApi>) {
    let mut responses = vec![json!({"code":200,"profile":{"userId":1}}).into()];
    responses.extend(replies);
    let api = Arc::new(ScriptApi::new(responses));
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session.refresh().await.unwrap();
    (session, api)
}
fn equal_numbers(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64()),
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (key, value) in b {
                equal_numbers(&a[key], value);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}

#[test]
fn membership_and_offline_decisions_match_executed_official_modules() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/membership.json")).unwrap();
    for row in fixture["membership"].as_array().unwrap() {
        let state = membership_state(&row["data"], row["now"].as_f64().unwrap()).unwrap();
        equal_numbers(&serde_json::to_value(state).unwrap(), &row["expected"]);
    }
    for row in fixture["offline"].as_array().unwrap() {
        let state = membership_state(&row["data"], row["now"].as_f64().unwrap()).unwrap();
        let quality = Quality::parse(row["level"].as_str().unwrap()).unwrap();
        let source = if row["source"] == "downloaded" {
            OfflineSource::Downloaded {
                complete: row["complete"].as_bool().unwrap(),
            }
        } else {
            OfflineSource::Imported
        };
        let decision = offline_access(
            Some(&row["privilege"]),
            quality,
            &state,
            source,
            row["encrypted"].as_bool().unwrap(),
        )
        .unwrap();
        assert_eq!(
            decision.can_play, row["expected"],
            "source={} level={} privilege={} membership={}",
            row["source"], row["level"], row["privilege"], row["data"]
        );
    }
    for row in fixture["qualities"].as_array().unwrap() {
        let quality = Quality::parse(row[0].as_str().unwrap()).unwrap();
        assert_eq!(quality.bitrate(), row[1].as_u64().unwrap() as u32);
        assert_eq!(Quality::from_bitrate(quality.bitrate()).unwrap(), quality);
        assert_eq!(quality.as_str(), row[0]);
    }
    assert!(Quality::parse("none").is_err());
    assert!(Quality::from_bitrate(0).is_err());
}

#[tokio::test]
async fn monotonic_expiry_restart_clock_guard_and_explicit_online_renewal() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (session, _) = session(
        store.clone(),
        [
            response(2000000.0, 2001000.0),
            response(2002000.0, 2010000.0),
        ],
    )
    .await;
    let clock = clock();
    let manager = MembershipManager::with_clock(session.clone(), clock.clone()).unwrap();
    let first = manager.refresh().await.unwrap();
    assert!(first.state.is_vinyl_vip);
    assert_eq!(first.state.vinyl_vip_level, 7);
    assert_eq!(first.refresh_after_ms, Some(2000.0));
    clock.0.lock().unwrap().monotonic_seconds += 1.0;
    assert!(!manager.snapshot().unwrap().state.is_vinyl_vip);
    clock.0.lock().unwrap().wall_seconds += 2.0;
    let reopened = MembershipManager::with_clock(session.clone(), clock.clone()).unwrap();
    assert!(!reopened.snapshot().unwrap().state.is_vinyl_vip);
    clock.0.lock().unwrap().wall_seconds = 900.0;
    assert!(matches!(
        MembershipManager::with_clock(session.clone(), clock.clone())
            .unwrap()
            .snapshot(),
        Err(Error::Response(_))
    ));
    assert!(reopened.refresh().await.unwrap().state.is_vinyl_vip);
    session.detach().unwrap();
    assert!(matches!(reopened.snapshot(), Err(Error::StaleResponse)));
}

#[tokio::test]
async fn refresh_tick_only_contacts_service_after_an_entitlement_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (session, api) = session(
        store,
        [
            response(2000000.0, 2001000.0),
            response(2002000.0, 2001000.0),
        ],
    )
    .await;
    let clock = clock();
    let manager = MembershipManager::with_clock(session, clock.clone()).unwrap();
    manager.refresh().await.unwrap();
    assert!(manager.refresh_if_due().await.unwrap().state.is_vinyl_vip);
    assert_eq!(api.calls().len(), 2);
    clock.0.lock().unwrap().monotonic_seconds += 2.0;
    assert!(!manager.refresh_if_due().await.unwrap().state.is_vinyl_vip);
    assert!(!manager.refresh_if_due().await.unwrap().state.is_vinyl_vip);
    assert_eq!(api.calls().len(), 3);
}

#[tokio::test]
async fn late_membership_response_cannot_cross_account_generation_or_enter_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (pending, release) = gate();
    let (session, api) = session(store.clone(), [pending]).await;
    let manager = Arc::new(MembershipManager::new(session.clone()).unwrap());
    let work = {
        let manager = manager.clone();
        tokio::spawn(async move { manager.refresh().await })
    };
    api.wait_for_calls(2).await;
    session.detach().unwrap();
    assert!(
        release
            .send(Ok(json!({"code":200,"data":{"userId":"1","now":2000000}})))
            .is_ok()
    );
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert!(store.values("1", "membership").unwrap().is_empty());
}

#[tokio::test]
async fn invalid_server_timestamp_other_uid_and_persistence_failure_do_not_publish_entitlements() {
    for data in [
        json!({"now":true}),
        json!({"now":0}),
        json!({"now":2000000,"userId":"2"}),
        json!({"now":2000000,"associator":{"expireTime":true}}),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(tmp.path()).unwrap());
        let (session, _) = session(store.clone(), [json!({"code":200,"data":data}).into()]).await;
        assert!(
            MembershipManager::new(session)
                .unwrap()
                .refresh()
                .await
                .is_err()
        );
        assert!(store.values("1", "membership").unwrap().is_empty());
    }
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (session, _) = session(
        store.clone(),
        [
            response(2000000.0, 2001000.0),
            response(2000000.0, 2010000.0),
        ],
    )
    .await;
    let manager = MembershipManager::with_clock(session, clock()).unwrap();
    manager.refresh().await.unwrap();
    let before = store.get("1", "membership", "current").unwrap();
    let db = rusqlite::Connection::open(tmp.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_membership BEFORE INSERT ON state WHEN NEW.kind='membership' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").unwrap();
    assert!(matches!(manager.refresh().await, Err(Error::Storage(_))));
    assert_eq!(
        manager.snapshot().unwrap().state.vinyl_vip_expire_time,
        2001000.0
    );
    assert_eq!(store.get("1", "membership", "current").unwrap(), before);
}

#[test]
fn offline_denial_reasons_plain_audio_and_invalid_privileges_are_explicit() {
    let state = membership_state(&json!({}), 1000.0).unwrap();
    assert_eq!(
        offline_access(
            None,
            Quality::Lossless,
            &state,
            OfflineSource::Imported,
            true
        )
        .unwrap()
        .reason,
        Some(OfflineDenial::MissingPrivilege)
    );
    assert_eq!(
        offline_access(
            Some(&json!({"fee":1})),
            Quality::Sky,
            &state,
            OfflineSource::Downloaded { complete: true },
            true
        )
        .unwrap()
        .reason,
        Some(OfflineDenial::Svip)
    );
    assert_eq!(
        offline_access(
            Some(&json!({"fee":1,"flag":8192})),
            Quality::Lossless,
            &state,
            OfflineSource::Imported,
            true
        )
        .unwrap()
        .reason,
        Some(OfflineDenial::Album)
    );
    assert!(
        offline_access(
            None,
            Quality::Lossless,
            &state,
            OfflineSource::Imported,
            false
        )
        .unwrap()
        .can_play
    );
    assert!(
        offline_access(
            Some(&json!({"fee":true})),
            Quality::Lossless,
            &state,
            OfflineSource::Imported,
            true
        )
        .is_err()
    );
    assert!(membership_state(&json!({}), f64::NAN).is_err());
}
