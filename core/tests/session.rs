mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, QrLogin, QrStatus, SessionStatus},
    protocol::api::Api,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

fn profile(uid: u64) -> Value {
    json!({"code":200,"profile":{"userId":uid}})
}
fn account(uid: u64) -> Arc<ScriptApi> {
    Arc::new(ScriptApi::new([profile(uid).into()]))
}

#[tokio::test]
async fn late_success_and_failure_cannot_cross_account_generation() {
    for failure in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(tmp.path()).unwrap());
        let (reply, release) = gate();
        let old = Arc::new(ScriptApi::new([profile(1).into(), reply]));
        let session = Arc::new(AccountSession::new(old.clone(), Some(store.clone())));
        session.refresh().await.unwrap();
        let token = session.token().unwrap();
        let pending = {
            let session = session.clone();
            tokio::spawn(async move {
                session.run(|api, _| async move { api.call("/api/test", json!({}), false, &[200]).await }).await
            })
        };
        old.wait_for_calls(2).await;
        session.attach(account(2)).unwrap();
        session.refresh().await.unwrap();
        release
            .send(Ok(if failure {
                json!({"code":301})
            } else {
                json!({"code":200})
            }))
            .unwrap();
        assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
        assert!(matches!(
            session.save_result(&token, "resource", "old", &json!(1)),
            Err(Error::StaleResponse)
        ));
        assert_eq!(
            session.snapshot().unwrap().status,
            SessionStatus::Authenticated
        );
        assert_eq!(session.token().unwrap().uid.as_deref(), Some("2"));
        assert!(store.values("2", "resource").unwrap().is_empty());
    }
}

#[tokio::test]
async fn late_refresh_and_attach_back_to_same_uid_are_stale() {
    let (reply, release) = gate();
    let old = Arc::new(ScriptApi::new([profile(1).into(), reply]));
    let session = Arc::new(AccountSession::new(old.clone(), None));
    session.refresh().await.unwrap();
    let token = session.token().unwrap();
    let pending = {
        let s = session.clone();
        tokio::spawn(async move { s.refresh().await })
    };
    old.wait_for_calls(2).await;
    session.attach(account(1)).unwrap();
    session.refresh().await.unwrap();
    release.send(Ok(profile(1))).unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_ne!(session.token().unwrap(), token);
}

#[tokio::test]
async fn same_identity_refresh_preserves_token_and_301_expires_without_erasing_data() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    store
        .put("2", "playlist", "mine", &json!("preserve"))
        .unwrap();
    let session = AccountSession::new(
        Arc::new(ScriptApi::new([profile(1).into(), profile(1).into()])),
        Some(store.clone()),
    );
    session.refresh().await.unwrap();
    let token = session.token().unwrap();
    session.refresh().await.unwrap();
    assert_eq!(session.token().unwrap(), token);
    session
        .save_result(&token, "resource", "mine", &json!(42))
        .unwrap();
    let result: cumulus_core::Result<(_, ())> = session
        .run(|_, _| async { Err(Error::service("/api/test", json!(301), Value::Null)) })
        .await;
    assert!(result.unwrap_err().is_service_code(301));
    assert_eq!(session.snapshot().unwrap().status, SessionStatus::Expired);
    assert_eq!(session.token().unwrap().uid, None);
    assert_eq!(
        store.get("2", "playlist", "mine").unwrap(),
        Some(json!("preserve"))
    );
    assert_eq!(store.get("1", "resource", "mine").unwrap(), Some(json!(42)));
}

#[tokio::test]
async fn stale_candidate_and_disk_failure_do_not_publish_authenticated_state() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let session = AccountSession::new(account(1), Some(store));
    let old = session.token().unwrap();
    session.detach().unwrap();
    assert!(matches!(
        session.adopt(account(2), json!({"userId":2}), &old),
        Err(Error::StaleResponse)
    ));
    // External schema damage simulates a failed persistent profile commit.
    rusqlite::Connection::open(tmp.path().join("core.sqlite3"))
        .unwrap()
        .execute_batch("DROP TABLE state")
        .unwrap();
    let current = session.token().unwrap();
    assert!(matches!(
        session.adopt(account(2), json!({"userId":2}), &current),
        Err(Error::Storage(_))
    ));
    assert_eq!(session.token().unwrap(), current);
    assert_eq!(session.snapshot().unwrap().status, SessionStatus::SignedOut);
    assert!(matches!(session.refresh().await, Err(Error::Storage(_))));
    assert_eq!(session.token().unwrap(), current);
}

#[tokio::test]
async fn qr_challenge_requires_input_and_authorized_does_not_commit_identity() {
    let api = Arc::new(ScriptApi::new(
        [
            json!({"code":200,"unikey":"synthetic"}),
            json!({"code":801}),
            json!({"code":802}),
            json!({"code":8821,"challenge":"private"}),
            json!({"code":803}),
        ]
        .map(Reply::from),
    ));
    let qr = QrLogin::new(api.clone());
    let session = AccountSession::new(api.clone(), None);
    assert_eq!(qr.begin().await.unwrap(), "synthetic");
    assert_eq!(qr.poll(None).await.unwrap(), QrStatus::WaitingScan);
    assert_eq!(qr.poll(None).await.unwrap(), QrStatus::WaitingConfirmation);
    assert_eq!(qr.poll(None).await.unwrap(), QrStatus::Challenge);
    assert_eq!(qr.poll(None).await.unwrap(), QrStatus::Challenge);
    assert_eq!(api.calls().len(), 4);
    assert_eq!(
        qr.poll(Some("completed-by-user")).await.unwrap(),
        QrStatus::Authorized
    );
    assert_eq!(
        api.calls()[4].payload,
        json!({"key":"synthetic","type":3,"secureCaptcha":"completed-by-user"})
    );
    assert_eq!(session.snapshot().unwrap().status, SessionStatus::Unknown);
    assert!(qr.snapshot().unwrap().challenge.is_none());
    assert!(qr.poll(None).await.is_err());
}

#[tokio::test]
async fn qr_cancel_during_begin_or_poll_cannot_resurrect_login() {
    for during_begin in [false, true] {
        let (reply, release) = gate();
        let api = Arc::new(ScriptApi::new(if during_begin {
            vec![reply]
        } else {
            vec![json!({"code":200,"unikey":"synthetic"}).into(), reply]
        }));
        let qr = Arc::new(QrLogin::new(api.clone()));
        if !during_begin {
            qr.begin().await.unwrap();
        }
        let pending = {
            let qr = qr.clone();
            tokio::spawn(async move {
                if during_begin {
                    qr.begin().await.map(|_| ())
                } else {
                    qr.poll(None).await.map(|_| ())
                }
            })
        };
        api.wait_for_calls(if during_begin { 1 } else { 2 }).await;
        qr.cancel().unwrap();
        release
            .send(Ok(if during_begin {
                json!({"code":200,"unikey":"late"})
            } else {
                json!({"code":803})
            }))
            .unwrap();
        assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
        assert_eq!(qr.snapshot().unwrap().status, QrStatus::Cancelled);
        assert!(qr.snapshot().unwrap().key.is_none());
    }
}

#[tokio::test]
async fn qr_expiry_unknown_codes_and_create_failure_are_explicit() {
    for reply in [
        Reply::Ready(Err(Error::Transport("private detail".into()))),
        json!({"code":200}).into(),
    ] {
        let qr = QrLogin::new(Arc::new(ScriptApi::new([reply])));
        assert!(qr.begin().await.is_err());
        assert_eq!(qr.snapshot().unwrap().status, QrStatus::Error);
    }
    for code in [800, 999, 301] {
        let api = Arc::new(ScriptApi::new(
            [
                json!({"code":200,"unikey":"first"}),
                json!({"code":code}),
                json!({"code":200,"unikey":"second"}),
            ]
            .map(Reply::from),
        ));
        let qr = QrLogin::new(api.clone());
        qr.begin().await.unwrap();
        let result = qr.poll(None).await;
        if code == 800 {
            assert_eq!(result.unwrap(), QrStatus::Expired);
        } else {
            assert!(result.unwrap_err().is_service_code(code));
        }
        assert_eq!(qr.begin().await.unwrap(), "second");
        assert_eq!(
            api.calls()[2].payload,
            json!({"type":3,"lastUniKey":"first"})
        );
    }
}

#[tokio::test]
async fn api_contract_does_not_accept_missing_identity_or_retry_rejected_mutation() {
    let api =
        ScriptApi::new([json!({"code":200,"profile":null}), json!({"code":403})].map(Reply::from));
    assert!(api.account().await.unwrap_err().is_service_code(301));
    assert!(
        api.call(
            "/api/playlist/create",
            json!({"name":"test"}),
            false,
            &[200]
        )
        .await
        .unwrap_err()
        .is_service_code(403)
    );
    assert_eq!(api.calls().len(), 2);
}

#[tokio::test]
async fn overlapping_qr_polls_are_rejected_and_aborted_future_releases_the_slot() {
    let (reply, _release) = gate();
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"unikey":"synthetic"}).into(),
        reply,
        json!({"code":801}).into(),
    ]));
    let qr = Arc::new(QrLogin::new(api.clone()));
    qr.begin().await.unwrap();
    let pending = {
        let qr = qr.clone();
        tokio::spawn(async move { qr.poll(None).await })
    };
    api.wait_for_calls(2).await;
    assert!(matches!(qr.poll(None).await, Err(Error::State(_))));
    assert_eq!(api.calls().len(), 2);
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert_eq!(qr.poll(None).await.unwrap(), QrStatus::WaitingScan);
    assert_eq!(api.calls().len(), 3);
}
