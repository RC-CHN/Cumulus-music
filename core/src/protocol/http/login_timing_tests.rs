//! Login acknowledgement boundaries exercised through the actual transport.
use super::*;
use crate::{account::credentials::Credentials, protocol::api::ApiFuture, storage::StateStore};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

struct Reply(Mutex<Option<HttpReply>>);
impl HttpBackend for Reply {
    fn post(&self, _: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async { Ok(self.0.lock().unwrap().take().expect("unexpected retry")) })
    }
}

#[tokio::test]
async fn only_acknowledged_completed_jsk_login_responses_record_login_time() {
    let cases = [
        (
            "/api/login",
            200,
            json!({"code":200}),
            BodyStatus::Complete,
            true,
        ),
        (
            "/api/login/cellphone",
            200,
            json!({"code":200}),
            BodyStatus::Complete,
            true,
        ),
        (
            "/api/login/qrcode/client/login",
            200,
            json!({"code":803}),
            BodyStatus::Complete,
            true,
        ),
        (
            "/api/login/2fa/mail/captcha/check",
            200,
            json!({"code":200}),
            BodyStatus::Complete,
            true,
        ),
        (
            "/api/login/origin-device/apply/check",
            200,
            json!({"code":200}),
            BodyStatus::Complete,
            true,
        ),
        (
            "/api/middle/account/uplinkcaptcha/verify",
            200,
            json!({"code":200,"data":true}),
            BodyStatus::Complete,
            true,
        ),
        (
            "/api/middle/account/captcha/verify",
            200,
            json!({"code":200,"data":true}),
            BodyStatus::Complete,
            true,
        ),
        (
            "/api/login",
            200,
            json!({"code":502}),
            BodyStatus::Complete,
            false,
        ),
        (
            "/api/login",
            503,
            json!({"code":200}),
            BodyStatus::Complete,
            false,
        ),
        (
            "/api/login",
            200,
            json!({"code":200,"data":false}),
            BodyStatus::Complete,
            false,
        ),
        (
            "/api/login",
            200,
            json!({"code":200,"data":{"success":false}}),
            BodyStatus::Complete,
            false,
        ),
        (
            "/api/login",
            200,
            json!({"code":200}),
            BodyStatus::Interrupted,
            false,
        ),
        (
            "/api/login/qrcode/client/login",
            200,
            json!({"code":801}),
            BodyStatus::Complete,
            false,
        ),
        (
            "/api/user/2fa/list",
            200,
            json!({"code":200}),
            BodyStatus::Complete,
            false,
        ),
        (
            "/api/user/2fa/begin",
            200,
            json!({"code":200}),
            BodyStatus::Complete,
            false,
        ),
        (
            "/api/middle/account/uplinkcaptcha/get",
            200,
            json!({"code":200}),
            BodyStatus::Complete,
            false,
        ),
        (
            "/api/middle/account/captcha/sent",
            200,
            json!({"code":200}),
            BodyStatus::Complete,
            false,
        ),
        (
            "/api/middle/account/uplinkcaptcha/verify",
            200,
            json!({"code":200}),
            BodyStatus::Complete,
            false,
        ),
    ];
    for (path, status, body, body_status, expected) in cases {
        let root = tempfile::tempdir().unwrap();
        let credentials = Arc::new(
            Credentials::open(
                Arc::new(StateStore::open(root.path()).unwrap()),
                ApiOrigin::Pc,
            )
            .unwrap(),
        );
        let backend = Arc::new(Reply(Mutex::new(Some(HttpReply {
            status,
            body: serde_json::to_vec(&body).unwrap(),
            body_status,
            set_cookies: vec!["MUSIC_U=synthetic-timing; Path=/; Secure".into()],
            refresh_token: Some("synthetic-refresh".into()),
            metadata: BTreeMap::new(),
        }))));
        let client = MusicHttpClient::with_backend(
            credentials.clone(),
            Default::default(),
            Default::default(),
            None,
            backend,
        )
        .unwrap();
        let _ = client
            .request_jsk_login(path, json!({}), &JskLoginContext::default())
            .await;
        assert_eq!(
            credentials
                .login_credentials()
                .unwrap()
                .last_login_at
                .is_some(),
            expected,
            "{path}: {body}"
        );
    }
}

#[tokio::test]
async fn response_without_a_refresh_token_still_records_successful_login_time() {
    let root = tempfile::tempdir().unwrap();
    let credentials = Arc::new(
        Credentials::open(
            Arc::new(StateStore::open(root.path()).unwrap()),
            ApiOrigin::Pc,
        )
        .unwrap(),
    );
    let backend = Arc::new(Reply(Mutex::new(Some(HttpReply {
        status: 200,
        body: serde_json::to_vec(&json!({"code":200})).unwrap(),
        body_status: BodyStatus::Complete,
        set_cookies: vec![],
        refresh_token: None,
        metadata: BTreeMap::new(),
    }))));
    let client = MusicHttpClient::with_backend(
        credentials.clone(),
        Default::default(),
        Default::default(),
        None,
        backend,
    )
    .unwrap();
    client
        .request_jsk_login(
            "/api/login",
            Value::Object(Default::default()),
            &JskLoginContext::default(),
        )
        .await
        .unwrap();
    let saved = credentials.login_credentials().unwrap();
    assert!(saved.last_login_at.is_some());
    assert!(saved.refresh_token.is_none() && saved.token.is_none());
}
