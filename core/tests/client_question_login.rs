mod support;
use cumulus_core::{
    Error,
    account::{
        SessionStatus,
        login::{LoginStatus, QuestionPhase as Phase},
        profiles::ProfileRegistry,
    },
    client::{
        MusicClient,
        dto::{LoginCommand as Cmd, LoginMode, LoginView},
    },
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{ApiOrigin, BodyStatus},
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use support::http::{HttpResponse, ScriptHttp, gate, reply};
const READ: &str = "/api/login/cellphone/twice/question/get";
const VERIFY: &str = "/api/login/cellphone/twice/question/verify";
#[tokio::test(start_paused = true)]
async fn readonly_reconciliation_is_cancellable_bounded_and_never_replays_the_answer() {
    for action in ["cancel", "timeout", "abort"] {
        let HttpResponse::Ready(Ok(mut partial)) = authorized() else {
            unreachable!()
        };
        partial.body_status = BodyStatus::Interrupted;
        let (pending, mut response) = gate();
        let w = World::new(
            true,
            [question(1), HttpResponse::Ready(Ok(partial)), pending],
        )
        .await;
        let id = w.read().await.unwrap().question.unwrap().id;
        assert!(w.answer(&id, "none").await.is_err());
        let before = w.http.calls().len();
        let client = w.client.clone();
        let operation = w.id.clone();
        let task =
            tokio::spawn(async move { client.login_command(&operation, Cmd::Reconcile).await });
        w.http.wait(before + 1).await;
        match action {
            "cancel" => {
                w.client.cancel_login(&w.id).unwrap();
                tokio::time::timeout(Duration::from_secs(1), response.closed())
                    .await
                    .expect("closing must drop the read without waiting for network timeout");
                assert!(task.await.unwrap().is_err());
                assert!(matches!(
                    w.client.login_snapshot(&w.id),
                    Err(Error::StaleResponse)
                ));
            }
            "timeout" => {
                tokio::time::advance(Duration::from_secs(46)).await;
                assert!(
                    tokio::time::timeout(Duration::from_secs(1), task)
                        .await
                        .expect("reconciliation deadline")
                        .unwrap()
                        .is_err()
                );
                response.closed().await;
                assert!(w.phase() == Phase::RecoveryRequired);
                assert!(!w.client.login_snapshot(&w.id).unwrap().busy);
            }
            _ => {
                task.abort();
                assert!(task.await.err().expect("aborted").is_cancelled());
                response.closed().await;
                assert!(w.phase() == Phase::RecoveryRequired);
                assert!(!w.client.login_snapshot(&w.id).unwrap().busy);
            }
        }
        assert_eq!(w.calls(VERIFY), 1);
        assert_eq!(w.calls("/api/w/nuser/account/get"), 1);
        assert_ne!(
            w.client.account().unwrap().status,
            SessionStatus::Authenticated
        );
    }
}
#[tokio::test]
async fn a_new_question_challenge_invalidates_the_old_question_without_automatically_reading_or_answering()
 {
    let w = World::new(
        true,
        [question(1), json!({"code":8861}).into(), question(3)],
    )
    .await;
    let old = w.read().await.unwrap().question.unwrap().id;
    let next = w.answer(&old, "none").await.unwrap();
    assert_eq!(next.status, LoginStatus::SecurityQuestion);
    assert!(next.question.is_none() && next.challenge.answers.is_empty());
    assert_eq!(w.calls(READ), 1);
    assert_eq!(w.calls(VERIFY), 1);
    let new = w.read().await.unwrap().question.unwrap().id;
    assert_ne!(old, new);
    assert!(w.answer(&old, "none").await.is_err());
    assert_eq!(w.calls(VERIFY), 1);
}
#[tokio::test]
async fn proven_pretransport_failure_keeps_the_same_question_and_allows_an_explicit_answer_retry() {
    struct Once(AtomicBool);
    impl TokenProvider for Once {
        fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
            Box::pin(async move {
                if self.0.swap(false, Ordering::SeqCst) {
                    Err(Error::State("synthetic provider offline"))
                } else {
                    Ok("synthetic-proof".into())
                }
            })
        }
    }
    let context = Arc::new(RequestContext::new(
        json!({"yidun#paths":{"whiteList":[{"businessId":"fixture","list":[VERIFY]}]}}),
        ApiOrigin::Pc.as_str().into(),
        Arc::new(Once(AtomicBool::new(true))),
    ));
    let w = World::with_context(true, [question(1), authorized(), identity()], Some(context)).await;
    let id = w.read().await.unwrap().question.unwrap().id;
    let e = w
        .answer(&id, "none")
        .await
        .err()
        .expect("provider must fail");
    assert!(matches!(e.root(), Error::RequestNotSent(_)));
    assert!(w.phase() == Phase::Ready);
    assert_eq!(w.calls(VERIFY), 0);
    assert_eq!(
        w.answer(&id, "none").await.unwrap().status,
        LoginStatus::Authenticated
    );
    assert_eq!(w.calls(READ), 1);
    assert_eq!(w.calls(VERIFY), 1);
}
#[tokio::test]
async fn original_login_replay_remains_alive_until_success_or_explicit_cancellation() {
    for cancel in [false, true] {
        let (pending, mut response) = gate();
        let w = World::new(
            true,
            [
                question(1),
                json!({"code":200}).into(),
                json!({"code":301}).into(),
                pending,
                identity(),
            ],
        )
        .await;
        let id = w.read().await.unwrap().question.unwrap().id;
        let before = w.http.calls().len();
        let client = w.client.clone();
        let operation = w.id.clone();
        let task = tokio::spawn(async move {
            client
                .login_command(
                    &operation,
                    Cmd::QuestionAnswer {
                        question_id: id,
                        answer: "none".into(),
                    },
                )
                .await
        });
        w.http.wait(before + 3).await;
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        if cancel {
            w.client.cancel_login(&w.id).unwrap();
            tokio::time::timeout(Duration::from_secs(1), response.closed())
                .await
                .unwrap();
            assert!(task.await.unwrap().is_err());
            assert_ne!(
                w.client.account().unwrap().status,
                SessionStatus::Authenticated
            );
        } else {
            let HttpResponse::Ready(r) = authorized() else {
                unreachable!()
            };
            assert!(response.send(r).is_ok());
            assert_eq!(
                task.await.unwrap().unwrap().status,
                LoginStatus::Authenticated
            );
        }
        assert_eq!(w.calls("/api/login"), 2);
        assert_eq!(w.calls(VERIFY), 1);
    }
}
fn question(kind: u8) -> HttpResponse {
    json!({"code":200,"data":{"title":"使用过哪个设备？","questionType":kind,"options":[{"title":"合成设备甲","subtitle":"仅用于测试","pic":"http://p1.music.126.net/a.jpg?private=DO_NOT_EXPORT","tag":"https://p2.music.126.net/t.png?secret=DO_NOT_EXPORT"},{"title":"合成设备乙","pic":"https://bad.example.test/?secret=DO_NOT_EXPORT"}]},"private":"DO_NOT_EXPORT"}).into()
}
fn authorized() -> HttpResponse {
    let mut r = reply(json!({"code":200,"data":{"success":true}}));
    r.set_cookies = vec!["MUSIC_U=synthetic-question; Path=/; Secure; HttpOnly".into()];
    r.refresh_token = Some("synthetic-question-refresh".into());
    HttpResponse::Ready(Ok(r))
}
fn identity() -> HttpResponse {
    json!({"code":200,"profile":{"userId":2,"nickname":"合成问题账号"}}).into()
}
struct World {
    root: tempfile::TempDir,
    client: Arc<MusicClient>,
    http: Arc<ScriptHttp>,
    id: String,
}
impl World {
    async fn new(jsk: bool, replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        Self::with_context(jsk, replies, None).await
    }
    async fn with_context(
        jsk: bool,
        replies: impl IntoIterator<Item = HttpResponse>,
        context: Option<Arc<RequestContext>>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut all = vec![];
        if jsk {
            all.push(json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":"t1"},{"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}]}).into());
        }
        all.push(json!({"code":8861}).into());
        all.extend(replies);
        let http = Arc::new(ScriptHttp::new(all));
        let client = MusicClient::open_with_request_context(
            root.path(),
            None,
            Default::default(),
            Some(http.clone()),
            context,
        )
        .unwrap();
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
                    address: "me@example.test".into(),
                    password: "synthetic-password".into(),
                    remember: true,
                },
            )
            .await
            .unwrap();
        Self {
            root,
            client,
            http,
            id,
        }
    }
    async fn read(&self) -> cumulus_core::Result<LoginView> {
        self.client.login_command(&self.id, Cmd::QuestionRead).await
    }
    async fn answer(&self, id: &str, answer: &str) -> cumulus_core::Result<LoginView> {
        self.client
            .login_command(
                &self.id,
                Cmd::QuestionAnswer {
                    question_id: id.into(),
                    answer: answer.into(),
                },
            )
            .await
    }
    fn phase(&self) -> Phase {
        self.client
            .login_snapshot(&self.id)
            .unwrap()
            .question
            .unwrap()
            .phase
    }
    fn calls(&self, path: &str) -> usize {
        self.http.calls().iter().filter(|c| c.path == path).count()
    }
}
#[tokio::test]
async fn original_source_vectors_types_images_and_confirmed_identity_are_used_in_both_protocols() {
    let vectors: Value =
        serde_json::from_str(include_str!("fixtures/question-login-official.json")).unwrap();
    for jsk in [false, true] {
        for kind in [1, 2, 3] {
            let w = World::new(
                jsk,
                [
                    question(kind),
                    json!({"code":4012}).into(),
                    json!({"code":200,"data":{"success":false}}).into(),
                    authorized(),
                    identity(),
                ],
            )
            .await;
            let q = w.read().await.unwrap();
            let id = q.question.as_ref().unwrap().id.clone();
            assert!(w.phase() == Phase::Ready);
            assert_eq!(q.question.unwrap().question_type, Some(kind));
            assert_eq!(q.challenge.answers.len(), 3);
            assert_eq!(q.challenge.answers[0].value, "合成设备甲");
            assert_eq!(q.challenge.answers[2].value, "none");
            assert!(q.challenge.answers[1].picture.is_none());
            let public = serde_json::to_string(&q.challenge).unwrap();
            assert!(!public.contains("DO_NOT_EXPORT"));
            assert_eq!(
                q.challenge.answers[0].tag.as_deref(),
                Some("https://p2.music.126.net/t.png?param=320y320")
            );
            assert!(w.answer("stale", "合成设备甲").await.is_err());
            assert!(w.answer(&id, "injected").await.is_err());
            assert!(w.read().await.is_err());
            assert_eq!(w.calls(VERIFY), 0);
            assert!(
                w.answer(&id, "合成设备甲")
                    .await
                    .unwrap()
                    .question
                    .unwrap()
                    .answer_rejected
            );
            assert!(
                w.answer(&id, "合成设备甲")
                    .await
                    .unwrap()
                    .question
                    .unwrap()
                    .answer_rejected
            );
            assert_eq!(w.calls("/api/w/nuser/account/get"), 0);
            assert_eq!(
                w.answer(&id, "合成设备甲").await.unwrap().status,
                LoginStatus::Authenticated
            );
            let saved = ProfileRegistry::open(w.root.path(), ApiOrigin::Pc)
                .unwrap()
                .current(Default::default())
                .unwrap()
                .unwrap()
                .credentials
                .login_credentials()
                .unwrap();
            assert_eq!(
                saved.refresh_token.as_deref(),
                Some("synthetic-question-refresh")
            );
            assert_eq!(saved.last_login_at.is_some(), jsk);
            for (path, name) in [(READ, "Read"), (VERIFY, "Answer")] {
                let call = w.http.calls().into_iter().find(|c| c.path == path).unwrap();
                let expected =
                    &vectors["vectors"][format!("{}{name}", if jsk { "jsk" } else { "legacy" })];
                for (key, value) in expected["payload"].as_object().unwrap() {
                    assert_eq!(&call.payload[key], value);
                }
                assert_eq!(call.payload.get("scene").is_some(), jsk);
                assert_eq!(call.payload.get("fromPage").is_some(), jsk);
            }
        }
    }
}
#[tokio::test]
async fn read_failures_are_explicitly_retryable_and_old_ids_never_validate_new_questions() {
    let w = World::new(
        false,
        [
            HttpResponse::Ready(Err(Error::Transport("synthetic read loss".into()))),
            question(3),
            json!({"code":4012}).into(),
        ],
    )
    .await;
    assert!(w.read().await.is_err());
    assert!(w.phase() == Phase::Failed);
    let old = w.client.login_snapshot(&w.id).unwrap().question.unwrap().id;
    let new = w.read().await.unwrap().question.unwrap().id;
    assert_ne!(old, new);
    assert!(w.answer(&old, "none").await.is_err());
    assert_eq!(w.calls(VERIFY), 0);
    w.answer(&new, "none").await.unwrap();
    assert_eq!(w.calls(READ), 2);
    assert_eq!(w.calls(VERIFY), 1);
}
#[tokio::test]
async fn malformed_or_ambiguous_question_data_never_becomes_an_offered_answer() {
    for data in [
        json!({"title":"","options":[]}),
        json!({"title":"问题","options":[{"title":"none"}]}),
        json!({"title":"问题","options":[{"title":"重复"},{"title":"重复"}]}),
        json!({"title":"问题","options":vec![json!({"title":"超量"});65]}),
    ] {
        let w = World::new(false, [json!({"code":200,"data":data}).into(), question(1)]).await;
        assert!(w.read().await.is_err());
        assert!(w.phase() == Phase::Failed);
        assert!(
            w.client
                .login_snapshot(&w.id)
                .unwrap()
                .challenge
                .answers
                .is_empty()
        );
        let id = w.client.login_snapshot(&w.id).unwrap().question.unwrap().id;
        assert!(w.answer(&id, "none").await.is_err());
        assert_eq!(w.calls(VERIFY), 0);
        assert!(w.read().await.unwrap().question.unwrap().phase == Phase::Ready);
    }
}
#[tokio::test]
async fn an_interrupted_answer_keeps_only_the_cookie_and_requires_readonly_reconciliation() {
    for jsk in [false, true] {
        let HttpResponse::Ready(Ok(mut partial)) = authorized() else {
            unreachable!()
        };
        partial.body_status = BodyStatus::Interrupted;
        let w = World::new(
            jsk,
            [question(2), HttpResponse::Ready(Ok(partial)), identity()],
        )
        .await;
        let id = w.read().await.unwrap().question.unwrap().id;
        assert!(w.answer(&id, "none").await.is_err());
        assert!(w.phase() == Phase::RecoveryRequired);
        assert!(w.read().await.is_err());
        assert!(w.answer(&id, "none").await.is_err());
        assert_eq!(w.calls(VERIFY), 1);
        assert_ne!(
            w.client.account().unwrap().status,
            SessionStatus::Authenticated
        );
        assert_eq!(
            w.client
                .login_command(&w.id, Cmd::Reconcile)
                .await
                .unwrap()
                .status,
            LoginStatus::Authenticated
        );
        let saved = ProfileRegistry::open(w.root.path(), ApiOrigin::Pc)
            .unwrap()
            .current(Default::default())
            .unwrap()
            .unwrap()
            .credentials
            .login_credentials()
            .unwrap();
        assert!(
            saved.token.is_some() && saved.refresh_token.is_none() && saved.last_login_at.is_none()
        );
        assert_eq!(w.calls(VERIFY), 1);
    }
}
#[tokio::test]
async fn completed_answers_and_empty_questions_allow_exactly_one_acknowledged_primary_replay() {
    for empty in [false, true] {
        let mut responses = vec![];
        if empty {
            responses.push(json!({"code":200,"data":{}}).into());
        } else {
            responses.extend([question(1), json!({"code":200}).into()]);
        }
        responses.extend([json!({"code":301}).into(), json!({"code":8830}).into()]);
        let w = World::new(true, responses).await;
        let q = w.read().await.unwrap();
        let next = if empty {
            q
        } else {
            w.answer(&q.question.unwrap().id, "none").await.unwrap()
        };
        assert_eq!(next.status, LoginStatus::TwoFactor);
        assert!(next.question.is_none() && next.challenge.answers.is_empty());
        assert_eq!(w.calls("/api/login"), 2);
        assert_eq!(w.calls(VERIFY), usize::from(!empty));
    }
}
#[tokio::test]
async fn cancellation_releases_held_reads_and_answers_and_removes_the_candidate() {
    for read in [false, true] {
        let (pending, mut response) = gate();
        let mut replies = vec![];
        if !read {
            replies.push(question(1));
        }
        replies.push(pending);
        let w = World::new(true, replies).await;
        let q = if read {
            None
        } else {
            Some(w.read().await.unwrap().question.unwrap().id)
        };
        let before = w.http.calls().len();
        let client = w.client.clone();
        let id = w.id.clone();
        let task = tokio::spawn(async move {
            match q {
                Some(q) => {
                    client
                        .login_command(
                            &id,
                            Cmd::QuestionAnswer {
                                question_id: q,
                                answer: "none".into(),
                            },
                        )
                        .await
                }
                None => client.login_command(&id, Cmd::QuestionRead).await,
            }
        });
        w.http.wait(before + 1).await;
        w.client.cancel_login(&w.id).unwrap();
        tokio::time::timeout(Duration::from_secs(1), response.closed())
            .await
            .unwrap();
        assert!(task.await.unwrap().is_err());
        assert!(matches!(
            w.client.login_snapshot(&w.id),
            Err(Error::StaleResponse)
        ));
        assert_ne!(
            w.client.account().unwrap().status,
            SessionStatus::Authenticated
        );
    }
}
#[tokio::test(start_paused = true)]
async fn timeouts_and_dropped_tasks_keep_reads_retryable_but_answers_uncertain() {
    for read in [false, true] {
        for abort in [false, true] {
            let (pending, mut response) = gate();
            let mut replies = vec![];
            if !read {
                replies.push(question(1));
            }
            replies.push(pending);
            let w = World::new(true, replies).await;
            let q = if read {
                None
            } else {
                Some(w.read().await.unwrap().question.unwrap().id)
            };
            let before = w.http.calls().len();
            let client = w.client.clone();
            let id = w.id.clone();
            let task = tokio::spawn(async move {
                match q {
                    Some(q) => {
                        client
                            .login_command(
                                &id,
                                Cmd::QuestionAnswer {
                                    question_id: q,
                                    answer: "none".into(),
                                },
                            )
                            .await
                    }
                    None => client.login_command(&id, Cmd::QuestionRead).await,
                }
            });
            w.http.wait(before + 1).await;
            if abort {
                task.abort();
                assert!(task.await.err().expect("task aborted").is_cancelled());
            } else {
                tokio::time::advance(Duration::from_secs(46)).await;
                assert!(task.await.unwrap().is_err());
            }
            response.closed().await;
            assert!(!w.client.login_snapshot(&w.id).unwrap().busy);
            assert!(
                w.phase()
                    == if read {
                        Phase::Failed
                    } else {
                        Phase::RecoveryRequired
                    }
            );
        }
    }
}
