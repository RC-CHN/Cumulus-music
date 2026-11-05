use super::*;
use std::time::Duration;

#[derive(Default)]
pub(super) struct Server {
    calls: Mutex<Vec<(String, Value)>>,
    receipt: Mutex<Option<Value>>,
    lose_receipt: AtomicU64,
    hold: AtomicU64,
    held: Notify,
    released: Notify,
    dispatch_hold: AtomicU64,
    dispatch_held: Notify,
    dispatch_release: Notify,
    post_hold: AtomicU64,
    post_held: Notify,
    post_release: Notify,
    like_reads: AtomicU64,
    pub(super) scoped_like_reads: AtomicU64,
    likes_enabled: AtomicU64,
    liked: AtomicU64,
    reports_enabled: AtomicU64,
}

impl Server {
    pub(super) async fn wait_dispatch(&self) {
        if self.dispatch_hold.swap(0, Ordering::SeqCst) == 1 {
            self.dispatch_held.notify_one();
            self.dispatch_release.notified().await;
        }
    }

    pub(super) async fn response(
        &self,
        path: &str,
        payload: &Value,
    ) -> Option<cumulus_core::Result<Value>> {
        match path {
            "/api/v1/resource/comments/R_SO_4_1" if payload["limit"] == 1 => {
                let selected = payload["commentId"].as_str()?;
                if selected == "21" && self.hold.swap(0, Ordering::SeqCst) == 1 {
                    self.held.notify_one();
                    self.released.notified().await;
                }
                let likes = self.likes_enabled.load(Ordering::SeqCst) == 1;
                if likes {
                    self.like_reads.fetch_add(1, Ordering::SeqCst);
                }
                let row = if selected == "21" {
                    let mut row = row("21", "20", "合成原文");
                    if likes {
                        let liked = self.liked.load(Ordering::SeqCst) == 1;
                        row["liked"] = json!(liked);
                        row["likedCount"] = json!(3 + u64::from(liked));
                    }
                    row
                } else if likes && selected == "22" {
                    let mut row = row("22", "20", "未知点赞状态");
                    row["liked"] = Value::Null;
                    row
                } else if likes && selected == "23" {
                    let mut row = row("23", "20", "其他线程合成评论");
                    row["threadId"] = json!("R_SO_4_2");
                    row
                } else if self.reports_enabled.load(Ordering::SeqCst) == 1 {
                    match selected {
                        "22" => row("22", "9", "合成自己的评论"),
                        "23" => {
                            let mut removed = row("23", "20", "合成已移除评论");
                            removed["status"] = json!(-10);
                            removed
                        }
                        "25" | "26" => row(selected, "20", "合成举报目标"),
                        "27" => {
                            let mut other = row("27", "20", "合成其他线程评论");
                            other["threadId"] = json!("R_SO_4_2");
                            other
                        }
                        "28" => {
                            let mut unknown = row("28", "20", "合成不可用作者");
                            unknown["user"]["accountStatus"] = json!(30);
                            unknown
                        }
                        _ => Value::Null,
                    }
                } else {
                    self.receipt
                        .lock()
                        .unwrap()
                        .clone()
                        .filter(|row| row["commentId"] == selected)
                        .unwrap_or(Value::Null)
                };
                Some(Ok(
                    json!({"code":200,"comments":[],"total":0,"more":false,"currentComment":row}),
                ))
            }
            "/api/report/reportcomment" if self.reports_enabled.load(Ordering::SeqCst) == 1 => {
                assert_eq!(payload.as_object().unwrap().len(), 3);
                assert_eq!(payload["threadId"], "R_SO_4_1");
                self.calls
                    .lock()
                    .unwrap()
                    .push((path.into(), payload.clone()));
                Some(match payload["commentId"].as_str().unwrap() {
                    "21" => Ok(json!({"code":200})),
                    "25" => Ok(json!({"code":"-1"})),
                    "26" => Err(Error::Transport("synthetic report reply lost".into())),
                    _ => panic!("unreportable comment reached transport"),
                })
            }
            "/api/v1/comment/like" | "/api/v1/comment/unlike"
                if self.likes_enabled.load(Ordering::SeqCst) == 1 =>
            {
                assert_eq!(*payload, json!({"threadId":"R_SO_4_1","commentId":"21"}));
                self.calls
                    .lock()
                    .unwrap()
                    .push((path.into(), payload.clone()));
                self.liked
                    .store(u64::from(path.ends_with("/like")), Ordering::SeqCst);
                Some(if self.lose_receipt.load(Ordering::SeqCst) == 1 {
                    Err(Error::Transport("synthetic lost like response".into()))
                } else {
                    Ok(json!({"code":200}))
                })
            }
            "/api/resource/comments/add" | "/api/v1/resource/comments/reply" => {
                self.calls
                    .lock()
                    .unwrap()
                    .push((path.into(), payload.clone()));
                let mut row = row("102", "9", payload["content"].as_str().unwrap());
                if path.ends_with("/reply") {
                    row["beReplied"] = json!([{"beRepliedCommentId":payload["commentId"],
                        "content":"合成原文","user":{"userId":"20","nickname":"Fixture"}}]);
                }
                *self.receipt.lock().unwrap() = Some(row.clone());
                if self.post_hold.swap(0, Ordering::SeqCst) == 1 {
                    self.post_held.notify_one();
                    self.post_release.notified().await;
                }
                Some(if self.lose_receipt.load(Ordering::SeqCst) == 1 {
                    Err(Error::Transport("synthetic lost response".into()))
                } else {
                    Ok(json!({"code":200,"comment":row}))
                })
            }
            _ => None,
        }
    }
}

fn row(id: &str, owner: &str, content: &str) -> Value {
    json!({"commentId":id,"threadId":"R_SO_4_1","content":content,"time":1700000000000_u64,
           "user":{"userId":owner,"nickname":"Fixture"},"liked":false,"likedCount":0,"beReplied":[]})
}

fn publish(reply: bool) -> CommentWriteCommand {
    CommentWriteCommand::Publish {
        content: "  合成内容 🎵 ".into(),
        reply_to: reply.then(|| "21".into()),
    }
}

const REPORT: &str = "/api/report/reportcomment";

async fn prepare_report(
    client: &MusicClient,
    play_id: &str,
    id: &str,
    reason: CommentReportReason,
) -> String {
    let result = client
        .player_vinyl_comment_report("1", play_id, id, CommentReportCommand::Prepare { reason })
        .await
        .unwrap();
    serde_json::to_value(result.value).unwrap()["receipt"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test]
async fn vinyl_report_uses_current_native_thread_and_shares_only_durable_status() {
    use cumulus_core::comments::{CommentResource, ResourceKind};
    let (_dir, api, _, client, _) = fixture();
    let server = &api.vinyl_composer;
    server.reports_enabled.store(1, Ordering::SeqCst);
    api.vinyl_comment_group.store(2, Ordering::SeqCst);
    let play_id = client
        .play_selection("1", songs("1"))
        .await
        .unwrap()
        .play_id
        .unwrap();
    assert!(
        client
            .player_vinyl_comment_report("1", &play_id, "21", CommentReportCommand::Inspect)
            .await
            .is_err()
    );
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    let inspect = || CommentReportCommand::Inspect;
    let resource = CommentResource {
        kind: ResourceKind::Track,
        id: "1".into(),
        owner: None,
    };
    let submit = |receipt| CommentReportCommand::Submit { receipt };
    let cancelled = prepare_report(&client, &play_id, "21", CommentReportReason::Political).await;
    client
        .player_vinyl_comment_report(
            "1",
            &play_id,
            "21",
            CommentReportCommand::Cancel {
                receipt: cancelled.clone(),
            },
        )
        .await
        .unwrap();
    assert!(
        client
            .player_vinyl_comment_report("1", &play_id, "21", submit(cancelled))
            .await
            .is_err()
    );
    for unavailable in ["22", "23", "24", "28"] {
        let receipt = prepare_report(
            &client,
            &play_id,
            unavailable,
            CommentReportReason::Advertising,
        )
        .await;
        let result = client
            .player_vinyl_comment_report("1", &play_id, unavailable, submit(receipt))
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(result.value).unwrap()["status"],
            "target_unavailable"
        );
    }
    let other_thread =
        prepare_report(&client, &play_id, "27", CommentReportReason::Meaningless).await;
    assert!(
        client
            .player_vinyl_comment_report("1", &play_id, "27", submit(other_thread))
            .await
            .is_err()
    );
    assert!(server.calls.lock().unwrap().is_empty());
    let raw = "  合成举报理由 🎵\n保留原文  ";
    for (id, reason, wire, outcome) in [
        (
            "21",
            CommentReportReason::Meaningless,
            "段子或者无意义的评论",
            "accepted",
        ),
        (
            "25",
            CommentReportReason::PersonalAttack,
            "恶意人身攻击",
            "duplicate",
        ),
        (
            "26",
            CommentReportReason::Other {
                description: raw.into(),
            },
            raw,
            "uncertain",
        ),
    ] {
        let receipt = prepare_report(&client, &play_id, id, reason).await;
        assert!(
            client
                .comment_report("1", resource.clone(), id, submit(receipt.clone()))
                .await
                .is_err()
        );
        for _ in 0..2 {
            let result = client
                .player_vinyl_comment_report("1", &play_id, id, submit(receipt.clone()))
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_value(result.value).unwrap(),
                json!({"status":outcome,"receipt":null})
            );
        }
        let independent = client
            .comment_report("1", resource.clone(), id, inspect())
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(independent.value).unwrap()["status"],
            outcome
        );
        assert_eq!(
            server.calls.lock().unwrap().last(),
            Some(&(
                REPORT.into(),
                json!({"threadId":"R_SO_4_1","commentId":id,"reason":wire})
            ))
        );
    }
    assert_eq!(server.calls.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn vinyl_report_proven_not_sent_does_not_poison_original_journal_after_scope_changes() {
    use cumulus_core::comments::{CommentResource, ResourceKind};
    for scenario in ["play", "credentials", "account"] {
        let (_dir, api, session, client, _) = fixture();
        let server = &api.vinyl_composer;
        server.reports_enabled.store(1, Ordering::SeqCst);
        api.vinyl_comment_group.store(1, Ordering::SeqCst);
        let first = client
            .play_selection("1", songs("1"))
            .await
            .unwrap()
            .play_id
            .unwrap();
        let receipt = prepare_report(&client, &first, "21", CommentReportReason::Meaningless).await;
        server.dispatch_hold.store(1, Ordering::SeqCst);
        let task = tokio::spawn({
            let client = client.clone();
            let first = first.clone();
            async move {
                client
                    .player_vinyl_comment_report(
                        "1",
                        &first,
                        "21",
                        CommentReportCommand::Submit { receipt },
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), server.dispatch_held.notified())
            .await
            .unwrap();
        match scenario {
            "play" => {
                client.play_selection("1", songs("2")).await.unwrap();
            }
            "credentials" => api.vinyl_comment_group.store(3, Ordering::SeqCst),
            "account" => {
                session
                    .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
                    .unwrap();
            }
            _ => unreachable!(),
        }
        server.dispatch_release.notify_one();
        assert!(task.await.unwrap().is_err(), "{scenario}");
        if scenario == "account" {
            session
                .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
                .unwrap();
        }
        let current_epoch = session.token().unwrap().generation.to_string();
        let view = client
            .comment_report(
                &current_epoch,
                CommentResource {
                    kind: ResourceKind::Track,
                    id: "1".into(),
                    owner: None,
                },
                "21",
                CommentReportCommand::Inspect,
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(view.value).unwrap()["status"],
            "ready",
            "{scenario}"
        );
        assert!(server.calls.lock().unwrap().is_empty(), "{scenario}");
    }
}

#[tokio::test]
async fn vinyl_likes_require_verified_current_play_and_share_reconciliation_with_comments_page() {
    use cumulus_core::{
        account::resources::MutationPhase, client::VinylCommentLikeAction as Action,
        comments::CommentResource,
    };
    let (_dir, api, _, client, _) = fixture();
    let server = &api.vinyl_composer;
    server.likes_enabled.store(1, Ordering::SeqCst);
    api.vinyl_comment_group.store(2, Ordering::SeqCst);
    let id = client
        .play_selection("1", songs("1"))
        .await
        .unwrap()
        .play_id
        .unwrap();
    let set = |desired| Action::Set { desired };
    assert!(
        client
            .player_vinyl_comment_like("1", &id, "21", set(true))
            .await
            .is_err()
    );
    assert_eq!(server.like_reads.load(Ordering::SeqCst), 0);
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    for invalid in ["021", "22", "23", "99"] {
        assert!(
            client
                .player_vinyl_comment_like("1", &id, invalid, set(true))
                .await
                .is_err()
        );
    }
    assert!(server.calls.lock().unwrap().is_empty());
    let result = client
        .player_vinyl_comment_like("1", &id, "21", set(true))
        .await
        .unwrap()
        .value;
    assert_eq!(result.phase, MutationPhase::Confirmed);
    let row = result.observed.unwrap();
    assert_eq!((row.liked, row.liked_count), (Some(true), 4));
    assert_eq!(server.scoped_like_reads.load(Ordering::SeqCst), 6);
    assert_eq!(server.like_reads.load(Ordering::SeqCst), 6);
    assert_eq!(
        client
            .player_vinyl_comment_like("1", &id, "21", set(true))
            .await
            .unwrap()
            .value
            .phase,
        MutationPhase::Unchanged
    );
    server.lose_receipt.store(1, Ordering::SeqCst);
    assert!(
        client
            .player_vinyl_comment_like("1", &id, "21", set(false))
            .await
            .is_err()
    );
    let pending = client
        .player_vinyl_comment_like_operations("1", &id)
        .await
        .unwrap()
        .value;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].phase, MutationPhase::Uncertain);
    assert!(matches!(
        client
            .player_vinyl_comment_like("1", &id, "21", set(false))
            .await,
        Err(Error::MutationUncertain)
    ));
    let resource: CommentResource =
        serde_json::from_value(json!({"kind":"track","id":"1"})).unwrap();
    let recovered = client
        .comments_reconcile("1", resource, "21")
        .await
        .unwrap()
        .value;
    assert_eq!(recovered.phase, MutationPhase::Confirmed);
    assert_eq!(recovered.observed.unwrap().liked, Some(false));
    assert!(
        client
            .player_vinyl_comment_like_operations("1", &id)
            .await
            .unwrap()
            .value
            .is_empty()
    );
    assert_eq!(
        server
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|call| call.0.as_str())
            .collect::<Vec<_>>(),
        ["/api/v1/comment/like", "/api/v1/comment/unlike"]
    );
}

#[tokio::test]
async fn vinyl_likes_drop_definitively_unsent_intent_after_native_play_or_revision_changes() {
    use cumulus_core::{
        account::resources::MutationPhase, client::VinylCommentLikeAction as Action,
    };
    for change_credentials in [false, true] {
        let (_dir, api, _, client, _) = fixture();
        let server = &api.vinyl_composer;
        server.likes_enabled.store(1, Ordering::SeqCst);
        api.vinyl_comment_group.store(1, Ordering::SeqCst);
        let first = client
            .play_selection("1", songs("1"))
            .await
            .unwrap()
            .play_id
            .unwrap();
        server.dispatch_hold.store(1, Ordering::SeqCst);
        let pending = tokio::spawn({
            let client = client.clone();
            let first = first.clone();
            async move {
                client
                    .player_vinyl_comment_like("1", &first, "21", Action::Set { desired: true })
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), server.dispatch_held.notified())
            .await
            .unwrap();
        if change_credentials {
            api.vinyl_comment_group.store(3, Ordering::SeqCst);
        } else {
            client.play_selection("1", songs("2")).await.unwrap();
        }
        server.dispatch_release.notify_one();
        assert!(
            matches!(pending.await.unwrap(), Err(Error::RequestNotSent(error))
            if matches!(*error, Error::CredentialChanged | Error::StaleResponse))
        );
        assert!(server.calls.lock().unwrap().is_empty());
        let current = if change_credentials {
            first
        } else {
            client
                .play_selection("1", songs("1"))
                .await
                .unwrap()
                .play_id
                .unwrap()
        };
        assert!(
            client
                .player_vinyl_comment_like_operations("1", &current)
                .await
                .unwrap()
                .value
                .is_empty()
        );
        let result = client
            .player_vinyl_comment_like("1", &current, "21", Action::Set { desired: true })
            .await
            .unwrap()
            .value;
        assert_eq!(result.phase, MutationPhase::Confirmed);
        assert_eq!(server.calls.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn vinyl_composer_binds_native_resource_and_only_submits_a_prepared_receipt_once() {
    for (reply, lose_receipt) in [(false, false), (true, false), (true, true)] {
        let (_dir, api, _, client, _) = fixture();
        api.vinyl_comment_group.store(2, Ordering::SeqCst);
        let play = client.play_selection("1", songs("1")).await.unwrap();
        let play_id = play.play_id.unwrap();
        assert!(
            client
                .player_vinyl_prepare_write("1", &play_id, publish(reply))
                .await
                .is_err()
        );
        api.vinyl_comment_group.store(1, Ordering::SeqCst);
        assert!(
            client
                .player_vinyl_prepare_write(
                    "1",
                    &play_id,
                    CommentWriteCommand::Delete {
                        comment_id: "21".into()
                    }
                )
                .await
                .is_err()
        );
        assert!(
            client
                .player_vinyl_write_operations("1", &play_id)
                .await
                .unwrap()
                .value
                .is_empty()
        );
        let prepared = client
            .player_vinyl_prepare_write("1", &play_id, publish(reply))
            .await
            .unwrap()
            .value;
        assert_eq!(prepared.resource.id, "1");
        assert_eq!(prepared.phase, CommentWritePhase::Prepared);
        assert!(api.vinyl_composer.calls.lock().unwrap().is_empty());
        if lose_receipt {
            api.vinyl_composer.lose_receipt.store(1, Ordering::SeqCst);
        }
        let result = client
            .player_vinyl_write_command("1", &play_id, &prepared.id, CommentWriteAction::Submit)
            .await;
        if lose_receipt {
            assert!(matches!(result, Err(Error::Transport(_))));
            let list = client
                .player_vinyl_write_operations("1", &play_id)
                .await
                .unwrap()
                .value;
            assert_eq!(list[0].phase, CommentWritePhase::Uncertain);
            assert!(!list[0].acknowledged);
        } else {
            let confirmed = result.unwrap().value;
            assert_eq!(confirmed.phase, CommentWritePhase::Confirmed);
            let item = confirmed.observed.unwrap();
            assert_eq!(item.id, "102");
            assert_eq!(item.content, "  合成内容 🎵 ");
        }
        assert!(
            client
                .player_vinyl_write_command("1", &play_id, &prepared.id, CommentWriteAction::Submit)
                .await
                .is_err()
        );
        let calls = api.vinyl_composer.calls.lock().unwrap();
        let expected = if reply {
            (
                "/api/v1/resource/comments/reply",
                json!({"threadId":"R_SO_4_1","commentId":"21",
                "content":"  合成内容 🎵 ","resourceType":0,"resourceId":0}),
            )
        } else {
            (
                "/api/resource/comments/add",
                json!({"threadId":"R_SO_4_1",
                "content":"  合成内容 🎵 ","resourceType":0,"resourceId":0,"expressionPicId":-1,"bubbleId":-1}),
            )
        };
        assert_eq!(&*calls, &[(expected.0.into(), expected.1)]);
    }
}

#[tokio::test]
async fn vinyl_composer_rejects_track_changes_during_reply_target_revalidation() {
    let (_dir, api, _, client, _) = fixture();
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    let first = client
        .play_selection("1", songs("1"))
        .await
        .unwrap()
        .play_id
        .unwrap();
    let prepared = client
        .player_vinyl_prepare_write("1", &first, publish(true))
        .await
        .unwrap()
        .value;
    api.vinyl_composer.hold.store(1, Ordering::SeqCst);
    let submit = tokio::spawn({
        let client = client.clone();
        let first = first.clone();
        let id = prepared.id.clone();
        async move {
            client
                .player_vinyl_write_command("1", &first, &id, CommentWriteAction::Submit)
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(2), api.vinyl_composer.held.notified())
        .await
        .unwrap();
    let second = client
        .play_selection("1", songs("2"))
        .await
        .unwrap()
        .play_id
        .unwrap();
    api.vinyl_composer.released.notify_one();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), submit)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::StaleResponse)
    ));
    assert!(api.vinyl_composer.calls.lock().unwrap().is_empty());
    assert!(
        client
            .player_vinyl_write_operations("1", &second)
            .await
            .unwrap()
            .value
            .is_empty()
    );
    let old = client
        .comments_write_operations("1", prepared.resource)
        .unwrap()
        .value;
    assert_eq!(old[0].phase, CommentWritePhase::Prepared);
}

#[tokio::test]
async fn vinyl_composer_rejects_a_held_dispatch_and_keeps_the_original_draft_prepared() {
    for change_credentials in [false, true] {
        let (_dir, api, _, client, _) = fixture();
        api.vinyl_comment_group.store(1, Ordering::SeqCst);
        let first = client
            .play_selection("1", songs("1"))
            .await
            .unwrap()
            .play_id
            .unwrap();
        let draft = client
            .player_vinyl_prepare_write("1", &first, publish(false))
            .await
            .unwrap()
            .value;
        api.vinyl_composer.dispatch_hold.store(1, Ordering::SeqCst);
        let pending = tokio::spawn({
            let client = client.clone();
            let first = first.clone();
            let id = draft.id.clone();
            async move {
                client
                    .player_vinyl_write_command("1", &first, &id, CommentWriteAction::Submit)
                    .await
            }
        });
        tokio::time::timeout(
            Duration::from_secs(2),
            api.vinyl_composer.dispatch_held.notified(),
        )
        .await
        .unwrap();
        assert_eq!(
            client
                .comments_write_operations("1", draft.resource.clone())
                .unwrap()
                .value[0]
                .phase,
            CommentWritePhase::Sending
        );
        if change_credentials {
            api.vinyl_comment_group.store(3, Ordering::SeqCst);
        } else {
            client.play_selection("1", songs("2")).await.unwrap();
        }
        api.vinyl_composer.dispatch_release.notify_one();
        let result = pending.await.unwrap();
        assert!(matches!(result, Err(Error::RequestNotSent(error))
            if matches!(*error, Error::CredentialChanged | Error::StaleResponse)));
        let old = client
            .comments_write_operations("1", draft.resource)
            .unwrap()
            .value;
        assert_eq!(old[0].phase, CommentWritePhase::Prepared);
        assert!(!old[0].acknowledged && old[0].receipt_id.is_none());
        assert!(api.vinyl_composer.calls.lock().unwrap().is_empty());
        assert!(
            client
                .player_vinyl_write_command("1", &first, &draft.id, CommentWriteAction::Submit)
                .await
                .is_err()
        );

        client
            .comments_write_command("1", &draft.id, CommentWriteAction::Abandon)
            .await
            .unwrap();
        let fresh_play = if change_credentials {
            first
        } else {
            client
                .play_selection("1", songs("1"))
                .await
                .unwrap()
                .play_id
                .unwrap()
        };
        let fresh = client
            .player_vinyl_prepare_write("1", &fresh_play, publish(false))
            .await
            .unwrap()
            .value;
        assert_ne!(fresh.id, draft.id);
        let settled = client
            .player_vinyl_write_command("1", &fresh_play, &fresh.id, CommentWriteAction::Submit)
            .await
            .unwrap()
            .value;
        assert_eq!(settled.phase, CommentWritePhase::Confirmed);
        assert_eq!(api.vinyl_composer.calls.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn vinyl_composer_retains_ack_after_dispatch_when_play_changes() {
    let (_dir, api, _, client, _) = fixture();
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    let first = client
        .play_selection("1", songs("1"))
        .await
        .unwrap()
        .play_id
        .unwrap();
    let draft = client
        .player_vinyl_prepare_write("1", &first, publish(false))
        .await
        .unwrap()
        .value;
    api.vinyl_composer.post_hold.store(1, Ordering::SeqCst);
    let pending = tokio::spawn({
        let client = client.clone();
        let first = first.clone();
        let id = draft.id.clone();
        async move {
            client
                .player_vinyl_write_command("1", &first, &id, CommentWriteAction::Submit)
                .await
        }
    });
    tokio::time::timeout(
        Duration::from_secs(2),
        api.vinyl_composer.post_held.notified(),
    )
    .await
    .unwrap();
    assert_eq!(api.vinyl_composer.calls.lock().unwrap().len(), 1);
    let second = client
        .play_selection("1", songs("2"))
        .await
        .unwrap()
        .play_id
        .unwrap();
    api.vinyl_composer.post_release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    let old = client
        .comments_write_operations("1", draft.resource)
        .unwrap()
        .value;
    assert_eq!(old[0].phase, CommentWritePhase::Confirmed);
    assert!(old[0].acknowledged && old[0].receipt_id.as_deref() == Some("102"));
    assert_eq!(
        client
            .player_snapshot("1")
            .await
            .unwrap()
            .play_id
            .as_deref(),
        Some(second.as_str())
    );
    assert_eq!(api.vinyl_composer.calls.lock().unwrap().len(), 1);
}
