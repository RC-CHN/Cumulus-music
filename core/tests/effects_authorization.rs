mod support;
use cumulus_core::{
    Error,
    account::AccountSession,
    effects::{
        api::EffectsApi,
        authorization::{EffectAuthorizations, LeasePhase, ReleaseRetry, TrialConsent},
        catalog::EffectId,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};
fn id() -> EffectId {
    EffectId::new(300).unwrap()
}
fn auth(allowed: bool, kind: u32) -> Reply {
    json!({"code":200,"data":[{"interestsId":300,"interestsType":127,"authResp":{"interestsVipType":6,"authResult":{"canUse":allowed,"canUseType":kind}}}]}).into()
}
fn recycle(value: bool) -> Reply {
    json!({"code":200,"data":{"127":[{"interestsId":300,"canRecycle":value}]}}).into()
}
fn ack() -> Reply {
    json!({"code":200,"data":true}).into()
}
fn start() -> Vec<Reply> {
    vec![auth(true, 11), ack(), auth(true, 11), recycle(false)]
}
fn world(api: Arc<ScriptApi>, store: Arc<StateStore>) -> Arc<AccountSession> {
    let s = Arc::new(AccountSession::new(api.clone(), Some(store)));
    s.adopt(api, json!({"userId":1}), &s.token().unwrap())
        .unwrap();
    s
}
fn writes(api: &ScriptApi) -> Vec<String> {
    api.calls()
        .into_iter()
        .filter(|c| c.path.ends_with("/set") || c.path.ends_with("/stop"))
        .map(|c| c.path)
        .collect()
}

#[tokio::test]
async fn request_builders_match_twenty_one_direct_official_requests() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/effects-authorization-official.json")).unwrap();
    let api =
        Arc::new(ScriptApi::new((0..7).flat_map(|_| {
            [ack(), ack(), json!({"code":200,"data":{}}).into()]
        })));
    let effects = EffectsApi::new(api.clone());
    for row in fixture["cases"].as_array().unwrap() {
        let id = EffectId::parse(&row["id"]).unwrap();
        effects.register_interest(id).await.unwrap();
        effects.release_interest(id).await.unwrap();
        assert!(effects.recyclable(id).await.unwrap().is_none());
    }
    let expected = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| [&row["begin"], &row["end"], &row["recycle"]])
        .collect::<Vec<_>>();
    assert_eq!(api.calls().len(), 21);
    for (call, expected) in api.calls().iter().zip(expected) {
        assert_eq!(call.path, expected["path"]);
        assert_eq!(call.payload, expected["payload"]);
        assert!(!call.cache);
    }
}
#[tokio::test]
async fn recycle_results_distinguish_missing_false_true_and_malformed() {
    for (value, expected) in [
        (json!({}), Some(None)),
        (json!({"127":[]}), Some(None)),
        (
            json!({"127":[{"interestsId":301,"canRecycle":false}]}),
            Some(None),
        ),
        (
            json!({"127":[{"interestsId":"300","canRecycle":false}]}),
            Some(Some(false)),
        ),
        (
            json!({"127":[{"interestsId":300,"canRecycle":true}]}),
            Some(Some(true)),
        ),
        (json!(null), None),
        (json!({"127":{}}), None),
        (
            json!({"127":[{"interestsId":300,"canRecycle":"false"}]}),
            None,
        ),
        (
            json!({"127":[{"interestsId":300,"canRecycle":false},{"interestsId":300,"canRecycle":true}]}),
            None,
        ),
    ] {
        let api = EffectsApi::new(Arc::new(ScriptApi::new([
            json!({"code":200,"data":value}).into()
        ])));
        let result = api.recyclable(id()).await;
        match expected {
            Some(expected) => assert_eq!(result.unwrap(), expected),
            None => assert!(result.is_err()),
        }
    }
}
#[tokio::test]
async fn start_reuse_reopen_release_and_new_selection_are_persistent() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let api = Arc::new(ScriptApi::new(start().into_iter().chain([
        auth(true, 11),
        recycle(false),
        ack(),
    ])));
    let s = world(api.clone(), store);
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s.clone()).unwrap();
    let begun = service
        .begin_for(&t, id(), TrialConsent::DoNotStart)
        .await
        .unwrap();
    assert!(begun.can_use);
    assert!(!begun.should_release);
    assert!(begun.lease.start_acknowledged);
    assert!(
        service
            .begin_for(&t, EffectId::new(301).unwrap(), TrialConsent::DoNotStart)
            .await
            .is_err()
    );
    assert_eq!(writes(&api).len(), 1);
    drop(service);
    drop(s);
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(temp.path()).unwrap()),
    );
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s).unwrap();
    let reused = service
        .begin_for(&t, id(), TrialConsent::DoNotStart)
        .await
        .unwrap();
    assert!(reused.can_use);
    assert_eq!(reused.lease.operation, begun.lease.operation);
    assert_eq!(writes(&api).len(), 1);
    let stopped = service
        .release_for(&t, id(), ReleaseRetry::Never)
        .await
        .unwrap();
    assert_eq!(stopped.phase, LeasePhase::Released);
    assert!(stopped.stop_acknowledged);
    service
        .release_for(&t, id(), ReleaseRetry::Never)
        .await
        .unwrap();
    assert_eq!(writes(&api).len(), 2);
}
#[tokio::test]
async fn trials_require_explicit_action_and_denial_never_sends_registration() {
    for kind in [50, 51] {
        let temp = tempfile::tempdir().unwrap();
        let api = Arc::new(ScriptApi::new([
            auth(true, kind),
            auth(true, kind),
            ack(),
            auth(true, kind),
            recycle(false),
        ]));
        let s = world(
            api.clone(),
            Arc::new(StateStore::open(temp.path()).unwrap()),
        );
        let t = s.token().unwrap();
        let service = EffectAuthorizations::new(s).unwrap();
        assert!(
            service
                .begin_for(&t, id(), TrialConsent::DoNotStart)
                .await
                .is_err()
        );
        assert!(writes(&api).is_empty());
        assert!(service.record_for(&t, id()).unwrap().is_none());
        assert!(
            service
                .begin_for(&t, id(), TrialConsent::ExplicitStart)
                .await
                .unwrap()
                .can_use
        );
        assert_eq!(writes(&api).len(), 1);
    }
    let temp = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([auth(false, 0)]));
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(temp.path()).unwrap()),
    );
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s).unwrap();
    assert!(
        service
            .begin_for(&t, id(), TrialConsent::ExplicitStart)
            .await
            .is_err()
    );
    assert!(writes(&api).is_empty());
    assert!(service.records_for(&t).unwrap().is_empty());
}
#[tokio::test]
async fn lost_start_response_cannot_be_promoted_by_positive_read_and_can_be_released() {
    let temp = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        auth(true, 11),
        Reply::Ready(Err(Error::Transport("lost successful response".into()))),
        auth(true, 11),
        recycle(false),
        ack(),
    ]));
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(temp.path()).unwrap()),
    );
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s).unwrap();
    assert!(
        service
            .begin_for(&t, id(), TrialConsent::DoNotStart)
            .await
            .is_err()
    );
    assert_eq!(
        service.record_for(&t, id()).unwrap().unwrap().phase,
        LeasePhase::StartUncertain
    );
    assert!(matches!(
        service.begin_for(&t, id(), TrialConsent::DoNotStart).await,
        Err(Error::MutationUncertain)
    ));
    let observed = service.observe_for(&t, id()).await.unwrap();
    assert!(!observed.can_use);
    assert_eq!(observed.lease.phase, LeasePhase::StartUncertain);
    assert_eq!(writes(&api).len(), 1);
    let released = service
        .release_for(&t, id(), ReleaseRetry::Never)
        .await
        .unwrap();
    assert!(released.stop_acknowledged);
    assert!(!released.start_acknowledged);
    assert_eq!(released.phase, LeasePhase::Released);
    assert_eq!(writes(&api).len(), 2);
}
#[tokio::test]
async fn lost_stop_response_requires_explicit_retry_and_reads_never_replay_it() {
    let temp = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new(start().into_iter().chain([
        Reply::Ready(Err(Error::Transport("lost stop response".into()))),
        auth(true, 11),
        recycle(false),
        ack(),
    ])));
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(temp.path()).unwrap()),
    );
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s).unwrap();
    service
        .begin_for(&t, id(), TrialConsent::DoNotStart)
        .await
        .unwrap();
    assert!(
        service
            .release_for(&t, id(), ReleaseRetry::Never)
            .await
            .is_err()
    );
    assert!(matches!(
        service.release_for(&t, id(), ReleaseRetry::Never).await,
        Err(Error::MutationUncertain)
    ));
    let observed = service.observe_for(&t, id()).await.unwrap();
    assert_eq!(observed.lease.phase, LeasePhase::StopUncertain);
    assert!(!observed.can_use);
    assert_eq!(writes(&api).len(), 2);
    service
        .release_for(&t, id(), ReleaseRetry::ExplicitAfterUnknownResult)
        .await
        .unwrap();
    assert_eq!(writes(&api).len(), 3);
}
#[tokio::test]
async fn stale_account_keeps_received_ack_in_original_namespace_without_publishing_success() {
    let temp = tempfile::tempdir().unwrap();
    let (pending, send) = gate();
    let api = Arc::new(ScriptApi::new([auth(true, 11), pending]));
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(temp.path()).unwrap()),
    );
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s.clone()).unwrap();
    let worker = {
        let service = service.clone();
        let t = t.clone();
        tokio::spawn(async move { service.begin_for(&t, id(), TrialConsent::DoNotStart).await })
    };
    api.wait_for_calls(2).await;
    s.adopt(api.clone(), json!({"userId":2}), &t).unwrap();
    send.send(Ok(json!({"code":200,"data":true}))).unwrap();
    assert!(matches!(worker.await.unwrap(), Err(Error::StaleResponse)));
    assert!(service.records_for(&s.token().unwrap()).unwrap().is_empty());
    s.adopt(api, json!({"userId":1}), &s.token().unwrap())
        .unwrap();
    let saved = service
        .record_for(&s.token().unwrap(), id())
        .unwrap()
        .unwrap();
    assert!(saved.start_acknowledged);
    assert_eq!(saved.phase, LeasePhase::Registered);
}
#[tokio::test]
async fn cancelled_registration_survives_reopen_as_uncertain_and_lock_prevents_second_write() {
    let temp = tempfile::tempdir().unwrap();
    let (pending, _send) = gate();
    let api = Arc::new(ScriptApi::new([
        auth(true, 11),
        pending,
        auth(true, 11),
        recycle(false),
    ]));
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(temp.path()).unwrap()),
    );
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s).unwrap();
    let worker = {
        let service = service.clone();
        let t = t.clone();
        tokio::spawn(async move { service.begin_for(&t, id(), TrialConsent::DoNotStart).await })
    };
    api.wait_for_calls(2).await;
    assert!(matches!(
        service.begin_for(&t, id(), TrialConsent::DoNotStart).await,
        Err(Error::MutationBusy)
    ));
    worker.abort();
    assert!(matches!(worker.await,Err(e) if e.is_cancelled()));
    assert_eq!(
        service.record_for(&t, id()).unwrap().unwrap().phase,
        LeasePhase::Registering
    );
    drop(service);
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(temp.path()).unwrap()),
    );
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s).unwrap();
    let observed = service.observe_for(&t, id()).await.unwrap();
    assert_eq!(observed.lease.phase, LeasePhase::StartUncertain);
    assert!(!observed.can_use);
    assert_eq!(writes(&api).len(), 1);
}
#[tokio::test]
async fn recycled_or_denied_rights_request_release_but_do_not_issue_hidden_writes() {
    for (can_use, can_recycle) in [(true, true), (false, false)] {
        let temp = tempfile::tempdir().unwrap();
        let api = Arc::new(ScriptApi::new(start().into_iter().chain([
            auth(can_use, 11),
            recycle(can_recycle),
            ack(),
        ])));
        let s = world(
            api.clone(),
            Arc::new(StateStore::open(temp.path()).unwrap()),
        );
        let t = s.token().unwrap();
        let service = EffectAuthorizations::new(s).unwrap();
        service
            .begin_for(&t, id(), TrialConsent::DoNotStart)
            .await
            .unwrap();
        let checked = service.observe_for(&t, id()).await.unwrap();
        assert!(!checked.can_use);
        assert!(checked.should_release);
        assert_eq!(writes(&api).len(), 1);
        service
            .release_for(&t, id(), ReleaseRetry::Never)
            .await
            .unwrap();
        assert_eq!(writes(&api).len(), 2);
    }
}
#[tokio::test]
async fn failed_post_registration_read_keeps_receipt_and_recovery_does_not_register_twice() {
    let temp = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        auth(true, 11),
        ack(),
        Reply::Ready(Err(Error::Transport("offline".into()))),
        auth(true, 11),
        recycle(false),
    ]));
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(temp.path()).unwrap()),
    );
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s).unwrap();
    assert!(
        service
            .begin_for(&t, id(), TrialConsent::DoNotStart)
            .await
            .is_err()
    );
    let saved = service.record_for(&t, id()).unwrap().unwrap();
    assert_eq!(saved.phase, LeasePhase::Registered);
    assert!(saved.authorization.is_none());
    assert!(saved.last_error.is_some());
    assert!(service.observe_for(&t, id()).await.unwrap().can_use);
    assert_eq!(writes(&api).len(), 1);
}
#[tokio::test]
async fn confirmed_rejection_is_distinct_from_unknown_and_cannot_release_foreign_leases() {
    let temp = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([auth(true, 11), json!({"code":403}).into()]));
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(temp.path()).unwrap()),
    );
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s).unwrap();
    assert!(
        service
            .begin_for(&t, id(), TrialConsent::DoNotStart)
            .await
            .is_err()
    );
    assert_eq!(
        service.record_for(&t, id()).unwrap().unwrap().phase,
        LeasePhase::StartRejected
    );
    assert_eq!(
        service
            .release_for(&t, id(), ReleaseRetry::Never)
            .await
            .unwrap()
            .phase,
        LeasePhase::StartRejected
    );
    assert!(
        service
            .release_for(&t, EffectId::new(301).unwrap(), ReleaseRetry::Never)
            .await
            .is_err()
    );
    assert_eq!(writes(&api).len(), 1);
}
#[tokio::test]
async fn corrupted_journal_cannot_turn_an_unacknowledged_write_into_registered_state() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let api = Arc::new(ScriptApi::new(start()));
    let s = world(api, store.clone());
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s).unwrap();
    service
        .begin_for(&t, id(), TrialConsent::DoNotStart)
        .await
        .unwrap();
    let mut record = store
        .get("1", "effect-authorizations-v1", "300")
        .unwrap()
        .unwrap();
    record["start_acknowledged"] = json!(false);
    store
        .put("1", "effect-authorizations-v1", "300", &record)
        .unwrap();
    assert!(service.record_for(&t, id()).is_err());
    assert!(service.records_for(&t).is_err());
    record["phase"] = json!("released");
    store
        .put("1", "effect-authorizations-v1", "300", &record)
        .unwrap();
    assert!(service.record_for(&t, id()).is_err());
}

#[tokio::test]
async fn cancelled_stop_recovers_as_unknown_and_only_explicit_retry_can_send_again() {
    let temp = tempfile::tempdir().unwrap();
    let (pending, _send) = gate();
    let api = Arc::new(ScriptApi::new(start().into_iter().chain([
        pending,
        auth(true, 11),
        recycle(false),
        ack(),
    ])));
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(temp.path()).unwrap()),
    );
    let t = s.token().unwrap();
    let service = EffectAuthorizations::new(s).unwrap();
    service
        .begin_for(&t, id(), TrialConsent::DoNotStart)
        .await
        .unwrap();
    let worker = {
        let service = service.clone();
        let t = t.clone();
        tokio::spawn(async move { service.release_for(&t, id(), ReleaseRetry::Never).await })
    };
    api.wait_for_calls(5).await;
    worker.abort();
    assert!(matches!(worker.await,Err(e) if e.is_cancelled()));
    assert_eq!(
        service.record_for(&t, id()).unwrap().unwrap().phase,
        LeasePhase::Releasing
    );
    let checked = service.observe_for(&t, id()).await.unwrap();
    assert_eq!(checked.lease.phase, LeasePhase::StopUncertain);
    assert!(!checked.can_use);
    assert!(matches!(
        service.release_for(&t, id(), ReleaseRetry::Never).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(writes(&api).len(), 2);
    service
        .release_for(&t, id(), ReleaseRetry::ExplicitAfterUnknownResult)
        .await
        .unwrap();
    assert_eq!(writes(&api).len(), 3);
}

#[tokio::test]
async fn success_envelope_without_positive_boolean_never_creates_a_write_receipt() {
    for response in [
        json!({"code":200}),
        json!({"code":200,"data":false}),
        json!({"code":200,"data":null}),
        json!({"code":200,"data":"true"}),
    ] {
        for stopping in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let replies = if stopping {
                start()
                    .into_iter()
                    .chain([response.clone().into()])
                    .collect::<Vec<_>>()
            } else {
                vec![auth(true, 11), response.clone().into()]
            };
            let api = Arc::new(ScriptApi::new(replies));
            let s = world(api, Arc::new(StateStore::open(temp.path()).unwrap()));
            let t = s.token().unwrap();
            let service = EffectAuthorizations::new(s).unwrap();
            if stopping {
                service
                    .begin_for(&t, id(), TrialConsent::DoNotStart)
                    .await
                    .unwrap();
                assert!(
                    service
                        .release_for(&t, id(), ReleaseRetry::Never)
                        .await
                        .is_err()
                );
            } else {
                assert!(
                    service
                        .begin_for(&t, id(), TrialConsent::DoNotStart)
                        .await
                        .is_err()
                );
            }
            let saved = service.record_for(&t, id()).unwrap().unwrap();
            assert_eq!(
                saved.phase,
                if stopping {
                    LeasePhase::StopUncertain
                } else {
                    LeasePhase::StartUncertain
                }
            );
            assert!(!saved.stop_acknowledged);
            assert_eq!(saved.start_acknowledged, stopping);
        }
    }
}
