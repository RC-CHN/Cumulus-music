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
