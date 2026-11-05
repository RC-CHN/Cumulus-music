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
                let row = if selected == "21" {
                    row("21", "20", "合成原文")
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
