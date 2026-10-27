use cumulus_core::{
    Error,
    account::AccountSession,
    listen_together::{
        ImCredentials,
        native_im::{NativeIm, NimConfig},
    },
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::Arc;
struct NoNetwork;
impl Api for NoNetwork {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("identity validation must not make network calls") })
    }
}
fn config() -> NimConfig {
    NimConfig {
        library_directory: "/missing-native-sdk".into(),
        data_root: "/must-not-be-created".into(),
        app_key: "0".repeat(32),
        database_key: "1".repeat(32),
    }
}
fn credentials(uid: &str) -> ImCredentials {
    ImCredentials {
        uid: uid.into(),
        account_id: "native-account".into(),
        token: "private-token".into(),
    }
}
#[test]
fn switched_or_mismatched_accounts_fail_before_loading_sdk_or_creating_cache() {
    let api = Arc::new(NoNetwork);
    let session = Arc::new(AccountSession::new(api.clone(), None));
    session
        .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let first = session.token().unwrap();
    session.adopt(api, json!({"userId":2}), &first).unwrap();
    assert!(matches!(
        NativeIm::open(config(), session.clone(), first, credentials("1")),
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        NativeIm::open(
            config(),
            session.clone(),
            session.token().unwrap(),
            credentials("1")
        ),
        Err(Error::StaleResponse)
    ));
}
#[test]
fn invalid_configuration_is_rejected_before_native_initialization() {
    let api = Arc::new(NoNetwork);
    let session = Arc::new(AccountSession::new(api.clone(), None));
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let mut invalid = config();
    invalid.app_key = "wrong".into();
    assert!(matches!(
        NativeIm::open(
            invalid,
            session.clone(),
            session.token().unwrap(),
            credentials("1")
        ),
        Err(Error::InvalidInput(_))
    ));
    let mut invalid = config();
    invalid.database_key.clear();
    assert!(matches!(
        NativeIm::open(
            invalid,
            session.clone(),
            session.token().unwrap(),
            credentials("1")
        ),
        Err(Error::InvalidInput(_))
    ));
}
