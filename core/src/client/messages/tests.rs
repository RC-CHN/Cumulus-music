use super::*;
use serde_json::json;

fn query(tab: MessageTab, cursor: Option<u64>, peer: Option<&str>) -> MessageQuery {
    MessageQuery {
        tab,
        cursor,
        peer_id: peer.map(str::to_owned),
    }
}

#[test]
fn read_routes_keep_official_cursors_limits_and_authenticated_uid() {
    let cases = [
        (
            query(MessageTab::Private, None, None),
            "/api/msg/private/users",
            json!({"time":-1,"offset":0,"limit":15}),
        ),
        (
            query(MessageTab::Comments, Some(2400), None),
            "/api/v1/user/comments/7654",
            json!({"beforeTime":2400,"limit":10}),
        ),
        (
            query(MessageTab::Forwards, None, None),
            "/api/forwards/get",
            json!({"time":-1,"offset":0,"limit":10,"total":true}),
        ),
        (
            query(MessageTab::Notices, Some(2400), None),
            "/api/msg/notices",
            json!({"time":2400,"offset":0,"limit":10,"total":true}),
        ),
        (
            query(MessageTab::Thread, Some(2300), Some("456")),
            "/api/msg/private/history",
            json!({"time":2300,"offset":0,"limit":10,"userId":"456"}),
        ),
    ];
    for (query, expected_path, expected_params) in cases {
        let (path, params, _) = endpoint(&query, "7654").unwrap();
        assert_eq!(path, expected_path);
        assert_eq!(params, expected_params);
    }
    for query in [
        query(MessageTab::Thread, None, None),
        query(MessageTab::Thread, None, Some("0")),
        query(MessageTab::Thread, None, Some("1/x")),
        query(MessageTab::Private, None, Some("123")),
        query(MessageTab::Notices, Some(0), None),
        query(MessageTab::Comments, Some(u64::MAX), None),
    ] {
        assert!(endpoint(&query, "7654").is_err());
    }
}

#[test]
fn projection_distinguishes_quote_content_own_thread_and_opaque_attachment() {
    let page = project(
        &query(MessageTab::Private, None, None),
        &json!({"more":true,"msgs":[{"fromUser":{"userId":11,"nickname":"Reader"},
            "lastMsgTime":2_000,"lastMsg":"{\"type\":6,\"msg\":\"hello\"}","newMsgCount":3}]}),
        "7654",
        15,
    )
    .unwrap();
    assert_eq!(page.rows[0].peer_id.as_deref(), Some("11"));
    assert_eq!(page.rows[0].body, "hello");
    assert_eq!(page.rows[0].unread, Some(3));
    assert_eq!(page.next_cursor, Some(2000));
    let page = project(
        &query(MessageTab::Thread, None, Some("11")),
        &json!({"more":false,"hint":"Hint","msgs":[{"id":99,"fromUser":{"userId":"7654"},
            "msg":{"type":12,"promotionUrl":{"text":"Caption","url":"https://private.invalid"}},"time":200}]}),
        "7654",
        10,
    )
    .unwrap();
    assert!(page.rows[0].own);
    assert!(page.rows[0].peer_id.is_none());
    assert!(
        !serde_json::to_string(&page)
            .unwrap()
            .contains("private.invalid")
    );
    assert_eq!(page.hint.as_deref(), Some("Hint"));
    let page = project(
        &query(MessageTab::Comments, None, None),
        &json!({"more":false,"comments":[{"id":18,"user":{"nickname":"Other"},"time":999,
            "content":"reply","beRepliedContent":"original","beRepliedUser":{"nickname":"Host"}}]}),
        "7654",
        10,
    )
    .unwrap();
    assert_eq!(page.rows[0].caption.as_deref(), Some("回复 Host"));
    assert_eq!(page.rows[0].quote.as_deref(), Some("original"));
}

#[test]
fn continuation_is_server_owned_for_notice_and_forward_and_rejects_repetition() {
    let result = json!({"more":true,"notices":[{"time":600,"notice":{"type":2,"playlist":{"name":"P"}}}],"lastTime":500});
    let page = project(&query(MessageTab::Notices, None, None), &result, "7654", 10).unwrap();
    assert_eq!(page.next_cursor, Some(500));
    assert_eq!(page.rows[0].body, "P");
    assert!(
        project(
            &query(MessageTab::Notices, Some(500), None),
            &result,
            "7654",
            10
        )
        .is_err()
    );
    let page = project(
        &query(MessageTab::Forwards, None, None),
        &json!({"more":true,"lasttime":400,"forwards":[{"type":3,"json":{"comment":
          {"user":{"nickname":"Author"},"time":800,"content":"Mention"},"resource":{"name":"Title"}}}]}),
        "7654",
        10,
    ).unwrap();
    assert_eq!(page.next_cursor, Some(400));
    assert_eq!(page.rows[0].body, "Mention");
    assert_eq!(page.rows[0].quote.as_deref(), Some("Title"));
    assert!(
        project(
            &query(MessageTab::Thread, None, Some("11")),
            &json!({"more":true,"msgs":[]}),
            "7654",
            10
        )
        .is_err()
    );
}

#[tokio::test]
async fn detaching_owner_rejects_an_already_dispatched_private_thread() {
    use crate::{
        account::{AccountSession, profiles::ProfileRegistry},
        protocol::{
            api::{Api, ApiFuture},
            http::ApiOrigin,
        },
    };
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[derive(Default)]
    struct Backend {
        started: Notify,
        release: Notify,
    }
    impl Api for Backend {
        fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
            Box::pin(async move {
                match path {
                    "/api/w/nuser/account/get" => {
                        Ok(json!({"code":200,"profile":{"userId":2,"nickname":"Synthetic"}}))
                    }
                    "/api/msg/private/history" => {
                        assert_eq!(
                            args,
                            json!({"userId":"501","time":-1,"limit":10,"offset":0})
                        );
                        self.started.notify_one();
                        self.release.notified().await;
                        Ok(json!({"code":200,"more":false,"msgs":[]}))
                    }
                    _ => panic!("unapproved message test route: {path}"),
                }
            })
        }
    }
    let api = Arc::new(Backend::default());
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), None));
    session.refresh().await.unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    let read = tokio::spawn(async move {
        client
            .message_page(&epoch, query(MessageTab::Thread, None, Some("501")))
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), api.started.notified())
        .await
        .unwrap();
    session.detach().unwrap();
    api.release.notify_one();
    assert!(matches!(read.await.unwrap(), Err(Error::StaleResponse)));
}

#[test]
fn attachment_targets_require_exact_compatible_resource_and_comment_identity() {
    use super::projection::attachment;
    use crate::comments::ResourceKind;

    assert!(
        matches!(attachment::thread(&json!({"type":1,"song":{"id":3,"url":"https://private.invalid"}})),
        Some(MessageTarget::PlayTrack { id }) if id == "3")
    );
    assert!(
        matches!(attachment::thread(&json!({"type":5,"program":{"id":401,"mainSong":{"id":3}}})),
        Some(MessageTarget::PlayVoice { id }) if id == "401")
    );
    assert!(
        matches!(attachment::thread(&json!({"type":7,"mv":{"id":33}})),
        Some(MessageTarget::Mv { id }) if id == "33")
    );
    for message in [
        json!({"type":14,"song":{"id":3}}),
        json!({"type":7,"mv":{"id":"https://private.invalid/33"}}),
        json!({"type":12,"promotionUrl":{"id":3,"url":"https://private.invalid"}}),
        json!({"type":2,"album":{"id":"3/redirect"}}),
    ] {
        assert!(attachment::thread(&message).is_none());
    }
    let good = json!({"resourceType":4,"combindId":"2:411:R_SO_4_3","resourceJson":"{\"id\":3}"});
    assert!(matches!(attachment::received_comment(&good),
        Some(MessageTarget::Comment { resource, comment_id })
          if resource.kind == ResourceKind::Track && resource.id == "3" && comment_id == "411"));
    for invalid in [
        json!({"resourceType":3,"combindId":"2:411:R_SO_4_3","resourceJson":"{\"id\":3}"}),
        json!({"resourceType":4,"combindId":"2:411:R_SO_4_3","resourceJson":"{\"id\":4}"}),
        json!({"resourceType":4,"combindId":"2:411:R_SO_4_3:extra","resourceJson":"{\"id\":3}"}),
        json!({"resourceType":2,"combindId":"2:411:A_EV_2_3_9","resourceJson":"{\"id\":3}"}),
    ] {
        assert!(attachment::received_comment(&invalid).is_none());
    }
    assert!(
        attachment::notice(&json!({"type":6,"comment":{"threadId":"R_MV_5_3","commentId":411}}))
            .is_none()
    );
    assert!(
        matches!(attachment::forwarded(1,&json!({"type":19,"json":{"album":{"id":7}}})),
        Some(MessageTarget::Album { id }) if id == "7")
    );
    assert!(attachment::forwarded(1, &json!({"type":21,"json":{"album":{"id":7}}})).is_none());
    assert!(attachment::forwarded(2, &json!({"resourceType":0,"resource":{"id":7}})).is_none());
}

#[tokio::test]
async fn private_deletion_is_observation_bound_persisted_before_write_and_readback_only_after_loss()
{
    use crate::{
        account::{AccountSession, profiles::ProfileRegistry},
        protocol::{
            api::{Api, ApiFuture},
            http::ApiOrigin,
        },
        storage::StateStore,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    };

    use tokio::sync::Notify;

    struct Backend {
        store: Arc<StateStore>,
        uid: AtomicU64,
        history_reads: AtomicU64,
        hold_private: AtomicBool,
        private_started: Notify,
        release_private: Notify,
        requests: Mutex<Vec<(String, Value)>>,
    }

    const LONG: &str = "9007199254740997";
    const JOURNAL: &str = "private-message-deletion-v1";

    impl Api for Backend {
        fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
            Box::pin(async move {
                assert!(!cache);
                self.requests
                    .lock()
                    .unwrap()
                    .push((path.into(), args.clone()));
                match path {
                    "/api/w/nuser/account/get" => Ok(json!({
                        "code":200,"profile":{"userId":self.uid.load(Ordering::SeqCst),"nickname":"Synthetic"}
                    })),
                    "/api/msg/private/users" => {
                        assert_eq!(args, json!({"time":-1,"offset":0,"limit":15}));
                        if self.hold_private.swap(false, Ordering::SeqCst) {
                            self.private_started.notify_one();
                            self.release_private.notified().await;
                        }
                        Ok(json!({"code":200,"more":false,"msgs":[
                            {"fromUser":{"userId":501,"nickname":"SYNTHETIC PRIVATE NICKNAME"},
                             "lastMsg":{"type":6,"msg":"SYNTHETIC PRIVATE BODY"}},
                            {"fromUser":{"userId":"502","nickname":"Other"}}
                        ]}))
                    }
                    "/api/msg/private/history" => {
                        assert_eq!(
                            args,
                            json!({"userId":"501","time":-1,"limit":10,"offset":0})
                        );
                        let reads = self.history_reads.fetch_add(1, Ordering::SeqCst);
                        if reads < 3 {
                            Ok(json!({"code":200,"more":false,"msgs":[
                                {"id":LONG,"time":500,"fromUser":{"userId":501},"toUser":{"userId":2},
                                 "msg":{"type":6,"msg":"SYNTHETIC PRIVATE BODY"}},
                                {"id":301,"time":490,"fromUser":{"userId":2},"toUser":{"userId":501}},
                                {"id":302,"time":480,"fromUser":{"userId":999}},
                                {"commentId":303,"time":470,"fromUser":{"userId":501}},
                                {"id":304,"time":460,"fromUser":{"userId":2},"toUser":{"userId":999}}
                            ]}))
                        } else {
                            Ok(json!({"code":200,"more":false,"msgs":[]}))
                        }
                    }
                    "/api/msg/private/delete" => {
                        assert_eq!(args, json!({"userId":"502"}));
                        let stored = self.store.values("2", JOURNAL).unwrap();
                        assert_eq!(stored.len(), 2);
                        assert!(stored.contains_key("peer:502"));
                        Ok(json!({"code":200}))
                    }
                    "/api/msg/private/delete/single" => {
                        let journal = self.store.values("2", JOURNAL).unwrap();
                        if args == json!({"id":LONG}) {
                            assert_eq!(journal.len(), 1);
                            let raw = serde_json::to_string(&journal).unwrap();
                            assert!(!raw.contains("SYNTHETIC PRIVATE"));
                            assert!(journal.contains_key(&format!("item:501:{LONG}")));
                            Err(Error::Transport("synthetic reply was lost".into()))
                        } else {
                            assert_eq!(args, json!({"id":301}));
                            assert!(journal.contains_key("item:501:301"));
                            Ok(json!({"code":403}))
                        }
                    }
                    _ => panic!("unexpected private message API: {path}"),
                }
            })
        }
    }

    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let backend = Arc::new(Backend {
        store: store.clone(),
        uid: AtomicU64::new(2),
        history_reads: AtomicU64::new(0),
        hold_private: AtomicBool::new(false),
        private_started: Notify::new(),
        release_private: Notify::new(),
        requests: Mutex::default(),
    });
    let session = Arc::new(AccountSession::new(backend.clone(), Some(store.clone())));
    session.refresh().await.unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry.clone()));
    let remove = MessageDeleteAction::Remove;
    let check = MessageDeleteAction::Check;

    assert!(matches!(
        client.delete_message(&epoch, &"a".repeat(32), remove).await,
        Err(Error::StaleResponse)
    ));
    let thread = || query(MessageTab::Thread, None, Some("501"));
    let unobserved = client.message_page(&epoch, thread()).await.unwrap().value;
    assert!(
        unobserved
            .rows
            .iter()
            .all(|row| row.deletion_receipt.is_none())
    );
    let private = client
        .message_page(&epoch, query(MessageTab::Private, None, None))
        .await
        .unwrap()
        .value;
    let observed = client.message_page(&epoch, thread()).await.unwrap().value;
    let lost = observed.rows[0].deletion_receipt.as_deref().unwrap();
    let rejected = observed.rows[1].deletion_receipt.as_deref().unwrap();
    let peer = private.rows[0].deletion_receipt.as_deref().unwrap();
    let other = private.rows[1].deletion_receipt.as_deref().unwrap();
    assert!(
        observed.rows[2..]
            .iter()
            .all(|row| row.deletion_receipt.is_none())
    );

    assert_eq!(
        client
            .delete_message(&epoch, lost, remove)
            .await
            .unwrap()
            .value,
        MessageDeleteOutcome::Uncertain
    );
    assert_eq!(
        client
            .delete_message(&epoch, lost, remove)
            .await
            .unwrap()
            .value,
        MessageDeleteOutcome::Uncertain
    );
    assert!(matches!(
        client.delete_message(&epoch, peer, remove).await,
        Err(Error::State(_))
    ));
    assert!(matches!(
        client.delete_message(&epoch, rejected, remove).await,
        Err(Error::Service { .. })
    ));
    assert_eq!(
        client
            .delete_message(&epoch, lost, check)
            .await
            .unwrap()
            .value,
        MessageDeleteOutcome::StillListed
    );
    backend.hold_private.store(true, Ordering::SeqCst);
    let old_page = tokio::spawn({
        let client = client.clone();
        let epoch = epoch.clone();
        async move {
            client
                .message_page(&epoch, query(MessageTab::Private, None, None))
                .await
                .unwrap()
        }
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        backend.private_started.notified(),
    )
    .await
    .unwrap();
    assert_eq!(
        client
            .delete_message(&epoch, other, remove)
            .await
            .unwrap()
            .value,
        MessageDeleteOutcome::Removed
    );
    backend.release_private.notify_one();
    let delayed = old_page.await.unwrap().value;
    assert!(
        delayed
            .rows
            .iter()
            .all(|row| row.deletion_receipt.is_none())
    );
    assert!(matches!(
        client.delete_message(&epoch, other, remove).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        client
            .pending_message_deletions(&epoch)
            .await
            .unwrap()
            .value
            .len(),
        1
    );

    // Reload from disk with no row observations. Only the journal receipt may
    // be read back, and seeing no row in a bounded page is not confirmation.
    let restarted = MusicClient::with_session(session.clone(), registry.clone());
    assert_eq!(
        restarted
            .delete_message(&epoch, lost, check)
            .await
            .unwrap()
            .value,
        MessageDeleteOutcome::NotInPage
    );
    assert!(matches!(
        restarted.delete_message(&epoch, lost, remove).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        restarted
            .pending_message_deletions(&epoch)
            .await
            .unwrap()
            .value
            .len(),
        1
    );

    session.detach().unwrap();
    backend.uid.store(3, Ordering::SeqCst);
    session.refresh().await.unwrap();
    let next = session.token().unwrap().generation.to_string();
    assert_ne!(epoch, next);
    assert!(
        restarted
            .pending_message_deletions(&next)
            .await
            .unwrap()
            .value
            .is_empty()
    );
    assert!(matches!(
        restarted.delete_message(&next, lost, check).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.delete_message(&next, peer, remove).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(store.values("2", JOURNAL).unwrap().len(), 1);
    assert!(store.values("3", JOURNAL).unwrap().is_empty());
    let wire = backend.requests.lock().unwrap();
    let deletes = wire
        .iter()
        .filter(|(path, _)| path.contains("/delete"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        deletes,
        vec![
            ("/api/msg/private/delete/single".into(), json!({"id":LONG})),
            ("/api/msg/private/delete/single".into(), json!({"id":301})),
            ("/api/msg/private/delete".into(), json!({"userId":"502"})),
        ]
    );
}
