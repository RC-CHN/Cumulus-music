mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::http::ApiOrigin,
};
use serde_json::json;
use std::sync::Arc;
use support::ScriptApi;

#[tokio::test]
async fn only_the_current_account_can_request_the_fixed_local_handoff() {
    let directory = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":1}}).into()
    ]));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = MusicClient::with_session(session.clone(), registry);
    let guest = client.account().unwrap().epoch;
    assert!(matches!(
        client.account_binding_qr(&guest),
        Err(Error::State("no authenticated account"))
    ));
    assert!(api.calls().is_empty());

    session.refresh().await.unwrap();
    let epoch = client.account().unwrap().epoch;
    assert!(matches!(
        client.account_binding_qr(&guest),
        Err(Error::StaleResponse)
    ));
    let qr = client.account_binding_qr(&epoch).unwrap();
    assert_eq!(qr.epoch, epoch);
    let matrix = &qr.value.modules;
    assert_eq!(matrix.len(), 25);
    assert!(matrix.iter().all(|row| row.len() == 25));
    assert!(matrix.iter().flatten().any(|cell| *cell));
    assert!(matrix.iter().flatten().any(|cell| !*cell));
    let response = serde_json::to_value(qr).unwrap();
    assert_eq!(response["value"].as_object().unwrap().len(), 1);
    assert!(response["value"]["modules"].is_array());
    assert_eq!(api.calls().len(), 1);
    assert_eq!(api.calls()[0].path, "/api/w/nuser/account/get");

    session.attach(Arc::new(ScriptApi::new([]))).unwrap();
    assert!(matches!(
        client.account_binding_qr(&epoch),
        Err(Error::StaleResponse)
    ));
}
