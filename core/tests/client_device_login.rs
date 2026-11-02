mod support;
use cumulus_core::{
    Error,
    account::{
        SessionStatus,
        login::{DeviceVerificationKind as Kind, DeviceVerificationPhase as Phase, LoginStatus},
    },
    client::{
        MusicClient,
        dto::{LoginCommand as Cmd, LoginMode},
    },
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::http::{HttpResponse, ScriptHttp, gate, reply};

struct World {
    _root: tempfile::TempDir,
    http: Arc<ScriptHttp>,
    client: Arc<MusicClient>,
    id: String,
}
fn start_response() -> HttpResponse {
    json!({"code":200,"data":{"pollingToken":"fixture-device-token","deviceName":"Synthetic PC","safeDeviceCount":2}}).into()
}
fn authorized() -> HttpResponse {
    let mut r = reply(json!({"code":200}));
    r.set_cookies = vec!["MUSIC_U=fixture-device-user; Path=/; Secure; HttpOnly".into()];
    r.refresh_token = Some("fixture-device-refresh".into());
    HttpResponse::Ready(Ok(r))
}
fn identity() -> HttpResponse {
    json!({"code":200,"profile":{"userId":2,"nickname":"synthetic device account"}}).into()
}
impl World {
    async fn new(jsk: bool, replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut responses = Vec::new();
        if jsk {
            responses.push(json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":"t1"},{"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}]}).into());
        }
        let mut challenge = reply(json!({"code":8830}));
        challenge.set_cookies =
            vec!["MUSIC_TU=fixture-device-ticket; Path=/; Secure; HttpOnly".into()];
        responses.push(HttpResponse::Ready(Ok(challenge)));
        let list =
            json!({"code":200,"data":{"types":["DEVICE_CONFIRM","DEVICE_SCAN","SECURITY_EMAIL"]}});
        if jsk {
            responses.extend([json!({"code":200}).into(), list.into()]);
        } else {
            responses.extend([list.into(), json!({"code":200}).into()]);
        }
        responses.extend(replies);
        let http = Arc::new(ScriptHttp::new(responses));
        let client = MusicClient::open_with_http_backend(root.path(), None, http.clone())
            .unwrap()
            .with_login_device_name("Synthetic PC")
            .unwrap();
        let client = Arc::new(if jsk {
            client.with_official_login_policy()
        } else {
            client
        });
        let epoch = client.account().unwrap().epoch;
        let id = client
            .begin_login(&epoch, LoginMode::Email, None)
            .await
            .unwrap()
            .operation_id;
        client
            .login_command(
                &id,
                Cmd::Email {
                    address: "me@example.test".into(),
                    password: "fixture-password".into(),
                    remember: true,
                },
            )
            .await
            .unwrap();
        client
            .login_command(&id, Cmd::VerificationMethods)
            .await
            .unwrap();
        Self {
            _root: root,
            http,
            client,
            id,
        }
    }
    async fn start(&self, method: Kind) -> String {
        self.client
            .login_command(&self.id, Cmd::DeviceStart { method })
            .await
            .unwrap()
            .device
            .unwrap()
            .id
    }
    fn phase(&self) -> Phase {
        self.client
            .login_snapshot(&self.id)
            .unwrap()
            .device
            .unwrap()
            .phase
    }
    async fn poll(&self, id: &str) -> cumulus_core::Result<()> {
        self.client
            .login_command(
                &self.id,
                Cmd::DevicePoll {
                    verification_id: id.into(),
                },
            )
            .await
            .map(|_| ())
    }
}
fn frozen(jsk: bool, name: &str, path: &str, payload: &Value) {
    let oracle: Value =
        serde_json::from_str(include_str!("fixtures/device-login-official.json")).unwrap();
    let expected = if jsk {
        &oracle["jsk"][name]
    } else {
        &oracle["legacy"][path]
    };
    let mut actual = payload.clone();
    actual.as_object_mut().unwrap().remove("header");
    actual.as_object_mut().unwrap().remove("e_r");
    assert_eq!(&actual, expected);
}
#[tokio::test]
async fn confirmation_routes_match_executed_official_fields_and_authenticate_only_after_code() {
    for jsk in [false, true] {
        let w = World::new(
            jsk,
            [
                start_response(),
                json!({"code":801}).into(),
                json!({"code":400}).into(),
                authorized(),
                identity(),
            ],
        )
        .await;
        let id = w.start(Kind::Confirm).await;
        assert_eq!(w.phase(), Phase::Waiting);
        w.poll(&id).await.unwrap();
        let wrong = w
            .client
            .login_command(
                &w.id,
                Cmd::DeviceVerify {
                    verification_id: id.clone(),
                    code: "000000".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(wrong.status, LoginStatus::TwoFactor);
        assert!(wrong.device.unwrap().answer_rejected);
        let done = w
            .client
            .login_command(
                &w.id,
                Cmd::DeviceVerify {
                    verification_id: id,
                    code: "123456".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(done.status, LoginStatus::Authenticated);
        assert!(done.device.is_none());
        let saved = cumulus_core::account::profiles::ProfileRegistry::open(
            w._root.path(),
            cumulus_core::protocol::http::ApiOrigin::Pc,
        )
        .unwrap()
        .current(Default::default())
        .unwrap()
        .unwrap();
        assert_eq!(
            saved
                .credentials
                .login_credentials()
                .unwrap()
                .refresh_token
                .as_deref(),
            Some("fixture-device-refresh")
        );
        let calls = w.http.calls();
        let at = if jsk { 4 } else { 3 };
        for (index, name) in [
            (at, "startDeviceApply"),
            (at + 1, "checkDeviceStatus"),
            (at + 3, "checkDeviceCaptcha"),
        ] {
            frozen(jsk, name, &calls[index].path, &calls[index].payload);
            assert_eq!(
                calls[index].login_chain.as_deref(),
                if jsk { Some("") } else { None }
            );
        }
        assert!(
            !serde_json::to_string(&done)
                .unwrap()
                .contains("fixture-device")
        );
    }
}
#[tokio::test]
async fn scan_wait_scan_confirm_sequence_keeps_tokens_and_urls_out_of_public_views() {
    let w = World::new(
        true,
        [
            start_response(),
            json!({"code":801}).into(),
            json!({"code":802}).into(),
            authorized(),
            identity(),
        ],
    )
    .await;
    let id = w.start(Kind::Scan).await;
    let view = w.client.login_snapshot(&w.id).unwrap();
    assert!(view.device.as_ref().unwrap().modules.is_some());
    for secret in [
        "fixture-device-token",
        "pollingToken",
        "orpheus://",
        "MUSIC_TU",
        "fixture-device-ticket",
    ] {
        assert!(!serde_json::to_string(&view).unwrap().contains(secret));
    }
    assert_eq!(view.device.unwrap().poll_after_ms, 1000);
    w.poll(&id).await.unwrap();
    assert_eq!(w.phase(), Phase::Waiting);
    w.poll(&id).await.unwrap();
    assert_eq!(w.phase(), Phase::Scanned);
    w.poll(&id).await.unwrap();
    assert_eq!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
    let calls = w.http.calls();
    frozen(true, "startScanApply", &calls[4].path, &calls[4].payload);
    frozen(
        true,
        "checkScanApplyStatus",
        &calls[5].path,
        &calls[5].payload,
    );
}
#[tokio::test]
async fn a_confirmation_status_success_is_not_itself_a_login_or_a_code_submission() {
    let w = World::new(true, [start_response(), json!({"code":200}).into()]).await;
    let id = w.start(Kind::Confirm).await;
    w.poll(&id).await.unwrap();
    assert_eq!(w.phase(), Phase::CodeRequired);
    assert_eq!(w.client.account().unwrap().status, SessionStatus::Unknown);
    assert_eq!(w.http.calls().len(), 6);
}
#[tokio::test]
async fn stale_ids_and_other_methods_cannot_control_an_active_device_attempt() {
    let w = World::new(true, [start_response()]).await;
    let id = w.start(Kind::Scan).await;
    let count = w.http.calls().len();
    assert!(w.poll("not-this-verification").await.is_err());
    assert!(
        w.client
            .login_command(&w.id, Cmd::SecurityMailSend)
            .await
            .is_err()
    );
    assert!(
        w.client
            .login_command(
                &w.id,
                Cmd::DeviceStart {
                    method: Kind::Confirm
                }
            )
            .await
            .is_err()
    );
    assert!(
        w.client
            .login_command(
                &w.id,
                Cmd::DeviceVerify {
                    verification_id: id,
                    code: "123".into()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(w.http.calls().len(), count);
}
#[tokio::test]
async fn explicit_cancel_is_sent_once_and_releases_only_the_original_challenge() {
    for kind in [Kind::Confirm, Kind::Scan] {
        let w = World::new(true, [start_response(), json!({"code":200}).into()]).await;
        let id = w.start(kind).await;
        for _ in 0..2 {
            w.client
                .login_command(
                    &w.id,
                    Cmd::DeviceCancel {
                        verification_id: id.clone(),
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(w.phase(), Phase::Cancelled);
        let calls = w.http.calls();
        assert_eq!(calls.len(), 6);
        frozen(
            true,
            if kind == Kind::Scan {
                "cancelScanApply"
            } else {
                "cancelDeviceApply"
            },
            &calls[5].path,
            &calls[5].payload,
        );
    }
}
#[tokio::test]
async fn failed_cancellation_is_not_repeated_or_claimed_confirmed() {
    let w = World::new(
        true,
        [
            start_response(),
            HttpResponse::Ready(Err(Error::Transport("lost cancel receipt".into()))),
        ],
    )
    .await;
    let id = w.start(Kind::Scan).await;
    for _ in 0..2 {
        w.client
            .login_command(
                &w.id,
                Cmd::DeviceCancel {
                    verification_id: id.clone(),
                },
            )
            .await
            .unwrap();
    }
    assert_eq!(w.phase(), Phase::StopUnconfirmed);
    assert_eq!(w.http.calls().len(), 6);
    assert!(w.poll(&id).await.is_err());
}
#[tokio::test]
async fn unknown_start_is_not_repeated_and_local_stop_does_not_invent_a_remote_token() {
    let w = World::new(
        true,
        [HttpResponse::Ready(Err(Error::Transport(
            "lost start receipt".into(),
        )))],
    )
    .await;
    assert!(
        w.client
            .login_command(&w.id, Cmd::DeviceStart { method: Kind::Scan })
            .await
            .is_err()
    );
    let id = w.client.login_snapshot(&w.id).unwrap().device.unwrap().id;
    assert_eq!(w.phase(), Phase::RecoveryRequired);
    assert!(
        w.client
            .login_command(&w.id, Cmd::DeviceStart { method: Kind::Scan })
            .await
            .is_err()
    );
    w.client
        .login_command(
            &w.id,
            Cmd::DeviceCancel {
                verification_id: id,
            },
        )
        .await
        .unwrap();
    assert_eq!(w.phase(), Phase::StopUnconfirmed);
    assert_eq!(w.http.calls().len(), 5);
}
#[tokio::test]
async fn unknown_code_submission_can_only_be_read_back_and_never_repeats_the_code() {
    let mut lost = reply(json!({"code":200}));
    lost.body_status = cumulus_core::protocol::http::BodyStatus::Interrupted;
    lost.set_cookies = vec!["MUSIC_U=fixture-device-user; Path=/; Secure; HttpOnly".into()];
    lost.refresh_token = Some("DO_NOT_INSTALL_PARTIAL_REFRESH".into());
    let w = World::new(
        true,
        [start_response(), HttpResponse::Ready(Ok(lost)), identity()],
    )
    .await;
    let id = w.start(Kind::Confirm).await;
    assert!(
        w.client
            .login_command(
                &w.id,
                Cmd::DeviceVerify {
                    verification_id: id.clone(),
                    code: "123456".into()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(w.phase(), Phase::RecoveryRequired);
    assert!(matches!(
        w.client
            .login_command(
                &w.id,
                Cmd::DeviceVerify {
                    verification_id: id,
                    code: "123456".into()
                }
            )
            .await,
        Err(Error::MutationUncertain)
    ));
    w.client.login_command(&w.id, Cmd::Reconcile).await.unwrap();
    assert_eq!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
    assert_eq!(w.http.calls().len(), 7);
    assert_eq!(w.http.calls()[6].path, "/api/w/nuser/account/get");
    let saved = cumulus_core::account::profiles::ProfileRegistry::open(
        w._root.path(),
        cumulus_core::protocol::http::ApiOrigin::Pc,
    )
    .unwrap()
    .current(Default::default())
    .unwrap()
    .unwrap();
    assert!(
        saved
            .credentials
            .login_credentials()
            .unwrap()
            .refresh_token
            .is_none()
    );
}
#[tokio::test]
async fn a_read_failure_stops_polling_until_an_explicit_read_retry() {
    let w = World::new(
        true,
        [
            start_response(),
            HttpResponse::Ready(Err(Error::Transport("status unavailable".into()))),
            json!({"code":801}).into(),
        ],
    )
    .await;
    let id = w.start(Kind::Scan).await;
    assert!(w.poll(&id).await.is_err());
    assert_eq!(w.phase(), Phase::Error);
    assert_eq!(w.http.calls().len(), 6);
    w.poll(&id).await.unwrap();
    assert_eq!(w.phase(), Phase::Waiting);
    assert_eq!(w.http.calls().len(), 7);
}
#[tokio::test]
async fn server_terminal_states_withdraw_the_qr_and_cannot_read_or_publish_identity() {
    for (code, phase) in [
        (810, Phase::Expired),
        (811, Phase::Rejected),
        (815, Phase::Blocked),
    ] {
        let w = World::new(true, [start_response(), json!({"code":code}).into()]).await;
        let id = w.start(Kind::Scan).await;
        w.poll(&id).await.unwrap();
        assert_eq!(w.phase(), phase);
        assert!(
            w.client
                .login_snapshot(&w.id)
                .unwrap()
                .device
                .unwrap()
                .modules
                .is_none()
        );
        assert!(w.poll(&id).await.is_err());
        assert_eq!(w.http.calls().len(), 6);
    }
}
#[tokio::test]
async fn closing_the_login_during_authorization_prevents_identity_reads_and_adoption() {
    let (held, release) = gate();
    let w = World::new(true, [start_response(), held]).await;
    let id = w.start(Kind::Scan).await;
    let client = w.client.clone();
    let login = w.id.clone();
    let pending = tokio::spawn(async move {
        client
            .login_command(
                &login,
                Cmd::DevicePoll {
                    verification_id: id,
                },
            )
            .await
    });
    w.http.wait(6).await;
    w.client.cancel_login(&w.id).unwrap();
    let HttpResponse::Ready(Ok(r)) = authorized() else {
        panic!("ready authorization")
    };
    assert!(release.send(Ok(r)).is_ok());
    assert!(pending.await.unwrap().is_err());
    assert_eq!(w.http.calls().len(), 6);
    assert_eq!(w.client.account().unwrap().status, SessionStatus::Unknown);
    assert!(
        w.client
            .saved_accounts(&w.client.account().unwrap().epoch)
            .unwrap()
            .value
            .is_empty()
    );
}

#[tokio::test]
async fn a_business_rejection_inside_http_200_does_not_authorize_the_candidate() {
    let mut rejected = reply(json!({"code":200,"data":{"success":false}}));
    rejected.set_cookies = vec!["MUSIC_U=fixture-device-user; Path=/; Secure; HttpOnly".into()];
    rejected.refresh_token = Some("must-not-install-rejected-refresh".into());
    let w = World::new(
        true,
        [
            start_response(),
            HttpResponse::Ready(Ok(rejected)),
            identity(),
        ],
    )
    .await;
    let id = w.start(Kind::Confirm).await;
    let view = w
        .client
        .login_command(
            &w.id,
            Cmd::DeviceVerify {
                verification_id: id,
                code: "123456".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(view.status, LoginStatus::TwoFactor);
    assert!(view.device.unwrap().answer_rejected);
    assert_eq!(w.http.calls().len(), 6);
    assert_ne!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
    // The Cookie may only be inspected through explicit identity reconciliation.
    // Its rejected refresh header must never become a successful login receipt.
    w.client.login_command(&w.id, Cmd::Reconcile).await.unwrap();
    let saved = cumulus_core::account::profiles::ProfileRegistry::open(
        w._root.path(),
        cumulus_core::protocol::http::ApiOrigin::Pc,
    )
    .unwrap()
    .current(Default::default())
    .unwrap()
    .unwrap();
    assert!(
        saved
            .credentials
            .login_credentials()
            .unwrap()
            .refresh_token
            .is_none()
    );
}
#[tokio::test]
async fn an_acknowledged_scan_can_replay_login_once_but_a_new_challenge_needs_new_offers() {
    let w = World::new(
        true,
        [
            start_response(),
            json!({"code":200}).into(),
            json!({"code":301}).into(),
            json!({"code":8830}).into(),
        ],
    )
    .await;
    let id = w.start(Kind::Scan).await;
    w.poll(&id).await.unwrap();
    let view = w.client.login_snapshot(&w.id).unwrap();
    assert_eq!(view.status, LoginStatus::TwoFactor);
    assert!(view.device.is_none());
    assert!(view.challenge.methods.is_empty());
    assert!(
        w.client
            .login_command(&w.id, Cmd::DeviceStart { method: Kind::Scan })
            .await
            .is_err()
    );
    let calls = w.http.calls();
    assert_eq!(calls.len(), 8);
    assert_eq!(calls[6].path, "/api/w/nuser/account/get");
    assert_eq!(calls[7].path, "/api/login");
    assert_eq!(calls[1].payload["password"], calls[7].payload["password"]);
}
#[tokio::test]
async fn a_scan_attempt_rejects_confirmation_code_input_before_changing_its_state() {
    let w = World::new(true, [start_response()]).await;
    let id = w.start(Kind::Scan).await;
    assert!(
        w.client
            .login_command(
                &w.id,
                Cmd::DeviceVerify {
                    verification_id: id,
                    code: "123456".into()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(w.phase(), Phase::Waiting);
    assert_eq!(
        w.client.login_snapshot(&w.id).unwrap().status,
        LoginStatus::TwoFactor
    );
    assert_eq!(w.http.calls().len(), 5);
}

#[tokio::test]
async fn jsk_local_stop_preempts_a_pending_poll_without_remote_cancel_or_late_login() {
    let (held, release) = gate();
    let w = World::new(true, [start_response(), held]).await;
    let id = w.start(Kind::Scan).await;
    let client = w.client.clone();
    let login = w.id.clone();
    let device = id.clone();
    let pending = tokio::spawn(async move {
        client
            .login_command(
                &login,
                Cmd::DevicePoll {
                    verification_id: device,
                },
            )
            .await
    });
    w.http.wait(6).await;
    let stopped = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        w.client.login_command(
            &w.id,
            Cmd::DeviceStop {
                verification_id: id.clone(),
            },
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(stopped.device.unwrap().phase, Phase::Stopped);
    assert_eq!(w.http.calls().len(), 6);
    let HttpResponse::Ready(Ok(response)) = authorized() else {
        panic!("ready")
    };
    assert!(release.send(Ok(response)).is_ok());
    assert!(pending.await.unwrap().is_err());
    assert!(w.poll(&id).await.is_err());
    assert_eq!(w.phase(), Phase::Stopped);
    assert_eq!(w.client.account().unwrap().status, SessionStatus::Unknown);
    assert_eq!(w.http.calls().len(), 6);
}
#[tokio::test]
async fn legacy_leaving_device_verification_sends_one_cancel_using_the_original_token() {
    let w = World::new(false, [start_response(), json!({"code":200}).into()]).await;
    let id = w.start(Kind::Confirm).await;
    w.client
        .login_command(
            &w.id,
            Cmd::DeviceStop {
                verification_id: id.clone(),
            },
        )
        .await
        .unwrap();
    w.client
        .login_command(
            &w.id,
            Cmd::DeviceStop {
                verification_id: id,
            },
        )
        .await
        .unwrap();
    assert_eq!(w.phase(), Phase::Cancelled);
    let calls = w.http.calls();
    assert_eq!(calls.len(), 5);
    frozen(
        false,
        "cancelDeviceApply",
        &calls[4].path,
        &calls[4].payload,
    );
}

#[tokio::test]
async fn closing_a_legacy_login_revokes_locally_before_its_single_cleanup_finishes() {
    let (held, release) = gate();
    let w = World::new(false, [start_response(), held]).await;
    let id = w.start(Kind::Scan).await;
    let c = w.client.clone();
    let login = w.id.clone();
    let closing = tokio::spawn(async move { c.cancel_login_and_cleanup(&login).await });
    w.http.wait(5).await;
    assert!(w.client.login_snapshot(&w.id).is_err());
    assert!(w.poll(&id).await.is_err());
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    closing.await.unwrap().unwrap();
    assert_eq!(w.http.calls().len(), 5);
    assert_eq!(
        w.http.calls()[4].path,
        "/api/login/origin-device/scan-apply/cancel"
    );
    assert!(w.client.cancel_login_and_cleanup(&w.id).await.is_err());
    assert_eq!(w.http.calls().len(), 5);
}
