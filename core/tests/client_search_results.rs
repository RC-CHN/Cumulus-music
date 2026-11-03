use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{SearchCategory as C, SearchResultQuery},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;
struct Backend {
    reply: Mutex<Value>,
    calls: Mutex<Vec<(String, Value)>>,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), args));
            if self.hold.load(Ordering::SeqCst) {
                self.started.notify_one();
                self.release.notified().await;
            }
            Ok(self.reply.lock().unwrap().clone())
        })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    MusicClient,
) {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Backend {
        reply: Mutex::new(json!({"code":200})),
        calls: Mutex::default(),
        hold: AtomicBool::new(false),
        started: Notify::new(),
        release: Notify::new(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry);
    (root, api, session, client)
}
fn query(category: C) -> SearchResultQuery {
    SearchResultQuery {
        keyword: " 合成 ".into(),
        category,
        offset: 0,
        limit: 30,
        correct: true,
        channel: None,
    }
}
#[tokio::test]
async fn categories_use_official_shapes_and_only_public_card_fields_cross_ipc() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let payloads: Value =
        serde_json::from_str(include_str!("fixtures/search-results.json")).unwrap();
    for category in C::ALL {
        let key = serde_json::to_value(category).unwrap();
        *api.reply.lock().unwrap() = payloads[key.as_str().unwrap()].clone();
        let view = client
            .search_results(&epoch, query(category))
            .await
            .unwrap()
            .value;
        assert!(!view.sections.is_empty());
        assert!(!view.sections[0].items.is_empty());
        assert!(!serde_json::to_string(&view).unwrap().contains("PRIVATE"));
        let calls = api.calls.lock().unwrap();
        let (path, args) = calls.last().unwrap();
        assert!(!path.contains("cloudsearch"));
        if matches!(category, C::Playlists | C::Artists | C::Albums | C::Users) {
            assert_eq!(args["s"], "合成");
            assert_eq!(args["queryCorrect"], true);
        } else {
            assert_eq!(args["keyword"], "合成");
            assert_eq!(args["scene"], "normal");
        }
        if category == C::Songs {
            assert!(view.more);
            assert_eq!(view.next_offset, 3);
        }
        if category == C::Lyrics {
            assert!(!serde_json::to_string(&view).unwrap().contains("<em>"));
        }
    }
}
#[tokio::test]
async fn corrections_are_valid_search_terms_and_cannot_override_an_opt_out() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"blocks":[
        {"blockCode":"search_block_correction","resources":[
            {"baseInfo":{"rewriteQuery":"合成"}},
            {"baseInfo":{"rewriteQuery":"bad\nquery"}},
            {"baseInfo":{"queryRewriteDTO":{"rewriteQuery":" 正确关键词 ","searchQcReminder":{
                "qcReminderChannel":"never expose","qcReminders":[
                    {"qcReminderPart":"更正为正确关键词，仍搜 "},
                    {"qcReminderPart":"合成","highLight":true,"url":"do not expose"},
                    {"qcReminderPart":"。","highLight":false}
                ]
            }}}}
        ]},
        {"blockCode":"search_block_correction","resources":[
            {"baseInfo":{"rewriteQuery":"后续建议"}}
        ]}
    ]}});
    let mut q = query(C::All);
    let view = client
        .search_results(&epoch, q.clone())
        .await
        .unwrap()
        .value;
    assert_eq!(view.correction.as_deref(), Some("正确关键词"));
    assert_eq!(
        serde_json::to_value(&view).unwrap()["correctionParts"],
        json!([
            {"text":"更正为正确关键词，仍搜 ","highlight":false},
            {"text":"合成","highlight":true},
            {"text":"。","highlight":false}
        ])
    );
    q.correct = false;
    let view = client.search_results(&epoch, q).await.unwrap().value;
    assert!(view.correction.is_none());
    assert!(view.correction_parts.is_none());
    assert_eq!(
        api.calls.lock().unwrap().last().unwrap().1["needCorrect"],
        false
    );

    *api.reply.lock().unwrap() = json!({"code":200,"data":{
            "resources":[],"queryRewriteInfo":{"rewriteQuery":"bad\tquery"},
            "queryRewrite":{"rewriteQuery":"合成"},
        "searchQcReminder":{"correctQuery":" 另一关键词 "}
    }});
    let mut q = query(C::Songs);
    assert_eq!(
        client
            .search_results(&epoch, q.clone())
            .await
            .unwrap()
            .value
            .correction
            .as_deref(),
        Some("另一关键词")
    );
    q.correct = false;
    assert!(
        client
            .search_results(&epoch, q)
            .await
            .unwrap()
            .value
            .correction
            .is_none()
    );
}

#[tokio::test]
async fn unsafe_or_different_reminder_text_falls_back_without_dropping_the_rewrite() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    for parts in [
        json!([{"qcReminderPart":"different","highLight":true}]),
        json!([{"qcReminderPart":"合成","highLight":true},{"qcReminderPart":"https://private.invalid"}]),
        json!([{"qcReminderPart":"合成","highLight":true},{"qcReminderPart":"bad\nline"}]),
        json!([{"qcReminderPart":"合成","highLight":"true"}]),
        Value::Array(vec![
            json!({"qcReminderPart":"合成","highLight":true});
            13
        ]),
    ] {
        *api.reply.lock().unwrap() = json!({"code":200,"data":{"blocks":[
            {"blockCode":"search_block_correction","resources":[{"baseInfo":{
                "rewriteQuery":"正确关键词","searchQcReminder":{"qcReminders":parts}
            }}]}
        ]}});
        let value = client
            .search_results(&epoch, query(C::All))
            .await
            .unwrap()
            .value;
        assert_eq!(value.correction.as_deref(), Some("正确关键词"));
        assert!(value.correction_parts.is_none());
    }
}
#[tokio::test]
async fn contradictory_empty_counts_stop_paging_and_malformed_results_are_errors() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() =
        json!({"code":200,"data":{"resources":[],"totalCount":46,"hasMore":true}});
    let result = client
        .search_results(&epoch, query(C::Mvs))
        .await
        .unwrap()
        .value;
    assert_eq!(result.total, Some(46));
    assert!(!result.more);
    assert!(result.notice.is_some());
    *api.reply.lock().unwrap() = json!({"code":200,"data":{}});
    assert!(
        client
            .search_results(&epoch, query(C::Songs))
            .await
            .is_err()
    );
    let before = api.calls.lock().unwrap().len();
    let mut invalid = query(C::All);
    invalid.offset = 30;
    assert!(client.search_results(&epoch, invalid).await.is_err());
    assert_eq!(api.calls.lock().unwrap().len(), before);
}
#[tokio::test]
async fn unusable_cards_do_not_drop_valid_siblings_or_shift_category_offsets() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let song = |id| json!({"id":id,"name":"合成歌曲","ar":[],"al":{"name":"合成专辑"}});
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"resources":[
        {"baseInfo":{"simpleSongData":song(11)}},
        {"baseInfo":{"simpleSongData":{"name":"PRIVATE malformed row"}}},
        {"baseInfo":{"simpleSongData":song(12)}}
    ],"totalCount":10,"hasMore":true}});
    let mut request = query(C::Songs);
    request.offset = 4;
    let page =
        serde_json::to_value(client.search_results(&epoch, request).await.unwrap().value).unwrap();
    assert_eq!(page["unavailableCount"], 1);
    assert_eq!(page["nextOffset"], 7);
    assert_eq!(page["more"], true);
    assert_eq!(page["sections"][0]["items"][0]["track"]["id"], "11");
    assert_eq!(page["sections"][0]["items"][1]["track"]["id"], "12");
    assert!(!page.to_string().contains("PRIVATE"));

    *api.reply.lock().unwrap() = json!({"code":200,"data":{"blocks":[
        {"blockCode":"search_block_best_match","resources":[
            {"type":"song","baseInfo":{"id":"invalid"}},
            {"type":"song","baseInfo":song(13)},
            {"type":"unsupported_fixture","baseInfo":{"secret":"PRIVATE"}}
        ]},
        {"blockCode":"search_block_song","resources":[{"type":"song","baseInfo":song(14)}]}
    ]}});
    let comprehensive = serde_json::to_value(
        client
            .search_results(&epoch, query(C::All))
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_eq!(comprehensive["unavailableCount"], 2);
    assert_eq!(
        comprehensive["sections"][0]["items"][0]["track"]["id"],
        "13"
    );
    assert_eq!(
        comprehensive["sections"][1]["items"][0]["track"]["id"],
        "14"
    );
    assert!(!comprehensive.to_string().contains("PRIVATE"));
}

#[tokio::test]
async fn result_from_replaced_account_is_rejected() {
    let (_root, api, session, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"resources":[],"totalCount":0}});
    api.hold.store(true, Ordering::SeqCst);
    let request = client.search_results(&epoch, query(C::Songs));
    tokio::pin!(request);
    tokio::select! {_=&mut request=>panic!("request must wait"),_=api.started.notified()=>{}}
    session
        .adopt(api.clone(), json!({"userId":3}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(request.await.is_err());
}

#[tokio::test]
async fn lyrics_keep_full_returned_text_and_report_bounded_projection() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let song = json!({"id": 1, "name": "合成歌曲", "ar": [], "al": {"name":"合成专辑"}});
    let full =
        "第一行\r\n夜色里的合成旋律🎵\n\n第四行\n第五行\n第六行\n<img src=x onerror=alert(1)>";
    for field in ["lyrics", "lyric"] {
        *api.reply.lock().unwrap() = json!({"code":200,"data":{"resources":[{"song":song,field:full,"matchedLyric":["旧兼容片段"]}],"totalCount":1}});
        let view = serde_json::to_value(
            client
                .search_results(&epoch, query(C::Lyrics))
                .await
                .unwrap()
                .value,
        )
        .unwrap();
        let item = &view["sections"][0]["items"][0];
        assert_eq!(
            item["excerpt"],
            json!([
                "第一行",
                "夜色里的合成旋律🎵",
                "",
                "第四行",
                "第五行",
                "第六行",
                "<img src=x onerror=alert(1)>"
            ])
        );
        assert_eq!(item["excerptTruncated"], false);
    }
    for long in [
        "合成🎵".repeat(20000),
        format!("{}\n\n", "a".repeat(65536)),
        (0..600).map(|n| format!("合成行{n}\n")).collect(),
    ] {
        *api.reply.lock().unwrap() =
            json!({"code":200,"data":{"resources":[{"song":song,"lyrics":long}],"totalCount":1}});
        let view = serde_json::to_value(
            client
                .search_results(&epoch, query(C::Lyrics))
                .await
                .unwrap()
                .value,
        )
        .unwrap();
        let item = &view["sections"][0]["items"][0];
        let lines: Vec<String> = serde_json::from_value(item["excerpt"].clone()).unwrap();
        assert!(lines.len() <= 512);
        assert!(lines.join("\n").len() <= 65536);
        assert_eq!(item["excerptTruncated"], true);
    }
}

#[tokio::test]
async fn channels_are_forwarded_only_to_recovered_channel_aware_endpoints() {
    use cumulus_core::catalog::search::results::SearchChannel;
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let payloads: Value =
        serde_json::from_str(include_str!("fixtures/search-results.json")).unwrap();
    for (channel, wire) in [
        (Some(SearchChannel::Hotquery), "hotquery"),
        (Some(SearchChannel::Typing), "typing"),
        (Some(SearchChannel::Defaultquery), "defaultquery"),
        (Some(SearchChannel::History), "history"),
        (Some(SearchChannel::RecQuery), "rec_query"),
        (None, "suggest"),
    ] {
        for category in C::ALL {
            let key = serde_json::to_value(category).unwrap();
            *api.reply.lock().unwrap() = payloads[key.as_str().unwrap()].clone();
            let mut q = query(category);
            q.channel = channel;
            let view = client.search_results(&epoch, q).await.unwrap().value;
            assert_eq!(view.query.channel, channel);
            let calls = api.calls.lock().unwrap();
            let (_, args) = calls.last().unwrap();
            assert_eq!(
                args["channel"],
                if matches!(category, C::All | C::Songs | C::Mvs) {
                    json!(wire)
                } else {
                    Value::Null
                }
            );
        }
    }
    assert!(serde_json::from_value::<SearchResultQuery>(json!({
        "keyword":"合成", "category":"all", "offset":0, "limit":30, "correct":true, "channel":"foreign"
    })).is_err());
}

#[tokio::test]
async fn only_comprehensive_service_405_becomes_a_bounded_empty_explanation() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = json!({
        "code":405, "msg":"该词暂无可展示的结果", "message":"备用文案",
        "private":{"credential":"not for frontend"}
    });
    let view = client
        .search_results(&epoch, query(C::All))
        .await
        .unwrap()
        .value;
    assert_eq!(view.empty_title.as_deref(), Some("该词暂无可展示的结果"));
    assert!(view.sections.is_empty() && !view.more && view.next_offset == 0);
    assert!(!serde_json::to_string(&view).unwrap().contains("credential"));
    assert!(
        client
            .search_results(&epoch, query(C::Songs))
            .await
            .is_err()
    );
    *api.reply.lock().unwrap() =
        json!({"code":405,"msg":"https://example.invalid/private","message":"字".repeat(200)});
    let limited = client
        .search_results(&epoch, query(C::All))
        .await
        .unwrap()
        .value;
    assert_eq!(limited.empty_title.unwrap(), "字".repeat(160));
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"blocks":[]}});
    assert!(
        client
            .search_results(&epoch, query(C::All))
            .await
            .unwrap()
            .value
            .empty_title
            .is_none()
    );
}
