mod support;
use cumulus_core::{
    account::{
        login::{LoginStatus, presentation::LoginDocument},
        profiles::ProfileRegistry,
    },
    client::{
        MusicClient,
        dto::{LoginCommand as Cmd, LoginMode},
    },
    protocol::http::ApiOrigin,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use support::http::{HttpResponse, ScriptHttp, gate, reply};

async fn world(
    replies: impl IntoIterator<Item = HttpResponse>,
    jsk: bool,
) -> (tempfile::TempDir, Arc<ScriptHttp>, Arc<MusicClient>, String) {
    let root = tempfile::tempdir().unwrap();
    let http = Arc::new(ScriptHttp::new(
        std::iter::once(
            json!({"code":200,"profile":{"userId":1,"nickname":"synthetic old"}}).into(),
        )
        .chain(replies),
    ));
    let client = MusicClient::open_with_http_backend(root.path(), None, http.clone()).unwrap();
    let client = Arc::new(if jsk {
        client.with_official_login_policy()
    } else {
        client
    });
    let epoch = client.restore_account().await.unwrap().epoch;
    let id = client
        .begin_login(&epoch, LoginMode::Email, None)
        .await
        .unwrap()
        .operation_id;
    (root, http, client, id)
}
fn email() -> Cmd {
    Cmd::Email {
        address: "synthetic@example.test".into(),
        password: "synthetic-password".into(),
        remember: true,
    }
}
fn flags() -> HttpResponse {
    json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":"t1"},{"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}]}).into()
}
fn penalty() -> HttpResponse {
    json!({"code":8831,"data":{"musicTuP":"native-only-ticket","steps":[{"type":"IDENTITY_VERIFY","state":"PENDING"}]}}).into()
}

#[tokio::test]
async fn notices_follow_exact_official_toast_fields_without_changing_the_active_account() {
    for (response, status, kind, title) in [
        (
            json!({"code":8810,"toast":{"type":3,"title":"合成风险","content":["段落一","段落二"],"url":"https://example.invalid/?private=DO_NOT_EXPORT","private":"DO_NOT_EXPORT"}}),
            LoginStatus::RiskNotice,
            "risk",
            "合成风险",
        ),
        (
            json!({"code":10004,"toast":{"type":3,"title":"合成风险","content":["段落一","段落二"]}}),
            LoginStatus::RiskNotice,
            "risk",
            "合成风险",
        ),
        (
            json!({"code":8810,"toast":{"type":2,"title":"DO_NOT_EXPORT","url":"https://example.invalid/?private=DO_NOT_EXPORT"}}),
            LoginStatus::UpgradeRequired,
            "upgrade",
            "版本更新提醒",
        ),
        (
            json!({"code":8820}),
            LoginStatus::UpgradeRequired,
            "upgrade",
            "版本更新提醒",
        ),
        (
            json!({"code":304,"message":"永久封禁：DO_NOT_EXPORT"}),
            LoginStatus::AccountDisabled,
            "disabled",
            "用户禁止登录",
        ),
    ] {
        let (_root, http, client, id) = world([response.into()], false).await;
        let view = client.login_command(&id, email()).await.unwrap();
        assert_eq!(view.status, status);
        let dto = serde_json::to_value(view).unwrap();
        assert_eq!(dto["notice"]["kind"], kind);
        assert_eq!(dto["notice"]["title"], title);
        if kind == "risk" {
            assert_eq!(dto["notice"]["paragraphs"], json!(["段落一", "段落二"]));
        }
        assert!(!dto.to_string().contains("DO_NOT_EXPORT"));
        assert!(!dto.to_string().contains("synthetic-password"));
        assert_eq!(
            client.account().unwrap().nickname.as_deref(),
            Some("synthetic old")
        );
        assert_eq!(http.calls().len(), 2);
        assert!(client.login_command(&id, Cmd::Reconcile).await.is_err());
        assert_eq!(
            http.calls().len(),
            2,
            "a known login restriction cannot be bypassed by an identity read"
        );
    }
}

#[tokio::test]
async fn malformed_penalty_or_security_ticket_cannot_fall_back_to_identity_adoption() {
    for response in [
        json!({"code":8831,"data":{}}),
        json!({"code":8810,"toast":{"type":1,"unikey":{}}}),
    ] {
        let (_root, http, client, id) = world([response.into()], false).await;
        assert!(client.login_command(&id, email()).await.is_err());
        assert!(client.login_command(&id, Cmd::Reconcile).await.is_err());
        assert_eq!(http.calls().len(), 2);
        assert_eq!(
            client.account().unwrap().nickname.as_deref(),
            Some("synthetic old")
        );
    }
}

#[tokio::test]
async fn invalid_notice_data_cannot_export_authorization_links_html_objects_or_unbounded_text() {
    let (_root,_,client,id)=world([json!({"code":8810,"toast":{"type":3,"title":"https://example.invalid/login?private=DO_NOT_EXPORT","content":[{"text":"DO_NOT_EXPORT"},"https://example.invalid/?secret=DO_NOT_EXPORT","x".repeat(513),"safe plain text","bad\u{0000}text"]}}).into()],false).await;
    let value = serde_json::to_value(client.login_command(&id, email()).await.unwrap()).unwrap();
    assert_eq!(value["notice"]["title"], "登录风险提示");
    assert_eq!(value["notice"]["paragraphs"], json!(["safe plain text"]));
    assert!(!value.to_string().contains("DO_NOT_EXPORT"));
    for (document, url) in [
        (
            LoginDocument::ClientRelease,
            "https://github.com/RC-CHN/Cumulus-music/releases",
        ),
        (
            LoginDocument::AppDownload,
            "https://music.163.com/#/download",
        ),
    ] {
        assert_eq!(document.url(), url);
    }
}

#[tokio::test]
async fn penalty_check_uses_only_status_and_identity_reads_and_never_replays_old_password() {
    let (_root,http,client,id)=world([flags(),penalty(),json!({"code":200,"data":{"steps":[{"type":"IDENTITY_VERIFY","state":"PASSED"},{"type":"UPGRADE_WEAK_PASSWORD","state":"PASSED"}]}}).into(),json!({"code":301}).into()],true).await;
    let view = client.login_command(&id, email()).await.unwrap();
    assert_eq!(view.status, LoginStatus::Penalty);
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("native-only-ticket")
    );
    let attempt = view.penalty.unwrap().id;
    assert!(
        client
            .login_command(
                &id,
                Cmd::PenaltyCheck {
                    verification_id: "stale".into()
                }
            )
            .await
            .is_err()
    );
    assert!(
        client
            .login_command(
                &id,
                Cmd::PenaltyCheck {
                    verification_id: attempt
                }
            )
            .await
            .is_err()
    );
    let snapshot = client.login_snapshot(&id).unwrap();
    assert_eq!(
        serde_json::to_value(snapshot.penalty).unwrap()["phase"],
        "status_passed"
    );
    let calls = http.calls();
    assert_eq!(calls.iter().filter(|v| v.path == "/api/login").count(), 1);
    assert_eq!(
        calls
            .iter()
            .filter(|v| v.path == "/api/login/penalty/status")
            .count(),
        1
    );
    let status = calls
        .iter()
        .find(|v| v.path == "/api/login/penalty/status")
        .unwrap();
    assert_eq!(status.payload["fromPage"], "JSK");
    assert_eq!(status.login_chain.as_deref(), Some(""));
    for absent in ["musicTuP", "password", "ignorePenalty", "stepType"] {
        assert!(status.payload.get(absent).is_none());
    }
    assert_eq!(
        client.account().unwrap().nickname.as_deref(),
        Some("synthetic old")
    );
}

#[tokio::test]
async fn status_endpoint_is_not_a_login_completion_and_only_all_passed_with_identity_can_adopt() {
    let mut response = reply(json!({"code":200,"data":{"steps":[]}}));
    response.set_cookies = vec!["MUSIC_U=synthetic-candidate; Path=/; Secure".into()];
    response.refresh_token = Some("must-not-install-status-header".into());
    let (root, http, client, id) = world(
        [
            flags(),
            penalty(),
            json!({"code":200,"data":{"steps":[{"type":"IDENTITY_VERIFY","state":"PENDING"}]}})
                .into(),
            HttpResponse::Ready(Ok(response)),
            json!({"code":200,"profile":{"userId":2,"nickname":"synthetic new"}}).into(),
        ],
        true,
    )
    .await;
    let view = client.login_command(&id, email()).await.unwrap();
    let attempt = view.penalty.unwrap().id;
    let pending = client.login_command(&id, Cmd::Reconcile).await.unwrap();
    assert_eq!(pending.status, LoginStatus::Penalty);
    assert_eq!(http.calls().len(), 4);
    let authenticated = client
        .login_command(
            &id,
            Cmd::PenaltyCheck {
                verification_id: attempt,
            },
        )
        .await
        .unwrap();
    assert_eq!(authenticated.status, LoginStatus::Authenticated);
    assert!(authenticated.penalty.is_none());
    let selected = ProfileRegistry::open(root.path(), ApiOrigin::Pc)
        .unwrap()
        .current(Default::default())
        .unwrap()
        .unwrap();
    assert!(
        selected
            .credentials
            .login_credentials()
            .unwrap()
            .refresh_token
            .is_none()
    );
    assert_eq!(
        client.account().unwrap().nickname.as_deref(),
        Some("synthetic new")
    );
}

#[tokio::test]
async fn cancelling_penalty_status_drops_inflight_read_without_adoption_or_credential_replay() {
    let (blocked, mut sender) = gate();
    let (_root, http, client, id) = world([flags(), penalty(), blocked], true).await;
    let view = client.login_command(&id, email()).await.unwrap();
    let attempt = view.penalty.unwrap().id;
    let task = {
        let client = client.clone();
        let id = id.clone();
        tokio::spawn(async move {
            client
                .login_command(
                    &id,
                    Cmd::PenaltyCheck {
                        verification_id: attempt,
                    },
                )
                .await
        })
    };
    http.wait(4).await;
    client.cancel_login(&id).unwrap();
    assert!(task.await.unwrap().is_err());
    sender.closed().await;
    assert_eq!(http.calls().len(), 4);
    assert_eq!(
        client.account().unwrap().nickname.as_deref(),
        Some("synthetic old")
    );
}

#[tokio::test]
async fn cancelling_penalty_identity_releases_the_client_gate_before_any_late_adoption() {
    let (identity, mut sender) = gate();
    let (_root, http, client, id) = world(
        [
            penalty(),
            json!({"code":200,"data":{"steps":[]}}).into(),
            identity,
        ],
        false,
    )
    .await;
    let old = client.account().unwrap();
    assert_eq!(
        old.status,
        cumulus_core::account::SessionStatus::Authenticated
    );
    assert_eq!(old.nickname.as_deref(), Some("synthetic old"));
    let view = client.login_command(&id, email()).await.unwrap();
    let verification_id = view.penalty.unwrap().id;
    let check = {
        let client = client.clone();
        let id = id.clone();
        tokio::spawn(async move {
            client
                .login_command(&id, Cmd::PenaltyCheck { verification_id })
                .await
        })
    };
    http.wait(4).await;
    assert_eq!(http.calls()[3].path, "/api/w/nuser/account/get");
    assert_eq!(client.account().unwrap().epoch, old.epoch);
    let next = {
        let client = client.clone();
        let epoch = old.epoch.clone();
        tokio::spawn(async move { client.begin_login(&epoch, LoginMode::Email, None).await })
    };
    tokio::task::yield_now().await;
    assert!(
        !next.is_finished(),
        "identity read still owns the client transition gate"
    );
    client.cancel_login(&id).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), check)
            .await
            .expect("cancel must end the entire penalty command")
            .unwrap()
            .is_err()
    );
    let next = tokio::time::timeout(Duration::from_secs(1), next)
        .await
        .expect("cancel must release the client transition gate")
        .unwrap()
        .unwrap();
    assert_eq!(next.status, LoginStatus::Ready);
    assert_ne!(next.operation_id, id);
    tokio::time::timeout(Duration::from_secs(1), sender.closed())
        .await
        .expect("cancel must drop the old identity response receiver");
    assert!(
        sender
            .send(Ok(reply(json!({
                "code":200,"profile":{"userId":2,"nickname":"must not be adopted"}
            }))))
            .is_err()
    );
    let retained = client.account().unwrap();
    assert_eq!(retained.epoch, old.epoch);
    assert_eq!(retained.nickname, old.nickname);
    let calls = http.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[2].path, "/api/login/penalty/status");
    assert_eq!(calls[2].payload["fromPage"], "JSK");
    assert_eq!(calls[2].login_chain.as_deref(), Some(""));
    assert!(calls[2].music_tu.is_none());
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.path == "/api/w/login")
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.path == "/api/login/penalty/status")
            .count(),
        1
    );
}

#[tokio::test(start_paused = true)]
async fn penalty_status_and_identity_share_one_deadline_and_timeout_releases_the_client_gate() {
    let (status_read, status_sender) = gate();
    let (identity, mut identity_sender) = gate();
    let (_root, http, client, id) = world([penalty(), status_read, identity], false).await;
    let old = client.account().unwrap();
    let view = client.login_command(&id, email()).await.unwrap();
    let verification_id = view.penalty.unwrap().id;
    let started = tokio::time::Instant::now();
    let check = {
        let client = client.clone();
        let id = id.clone();
        tokio::spawn(async move {
            client
                .login_command(&id, Cmd::PenaltyCheck { verification_id })
                .await
        })
    };
    http.wait(3).await;
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(
        status_sender
            .send(Ok(reply(json!({"code":200,"data":{"steps":[]}}))))
            .is_ok()
    );
    http.wait(4).await;
    assert_eq!(http.calls()[3].path, "/api/w/nuser/account/get");
    tokio::time::advance(Duration::from_secs(14)).await;
    tokio::task::yield_now().await;
    assert!(
        !check.is_finished(),
        "the shared deadline has not elapsed at 44 seconds"
    );
    tokio::time::advance(Duration::from_secs(1)).await;
    let result = tokio::time::timeout(Duration::from_secs(1), check)
        .await
        .expect("the identity read must not receive a second 45-second deadline")
        .unwrap();
    assert!(matches!(result, Err(cumulus_core::Error::Transport(_))));
    assert_eq!(
        tokio::time::Instant::now() - started,
        Duration::from_secs(45)
    );
    tokio::time::timeout(Duration::from_secs(1), identity_sender.closed())
        .await
        .expect("deadline must drop the identity response receiver");
    assert!(
        identity_sender
            .send(Ok(reply(json!({
                "code":200,"profile":{"userId":2,"nickname":"must not be adopted"}
            }))))
            .is_err()
    );
    let next = tokio::time::timeout(
        Duration::from_secs(1),
        client.begin_login(&old.epoch, LoginMode::Email, None),
    )
    .await
    .expect("deadline must release the client transition gate")
    .unwrap();
    assert_eq!(next.status, LoginStatus::Ready);
    assert_eq!(client.account().unwrap().epoch, old.epoch);
    assert_eq!(client.account().unwrap().nickname, old.nickname);
    assert_eq!(http.calls().len(), 4);
    assert_eq!(
        http.calls()
            .iter()
            .filter(|call| call.path == "/api/w/login")
            .count(),
        1
    );
}
