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
}

impl Server {
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
