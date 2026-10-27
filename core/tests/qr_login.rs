mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, QrStatus, login::QrAuthentication, profiles::ProfileRegistry},
    protocol::http::{ApiOrigin, ClientProfile, RequestOptions},
};
use serde_json::json;
use std::sync::Arc;
use support::{
    ScriptApi,
    http::{ScriptHttp, gate, reply},
};

#[tokio::test]
async fn qr_803_requires_identity_before_persisting_selection_and_can_reconcile_failure() {
    let directory = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
    let http = Arc::new(ScriptHttp::new([
        json!({"code":200,"unikey":"synthetic-qr"}).into(),
        json!({"code":801}).into(),
        json!({"code":802}).into(),
        json!({"code":803}).into(),
        json!({"code":301}).into(),
        json!({"code":200,"profile":{"userId":2}}).into(),
    ]));
    let session = Arc::new(AccountSession::new(
        Arc::new(ScriptApi::new([])),
        Some(registry.store()),
    ));
    let candidate = registry
        .candidate_with_backend(
            ClientProfile::default(),
            RequestOptions::default(),
            None,
            http.clone(),
        )
        .unwrap();
    let flow = QrAuthentication::new(session.clone(), candidate).unwrap();
    assert_eq!(flow.begin().await.unwrap(), "synthetic-qr");
    assert_eq!(flow.poll(None).await.unwrap(), QrStatus::WaitingScan);
    assert_eq!(
        flow.poll(None).await.unwrap(),
        QrStatus::WaitingConfirmation
    );
    assert!(flow.poll(None).await.unwrap_err().is_service_code(301));
    assert!(!flow.authenticated().unwrap());
    assert!(registry.selection().unwrap().is_none());
    flow.reconcile().await.unwrap();
    assert!(flow.authenticated().unwrap());
    assert_eq!(session.token().unwrap().uid.as_deref(), Some("2"));
    assert!(registry.selection().unwrap().is_some());
    assert_eq!(
        http.calls()
            .iter()
            .filter(|c| c.path.ends_with("client/login"))
            .count(),
        3
    );
}

#[tokio::test]
async fn cancelling_qr_during_identity_read_prevents_selected_profile_commit() {
    let directory = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
    let (pending, release) = gate();
    let http = Arc::new(ScriptHttp::new([
        json!({"code":200,"unikey":"synthetic-qr"}).into(),
        json!({"code":803}).into(),
        pending,
    ]));
    let session = Arc::new(AccountSession::new(
        Arc::new(ScriptApi::new([])),
        Some(registry.store()),
    ));
    let candidate = registry
        .candidate_with_backend(
            ClientProfile::default(),
            RequestOptions::default(),
            None,
            http.clone(),
        )
        .unwrap();
    let flow = Arc::new(QrAuthentication::new(session.clone(), candidate).unwrap());
    flow.begin().await.unwrap();
    let work = {
        let flow = flow.clone();
        tokio::spawn(async move { flow.poll(None).await })
    };
    http.wait(3).await;
    flow.cancel().unwrap();
    assert!(
        release
            .send(Ok(reply(json!({"code":200,"profile":{"userId":2}}))))
            .is_ok()
    );
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(flow.snapshot().unwrap().status, QrStatus::Cancelled);
    assert!(registry.selection().unwrap().is_none());
    assert!(session.token().unwrap().uid.is_none());
}
