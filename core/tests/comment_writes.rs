#[path = "support/comment_writes.rs"]
mod support;
use cumulus_core::{Error, account::AccountSession, comments::*, storage::StateStore};
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use support::*;

#[tokio::test]
async fn official_write_helpers_and_utf16_widget_limits_match_direct_js_execution() {
    let oracle: Value =
        serde_json::from_str(include_str!("fixtures/comment-writes-official.json")).unwrap();
    for row in oracle["texts"].as_array().unwrap() {
        let text = row["content"].as_str().unwrap();
        assert_eq!(
            text.encode_utf16().count(),
            row["utf16"].as_u64().unwrap() as usize
        );
        assert_eq!(
            validate_comment_text(text).is_ok(),
            row["valid"].as_bool().unwrap()
        );
    }
    let f = Fixture::new();
    let api = CommentApi::new(f.api.clone());
    api.write(&resource(), &publish()).await.unwrap();
    api.write(
        &CommentResource {
            kind: ResourceKind::Playlist,
            id: "456".into(),
            owner: None,
        },
        &CommentWriteCommand::Publish {
            content: "回复内容".into(),
            reply_to: Some("900719925474099312345".into()),
        },
    )
    .await
    .unwrap();
    api.write(
        &CommentResource {
            kind: ResourceKind::Mv,
            id: "789".into(),
            owner: None,
        },
        &CommentWriteCommand::Delete {
            comment_id: "900719925474099312345".into(),
        },
    )
    .await
    .unwrap();
    let n = {
        let calls = f.api.calls.lock().unwrap();
        for (actual, expected) in calls.iter().zip(oracle["calls"].as_array().unwrap()) {
            assert_eq!(actual.0, expected["path"]);
            assert_eq!(actual.1, expected["payload"]);
        }
        calls.len()
    };
    assert!(
        api.write(
            &resource(),
            &CommentWriteCommand::Publish {
                content: " ".into(),
                reply_to: None
            }
        )
        .await
        .is_err()
    );
    assert_eq!(f.api.calls.lock().unwrap().len(), n);
}
#[tokio::test]
async fn publish_and_reply_are_persisted_before_sending_and_confirm_exact_large_identity() {
    for reply_to in [None, Some("102".into())] {
        let f = Fixture::new();
        let t = f.session.token().unwrap();
        let command = CommentWriteCommand::Publish {
            content: "  保留原文\n🎵 ".into(),
            reply_to: reply_to.clone(),
        };
        let prepared = f.prepare(command.clone()).await.unwrap();
        assert_eq!(f.writes(), 0);
        assert_eq!(prepared.phase, CommentWritePhase::Prepared);
        let row = f.service.submit_write_for(&t, &prepared.id).await.unwrap();
        assert_eq!(row.phase, CommentWritePhase::Confirmed);
        assert!(row.acknowledged);
        assert_eq!(row.command, command);
        assert_eq!(row.receipt_id.as_deref(), Some("900719925474099312345"));
        assert_eq!(row.observed.as_ref().unwrap().content, "  保留原文\n🎵 ");
        f.service.submit_write_for(&t, &row.id).await.unwrap();
        f.service.reconcile_write_for(&t, &row.id).await.unwrap();
        assert_eq!(f.writes(), 1);
    }
}
#[tokio::test]
async fn only_owned_live_targets_can_be_deleted_and_identity_is_rechecked_before_send() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    assert!(
        f.prepare(CommentWriteCommand::Delete {
            comment_id: "102".into()
        })
        .await
        .is_err()
    );
    assert!(
        f.prepare(CommentWriteCommand::Delete {
            comment_id: "999".into()
        })
        .await
        .is_err()
    );
    let row = f
        .prepare(CommentWriteCommand::Delete {
            comment_id: "101".into(),
        })
        .await
        .unwrap();
    f.api.rows.lock().unwrap().get_mut("101").unwrap()["user"]["userId"] = json!("2");
    assert!(f.service.submit_write_for(&t, &row.id).await.is_err());
    assert_eq!(f.writes(), 0);
    f.api.rows.lock().unwrap().get_mut("101").unwrap()["user"]["userId"] = json!("1");
    assert_eq!(
        f.service.submit_write_for(&t, &row.id).await.unwrap().phase,
        CommentWritePhase::Confirmed
    );
    assert!(!f.api.rows.lock().unwrap().contains_key("101"));
    assert_eq!(f.writes(), 1);
}
#[tokio::test]
async fn acknowledged_deletion_accepts_a_tombstone_but_unacknowledged_absence_stays_unknown() {
    for mode in [Mode::Tombstone, Mode::LostApplied] {
        let f = Fixture::new();
        let t = f.session.token().unwrap();
        *f.api.mode.lock().unwrap() = mode;
        let row = f
            .prepare(CommentWriteCommand::Delete {
                comment_id: "101".into(),
            })
            .await
            .unwrap();
        let outcome = f.service.submit_write_for(&t, &row.id).await;
        if matches!(mode, Mode::Tombstone) {
            assert_eq!(outcome.unwrap().phase, CommentWritePhase::Confirmed);
        } else {
            assert!(outcome.is_err());
            assert!(matches!(
                f.service.reconcile_write_for(&t, &row.id).await,
                Err(Error::MutationUncertain)
            ));
            assert!(
                f.service
                    .write_operation_for(&t, &row.id)
                    .unwrap()
                    .observed_absent
            );
        }
        assert_eq!(f.writes(), 1);
    }
}
#[tokio::test]
async fn lost_or_malformed_publish_receipt_never_guesses_by_content_or_replays() {
    for mode in [
        Mode::LostBefore,
        Mode::LostApplied,
        Mode::MissingId,
        Mode::WrongThread,
    ] {
        let f = Fixture::new();
        let t = f.session.token().unwrap();
        *f.api.mode.lock().unwrap() = mode;
        let row = f.prepare(publish()).await.unwrap();
        assert!(f.service.submit_write_for(&t, &row.id).await.is_err());
        assert!(f.service.submit_write_for(&t, &row.id).await.is_err());
        assert!(f.service.reconcile_write_for(&t, &row.id).await.is_err());
        assert_eq!(f.writes(), 1);
        assert!(matches!(
            f.prepare(publish()).await,
            Err(Error::MutationUncertain)
        ));
        let ended = f.service.abandon_write_for(&t, &row.id).unwrap();
        assert_eq!(ended.phase, CommentWritePhase::Abandoned);
        assert_eq!(
            ended.acknowledged,
            matches!(mode, Mode::MissingId | Mode::WrongThread)
        );
        let next = f.prepare(publish()).await.unwrap();
        assert_ne!(next.id, row.id);
        assert_eq!(f.writes(), 1);
    }
}
#[tokio::test]
async fn wrong_author_is_not_confirmed_and_delayed_visibility_recovers_using_saved_receipt() {
    for mode in [Mode::WrongOwner, Mode::Hidden] {
        let f = Fixture::new();
        let t = f.session.token().unwrap();
        *f.api.mode.lock().unwrap() = mode;
        let row = f.prepare(publish()).await.unwrap();
        assert!(f.service.submit_write_for(&t, &row.id).await.is_err());
        let pending = f.service.write_operation_for(&t, &row.id).unwrap();
        assert!(pending.acknowledged);
        assert!(pending.receipt_id.is_some());
        if matches!(mode, Mode::Hidden) {
            f.api.hidden.store(false, Ordering::Release);
            let store = Arc::new(StateStore::open(f.root.path()).unwrap());
            let session = Arc::new(AccountSession::new(f.api.clone(), Some(store)));
            session
                .adopt(
                    f.api.clone(),
                    json!({"userId":"1"}),
                    &session.token().unwrap(),
                )
                .unwrap();
            let restored = Comments::new(session.clone())
                .unwrap()
                .with_settle_delay(Duration::ZERO);
            assert_eq!(
                restored
                    .reconcile_write_for(&session.token().unwrap(), &row.id)
                    .await
                    .unwrap()
                    .phase,
                CommentWritePhase::Confirmed
            );
        } else {
            assert!(f.service.reconcile_write_for(&t, &row.id).await.is_err());
        }
        assert_eq!(f.writes(), 1);
    }
}
#[tokio::test]
async fn recovered_rejection_codes_remain_rejected_and_cannot_be_retried_as_same_intent() {
    for code in [250, 403, 404, 405, 480] {
        let f = Fixture::new();
        let t = f.session.token().unwrap();
        *f.api.mode.lock().unwrap() = Mode::Reject(code);
        let row = f.prepare(publish()).await.unwrap();
        assert!(
            f.service
                .submit_write_for(&t, &row.id)
                .await
                .unwrap_err()
                .is_service_code(code)
        );
        let rejected = f.service.submit_write_for(&t, &row.id).await.unwrap();
        assert_eq!(rejected.phase, CommentWritePhase::Rejected);
        assert_eq!(rejected.service_code, Some(code));
        assert!(!rejected.acknowledged);
        assert_eq!(f.writes(), 1);
        assert!(f.prepare(publish()).await.is_ok());
    }
}
#[tokio::test]
async fn concurrent_submit_is_blocked_and_cancelled_future_leaves_an_unreplayed_sending_intent() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    *f.api.mode.lock().unwrap() = Mode::Hold;
    let row = f.prepare(publish()).await.unwrap();
    let service = f.service.clone();
    let token = t.clone();
    let id = row.id.clone();
    let pending = tokio::spawn(async move { service.submit_write_for(&token, &id).await });
    f.api.started.notified().await;
    assert!(matches!(
        f.service.submit_write_for(&t, &row.id).await,
        Err(Error::MutationBusy)
    ));
    assert!(matches!(
        f.service.abandon_write_for(&t, &row.id),
        Err(Error::MutationBusy)
    ));
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert_eq!(
        f.service.write_operation_for(&t, &row.id).unwrap().phase,
        CommentWritePhase::Sending
    );
    assert!(matches!(
        f.service.submit_write_for(&t, &row.id).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(f.writes(), 1);
}
#[tokio::test]
async fn late_acknowledgement_is_retained_under_old_owner_and_never_published_to_new_account() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    *f.api.mode.lock().unwrap() = Mode::Hold;
    let row = f.prepare(publish()).await.unwrap();
    let service = f.service.clone();
    let token = t.clone();
    let id = row.id.clone();
    let pending = tokio::spawn(async move { service.submit_write_for(&token, &id).await });
    f.api.started.notified().await;
    f.session
        .adopt(f.api.clone(), json!({"userId":"2"}), &t)
        .unwrap();
    f.api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    let record = f
        .store
        .get("1", "comment-writes-v1", &row.id)
        .unwrap()
        .unwrap();
    assert_eq!(record["acknowledged"], true);
    assert_eq!(record["phase"], "verifying");
    let current = f.session.token().unwrap();
    assert!(
        f.service
            .write_operations_for(&current, &resource())
            .unwrap()
            .is_empty()
    );
    assert!(f.service.write_operation_for(&current, &row.id).is_err());
    assert_eq!(f.writes(), 1);
}
#[tokio::test]
async fn prepared_restart_requires_explicit_submit_and_abandon_has_no_remote_effect() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    let row = f.prepare(publish()).await.unwrap();
    assert_eq!(
        f.service
            .reconcile_write_for(&t, &row.id)
            .await
            .unwrap()
            .phase,
        CommentWritePhase::Prepared
    );
    let new = Comments::new(f.session.clone()).unwrap();
    assert_eq!(
        new.write_operations_for(&t, &resource()).unwrap()[0].id,
        row.id
    );
    new.abandon_write_for(&t, &row.id).unwrap();
    new.submit_write_for(&t, &row.id).await.unwrap();
    assert_eq!(f.writes(), 0);
}
#[tokio::test]
async fn disk_failure_before_send_prevents_write_and_confirmation_failure_retains_receipt_for_recovery()
 {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    let row = f.prepare(publish()).await.unwrap();
    f.trigger("CREATE TRIGGER stop_sending BEFORE INSERT ON state WHEN NEW.kind='comment-writes-v1' AND json_extract(NEW.value,'$.phase')='sending' BEGIN SELECT RAISE(ABORT,'fixture'); END;");
    assert!(f.service.submit_write_for(&t, &row.id).await.is_err());
    assert_eq!(f.writes(), 0);
    f.trigger("DROP TRIGGER stop_sending; CREATE TRIGGER stop_confirm BEFORE INSERT ON state WHEN NEW.kind='comment-writes-v1' AND json_extract(NEW.value,'$.phase')='confirmed' BEGIN SELECT RAISE(ABORT,'fixture'); END;");
    assert!(f.service.submit_write_for(&t, &row.id).await.is_err());
    let pending = f.service.write_operation_for(&t, &row.id).unwrap();
    assert!(pending.phase.pending() && pending.acknowledged && pending.receipt_id.is_some());
    f.trigger("DROP TRIGGER stop_confirm");
    assert_eq!(
        f.service
            .reconcile_write_for(&t, &row.id)
            .await
            .unwrap()
            .phase,
        CommentWritePhase::Confirmed
    );
    assert_eq!(f.writes(), 1);
}
#[tokio::test]
async fn old_pages_cannot_overwrite_write_results_and_other_resource_cache_is_preserved() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    let q = CommentQuery::default();
    let r = resource();
    let other = CommentResource {
        id: "999".into(),
        ..r.clone()
    };
    f.service.page_for(&t, &r, &q).await.unwrap();
    f.service.page_for(&t, &other, &q).await.unwrap();
    f.api.hold_read.store(true, Ordering::Release);
    let service = f.service.clone();
    let token = t.clone();
    let resource = r.clone();
    let query = q.clone();
    let stale = tokio::spawn(async move { service.page_for(&token, &resource, &query).await });
    f.api.started.notified().await;
    let record = f.prepare(publish()).await.unwrap();
    f.service.submit_write_for(&t, &record.id).await.unwrap();
    f.api.release.notify_one();
    assert!(matches!(stale.await.unwrap(), Err(Error::QueueChanged)));
    assert!(f.service.cached_for(&t, &r, &q).unwrap().is_none());
    assert!(f.service.cached_for(&t, &other, &q).unwrap().is_some());
}

#[tokio::test]
async fn native_client_projects_intents_receipts_and_errors_without_author_ids_or_raw_fields() {
    use cumulus_core::{
        account::profiles::ProfileRegistry,
        client::{ClientError, MusicClient, dto::CommentWriteAction},
        protocol::http::ApiOrigin,
    };
    let f = Fixture::new();
    let registry = ProfileRegistry::open(f.root.path(), ApiOrigin::Pc).unwrap();
    let client = MusicClient::with_session(f.session.clone(), registry);
    let epoch = f.session.token().unwrap().generation.to_string();
    f.api.rows.lock().unwrap().get_mut("102").unwrap()["user"]["avatarUrl"] =
        json!("http://p1.music.126.net/image.jpg?secret=private");
    f.api.rows.lock().unwrap().get_mut("102").unwrap()["richContent"] =
        json!({"secret":"native only"});
    assert!(matches!(
        client
            .comments_prepare_write("0", resource(), publish())
            .await,
        Err(Error::StaleResponse)
    ));
    let prepared = client
        .comments_prepare_write(
            &epoch,
            resource(),
            CommentWriteCommand::Publish {
                content: "reply".into(),
                reply_to: Some("102".into()),
            },
        )
        .await
        .unwrap();
    let projected = serde_json::to_string(&prepared.value).unwrap();
    assert!(
        !projected.contains("userId")
            && !projected.contains("richContent")
            && !projected.contains("secret")
            && !projected.contains("http://")
    );
    assert!(projected.contains("replyTo"));
    assert_eq!(f.writes(), 0);
    let done = client
        .comments_write_command(&epoch, &prepared.value.id, CommentWriteAction::Submit)
        .await
        .unwrap();
    assert_eq!(done.value.phase, CommentWritePhase::Confirmed);
    assert_eq!(
        client
            .comments_write_operations(&epoch, resource())
            .unwrap()
            .value
            .len(),
        1
    );
    let error = ClientError::from(Error::service(
        "/api/resource/comments/add",
        json!(480),
        json!({"secret":"not public"}),
    ));
    assert_eq!(error.code, "comment_phone");
    assert!(!serde_json::to_string(&error).unwrap().contains("secret"));
}
