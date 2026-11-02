mod support;
use cumulus_core::{
    Error,
    account::{
        SessionStatus,
        login::{LoginStatus, SmsDirection as Direction, SmsPhase as Phase},
    },
    client::{
        MusicClient,
        dto::{LoginCommand as Cmd, LoginMode},
    },
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::http::{HttpResponse, ScriptHttp, gate, reply};

struct World {
    _root: tempfile::TempDir,
    http: Arc<ScriptHttp>,
    client: Arc<MusicClient>,
    id: String,
}
fn info() -> HttpResponse {
    json!({"code":200,"data":{"uploadText":"SEND 1234","uploadNumber":"1069000001","uploadNumberBackup":"1069000002"}}).into()
}
fn identity() -> HttpResponse {
    json!({"code":200,"profile":{"userId":2,"nickname":"Synthetic SMS account"}}).into()
}
fn authorized(data: Value) -> HttpResponse {
    let mut response = reply(json!({"code":200,"data":data}));
    response.set_cookies = vec!["MUSIC_U=synthetic-sms-user; Path=/; Secure; HttpOnly".into()];
    response.refresh_token = Some("synthetic-sms-refresh".into());
    HttpResponse::Ready(Ok(response))
}
impl World {
    async fn new(
        jsk: bool,
        country: Value,
        offered: bool,
        replies: impl IntoIterator<Item = HttpResponse>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut responses = Vec::new();
        if jsk {
            responses.push(json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":"t1"},{"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}]}).into());
        }
        let mut challenge = reply(json!({"code":8830}));
        challenge.set_cookies =
            vec!["MUSIC_TU=synthetic-sms-ticket; Path=/; Secure; HttpOnly".into()];
        responses.push(HttpResponse::Ready(Ok(challenge)));
        let list = json!({"code":200,"data":{"types":if offered {json!(["UP_SMS","SECURITY_EMAIL"])}else{json!(["SECURITY_EMAIL"])},"phoneMask":"139****1111","authDetail":{"UP_SMS":"138****8000"},"ctCode":country}});
        if jsk {
            responses.extend([json!({"code":200}).into(), list.into()]);
        } else {
            responses.extend([list.into(), json!({"code":200}).into()]);
        }
        responses.extend(replies);
        let http = Arc::new(ScriptHttp::new(responses));
        let client = MusicClient::open_with_http_backend(root.path(), None, http.clone()).unwrap();
        let client = Arc::new(if jsk {
            client.with_official_login_policy()
        } else {
            client
        });
        let id = client
            .begin_login(&client.account().unwrap().epoch, LoginMode::Email, None)
            .await
            .unwrap()
            .operation_id;
        client
            .login_command(
                &id,
                Cmd::Email {
                    address: "sms@example.test".into(),
                    password: "synthetic-password".into(),
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
    async fn start(&self, direction: Direction) -> String {
        self.client
            .login_command(&self.id, Cmd::SmsStart { direction })
            .await
            .unwrap()
            .sms
            .unwrap()
            .id
    }
    fn phase(&self) -> Phase {
        self.client
            .login_snapshot(&self.id)
            .unwrap()
            .sms
            .unwrap()
            .phase
    }
    fn check(
        &self,
        id: String,
    ) -> tokio::task::JoinHandle<cumulus_core::Result<cumulus_core::client::dto::LoginView>> {
        let client = self.client.clone();
        let op = self.id.clone();
        tokio::spawn(async move {
            client
                .login_command(
                    &op,
                    Cmd::SmsCheck {
                        verification_id: id,
                    },
                )
                .await
        })
    }
}
fn frozen(jsk: bool, name: &str, path: &str, payload: &Value) {
    let oracle: Value =
        serde_json::from_str(include_str!("fixtures/sms-login-official.json")).unwrap();
    let expected = if jsk {
        &oracle["jsk"][name]["payload"]
    } else {
        &oracle["legacy"][path]
    };
    let mut actual = payload.clone();
    actual.as_object_mut().unwrap().remove("header");
    actual.as_object_mut().unwrap().remove("e_r");
    assert_eq!(&actual, expected, "{name}");
}
#[tokio::test]
async fn uplink_matches_original_wire_and_saves_only_acknowledged_real_pair() {
    for jsk in [false, true] {
        let w = World::new(
            jsk,
            json!("86"),
            true,
            [info(), authorized(json!(true)), identity()],
        )
        .await;
        let before = w.http.calls().len();
        let id = w.start(Direction::Uplink).await;
        assert_eq!(w.phase(), Phase::Ready);
        assert_eq!(w.http.calls().len(), before + 1);
        let done = w.check(id).await.unwrap().unwrap();
        assert_eq!(done.status, LoginStatus::Authenticated);
        assert!(done.sms.is_none());
        let calls = w.http.calls();
        for (at, name) in [(before, "getUpSmsInfo"), (before + 1, "verifyUpSms")] {
            frozen(jsk, name, &calls[at].path, &calls[at].payload);
            assert_eq!(calls[at].login_chain.as_deref(), jsk.then_some(""));
            assert_eq!(
                calls[at].music_tu.as_deref(),
                jsk.then_some("synthetic-sms-ticket")
            );
        }
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
            Some("synthetic-sms-refresh")
        );
        for secret in [
            "MUSIC_U",
            "synthetic-sms-ticket",
            "synthetic-sms-refresh",
            "synthetic-password",
        ] {
            assert!(!serde_json::to_string(&done).unwrap().contains(secret));
        }
    }
}
#[tokio::test]
async fn downlink_is_explicit_shares_cooldown_and_matches_original_wire() {
    for jsk in [false, true] {
        let w = World::new(
            jsk,
            json!("1"),
            true,
            [
                json!({"code":200}).into(),
                json!({"code":400}).into(),
                authorized(json!({"success":false})),
                authorized(json!({"success":true})),
                identity(),
            ],
        )
        .await;
        let before = w.http.calls().len();
        w.start(Direction::Downlink).await;
        assert_eq!(
            w.http.calls().len(),
            before,
            "selection must not send a code"
        );
        w.client
            .login_command(&w.id, Cmd::VerificationSmsSend)
            .await
            .unwrap();
        assert_eq!(w.phase(), Phase::CodeSent);
        assert!(
            w.client
                .login_command(&w.id, Cmd::VerificationSmsSend)
                .await
                .is_err()
        );
        assert_eq!(
            w.phase(),
            Phase::CodeSent,
            "cooldown is not an unknown write"
        );
        for _ in 0..2 {
            let wrong = w
                .client
                .login_command(
                    &w.id,
                    Cmd::VerificationSmsVerify {
                        code: "123456".into(),
                    },
                )
                .await
                .unwrap();
            assert_eq!(wrong.status, LoginStatus::TwoFactor);
            assert!(wrong.sms.unwrap().answer_rejected);
        }
        let done = w
            .client
            .login_command(
                &w.id,
                Cmd::VerificationSmsVerify {
                    code: "123456".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(done.status, LoginStatus::Authenticated);
        let calls = w.http.calls();
        frozen(
            jsk,
            "sendDownSms",
            &calls[before].path,
            &calls[before].payload,
        );
        frozen(
            jsk,
            "verifyDownSms",
            &calls[before + 3].path,
            &calls[before + 3].payload,
        );
        assert_eq!(
            calls
                .iter()
                .filter(|c| c.path == "/api/w/nuser/account/get")
                .count(),
            1
        );
    }
}
#[tokio::test]
async fn country_and_method_come_from_server_never_frontend_phone_input() {
    for country in [Value::Null, json!("1"), json!(86), json!("+86")] {
        let w = World::new(true, country, true, []).await;
        let before = w.http.calls().len();
        assert!(
            w.client
                .login_command(
                    &w.id,
                    Cmd::SmsStart {
                        direction: Direction::Uplink
                    }
                )
                .await
                .is_err()
        );
        w.start(Direction::Downlink).await;
        assert_eq!(w.http.calls().len(), before);
    }
    let w = World::new(false, json!("86"), false, []).await;
    for direction in [Direction::Uplink, Direction::Downlink] {
        assert!(
            w.client
                .login_command(&w.id, Cmd::SmsStart { direction })
                .await
                .is_err()
        );
    }
    let w = World::new(true, json!("86"), true, [info()]).await;
    let view = w.client.login_snapshot(&w.id).unwrap();
    assert_eq!(view.challenge.phone_mask.as_deref(), Some("138****8000"));
    w.start(Direction::Uplink).await;
    assert_eq!(
        w.http.calls().last().unwrap().payload["cellphone"],
        "138****8000"
    );
}
#[tokio::test]
async fn explicit_refresh_gets_new_info_and_alternates_primary_backup_numbers() {
    let w = World::new(true, json!("86"), true, [info(), info(), info(), info()]).await;
    let mut id = w.start(Direction::Uplink).await;
    for (number, refresh) in [
        ("1069000001", true),
        ("1069000002", true),
        ("1069000001", false),
    ] {
        let snapshot = w.client.login_snapshot(&w.id).unwrap().sms.unwrap();
        assert_eq!(snapshot.upload_number.as_deref(), Some(number));
        if refresh {
            let next = w
                .client
                .login_command(
                    &w.id,
                    Cmd::SmsRefresh {
                        verification_id: id.clone(),
                    },
                )
                .await
                .unwrap()
                .sms
                .unwrap();
            assert_ne!(id, next.id);
            id = next.id;
        }
    }
    w.client
        .login_command(
            &w.id,
            Cmd::SmsStop {
                verification_id: id,
            },
        )
        .await
        .unwrap();
    let id = w.start(Direction::Uplink).await;
    assert!(!id.is_empty());
    assert_eq!(
        w.client
            .login_snapshot(&w.id)
            .unwrap()
            .sms
            .unwrap()
            .upload_number
            .as_deref(),
        Some("1069000001")
    );
}
#[tokio::test(start_paused = true)]
async fn false_data_polls_three_seconds_and_times_out_without_identity_or_replay() {
    let mut responses = vec![info()];
    responses.extend((0..10).map(|_| authorized(json!(false))));
    let w = World::new(true, json!("86"), true, responses).await;
    let id = w.start(Direction::Uplink).await;
    let before = w.http.calls().len();
    let checking = w.check(id);
    w.http.wait(before + 1).await;
    for n in 1..10 {
        tokio::time::advance(Duration::from_secs(3)).await;
        w.http.wait(before + n + 1).await;
    }
    tokio::time::advance(Duration::from_secs(3)).await;
    let done = checking.await.unwrap().unwrap();
    assert_eq!(done.sms.unwrap().phase, Phase::TimedOut);
    assert_eq!(w.http.calls().len(), before + 10);
    assert_ne!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
    assert!(
        w.http
            .calls()
            .iter()
            .all(|c| c.path != "/api/w/nuser/account/get")
    );
}
#[tokio::test(start_paused = true)]
async fn transport_status_reads_retry_but_explicit_business_failure_stops() {
    let w = World::new(
        true,
        json!("86"),
        true,
        [
            info(),
            HttpResponse::Ready(Err(Error::Transport("synthetic read failure".into()))),
            json!({"code":200,"data":false}).into(),
            authorized(json!({"success":false})),
        ],
    )
    .await;
    let id = w.start(Direction::Uplink).await;
    let before = w.http.calls().len();
    let checking = w.check(id);
    w.http.wait(before + 1).await;
    tokio::time::advance(Duration::from_secs(3)).await;
    w.http.wait(before + 2).await;
    tokio::time::advance(Duration::from_secs(3)).await;
    let done = checking.await.unwrap().unwrap();
    assert_eq!(done.sms.unwrap().phase, Phase::Failed);
    assert_eq!(w.http.calls().len(), before + 3);
}
#[tokio::test]
async fn stopping_held_check_drops_transport_and_releases_gate_for_new_login() {
    let (held, late) = gate();
    let w = World::new(true, json!("86"), true, [info(), held]).await;
    let id = w.start(Direction::Uplink).await;
    let before = w.http.calls().len();
    let checking = w.check(id.clone());
    w.http.wait(before + 1).await;
    w.client
        .login_command(
            &w.id,
            Cmd::SmsStop {
                verification_id: id,
            },
        )
        .await
        .unwrap();
    assert!(matches!(checking.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        late.send(Ok(reply(json!({"code":200,"data":true}))))
            .is_err(),
        "cancel drops the actual pending transport future"
    );
    assert!(w.client.login_snapshot(&w.id).unwrap().sms.is_none());
    let next = tokio::time::timeout(
        Duration::from_secs(1),
        w.client
            .begin_login(&w.client.account().unwrap().epoch, LoginMode::Email, None),
    )
    .await
    .unwrap()
    .unwrap();
    assert_ne!(next.operation_id, w.id);
    assert_ne!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
}
#[tokio::test]
async fn full_cancel_also_stops_pending_check_and_cannot_adopt_late_authorization() {
    let (held, late) = gate();
    let w = World::new(false, json!("86"), true, [info(), held]).await;
    let id = w.start(Direction::Uplink).await;
    let before = w.http.calls().len();
    let checking = w.check(id);
    w.http.wait(before + 1).await;
    w.client.cancel_login(&w.id).unwrap();
    assert!(checking.await.unwrap().is_err());
    assert!(
        late.send(Ok(reply(json!({"code":200,"data":true}))))
            .is_err()
    );
    assert_ne!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
}
#[tokio::test]
async fn acknowledged_completion_reads_identity_before_one_original_login_replay() {
    for jsk in [false, true] {
        let w = World::new(
            jsk,
            json!("86"),
            true,
            [
                info(),
                json!({"code":200,"data":true}).into(),
                json!({"code":301}).into(),
                authorized(json!({})),
                identity(),
            ],
        )
        .await;
        let id = w.start(Direction::Uplink).await;
        let done = w.check(id).await.unwrap().unwrap();
        assert_eq!(done.status, LoginStatus::Authenticated);
        let calls = w.http.calls();
        let paths: Vec<_> = calls
            .iter()
            .rev()
            .take(4)
            .map(|c| c.path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec![
                "/api/w/nuser/account/get",
                if jsk { "/api/login" } else { "/api/w/login" },
                "/api/w/nuser/account/get",
                "/api/middle/account/uplinkcaptcha/verify"
            ]
        );
    }
}
#[tokio::test]
async fn lost_downlink_answer_requires_read_reconciliation_and_cannot_resubmit() {
    let mut partial = reply(json!({"code":200}));
    partial.set_cookies = vec!["MUSIC_U=synthetic-pending; Path=/; Secure; HttpOnly".into()];
    partial.refresh_token = Some("must-not-install".into());
    partial.body_status = cumulus_core::protocol::http::BodyStatus::Interrupted;
    let w = World::new(
        true,
        json!("86"),
        true,
        [
            json!({"code":400}).into(),
            HttpResponse::Ready(Ok(partial)),
            identity(),
        ],
    )
    .await;
    w.start(Direction::Downlink).await;
    let wrong = w
        .client
        .login_command(
            &w.id,
            Cmd::VerificationSmsVerify {
                code: "000000".into(),
            },
        )
        .await
        .unwrap();
    assert!(wrong.sms.unwrap().answer_rejected);
    assert!(
        w.client
            .login_command(
                &w.id,
                Cmd::VerificationSmsVerify {
                    code: "123456".into()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(w.phase(), Phase::RecoveryRequired);
    assert!(
        !w.client
            .login_snapshot(&w.id)
            .unwrap()
            .sms
            .unwrap()
            .answer_rejected,
        "an unknown new result must not reuse the previous wrong-code feedback"
    );
    let before = w.http.calls().len();
    assert!(
        w.client
            .login_command(
                &w.id,
                Cmd::VerificationSmsVerify {
                    code: "123456".into()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(w.http.calls().len(), before);
    let done = w.client.login_command(&w.id, Cmd::Reconcile).await.unwrap();
    assert_eq!(done.status, LoginStatus::Authenticated);
}

#[tokio::test]
async fn original_login_replay_remains_alive_while_its_real_http_reply_is_pending() {
    for direction in [Direction::Uplink, Direction::Downlink] {
        let (held, release) = gate();
        let mut replies = Vec::new();
        if direction == Direction::Uplink {
            replies.push(info());
        }
        replies.extend([
            json!({"code":200,"data":true}).into(),
            json!({"code":301}).into(),
            held,
            identity(),
        ]);
        let w = World::new(true, json!("86"), true, replies).await;
        let id = w.start(direction).await;
        let before = w.http.calls().len();
        let client = w.client.clone();
        let operation = w.id.clone();
        let pending = tokio::spawn(async move {
            client
                .login_command(
                    &operation,
                    if direction == Direction::Uplink {
                        Cmd::SmsCheck {
                            verification_id: id,
                        }
                    } else {
                        Cmd::VerificationSmsVerify {
                            code: "123456".into(),
                        }
                    },
                )
                .await
        });
        w.http.wait(before + 3).await;
        tokio::task::yield_now().await;
        assert!(
            !pending.is_finished(),
            "clearing the SMS step must not cancel its own acknowledged login replay"
        );
        let mut response = reply(json!({"code":200}));
        response.set_cookies = vec!["MUSIC_U=synthetic-replayed; Path=/; Secure; HttpOnly".into()];
        release.send(Ok(response)).map_err(|_| ()).unwrap();
        assert_eq!(
            pending.await.unwrap().unwrap().status,
            LoginStatus::Authenticated
        );
    }
}

#[tokio::test]
async fn dropped_native_sms_tasks_release_busy_state_into_read_only_recovery() {
    for direction in [Direction::Uplink, Direction::Downlink] {
        let (held, release) = gate();
        let mut responses = Vec::new();
        if direction == Direction::Uplink {
            responses.push(info());
        }
        responses.push(held);
        let w = World::new(true, json!("86"), true, responses).await;
        let id = w.start(direction).await;
        let before = w.http.calls().len();
        let client = w.client.clone();
        let operation = w.id.clone();
        let op_id = id.clone();
        let pending = tokio::spawn(async move {
            client
                .login_command(
                    &operation,
                    if direction == Direction::Uplink {
                        Cmd::SmsCheck {
                            verification_id: op_id,
                        }
                    } else {
                        Cmd::VerificationSmsVerify {
                            code: "123456".into(),
                        }
                    },
                )
                .await
        });
        w.http.wait(before + 1).await;
        pending.abort();
        assert!(matches!(pending.await, Err(error) if error.is_cancelled()));
        let state = w.client.login_snapshot(&w.id).unwrap();
        assert!(!state.busy);
        assert!(state.interrupted);
        assert_eq!(state.sms.unwrap().phase, Phase::RecoveryRequired);
        assert!(
            release
                .send(Ok(reply(json!({"code":200,"data":true}))))
                .is_err()
        );
        let calls = w.http.calls().len();
        assert!(
            w.client
                .login_command(
                    &w.id,
                    if direction == Direction::Uplink {
                        Cmd::SmsCheck {
                            verification_id: id,
                        }
                    } else {
                        Cmd::VerificationSmsVerify {
                            code: "123456".into(),
                        }
                    }
                )
                .await
                .is_err()
        );
        assert_eq!(
            w.http.calls().len(),
            calls,
            "an interrupted task cannot silently replay its submission"
        );
    }
}
#[tokio::test]
async fn malformed_info_is_not_ready_and_explicit_manual_refresh_can_recover() {
    let w = World::new(
        true,
        json!("86"),
        true,
        [
            json!({"code":200,"data":{"uploadText":"x","uploadNumber":"bad\nnumber"}}).into(),
            info(),
        ],
    )
    .await;
    assert!(
        w.client
            .login_command(
                &w.id,
                Cmd::SmsStart {
                    direction: Direction::Uplink
                }
            )
            .await
            .is_err()
    );
    assert_eq!(w.phase(), Phase::RecoveryRequired);
    let id = w.client.login_snapshot(&w.id).unwrap().sms.unwrap().id;
    let next = w
        .client
        .login_command(
            &w.id,
            Cmd::SmsRefresh {
                verification_id: id,
            },
        )
        .await
        .unwrap();
    assert_eq!(next.sms.unwrap().phase, Phase::Ready);
}
