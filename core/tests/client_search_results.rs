use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    catalog::search::results::SearchChannel as Channel,
    client::{
        MusicClient,
        dto::{SearchCategory as C, SearchDiscoveryQuery, SearchResultQuery},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;
struct Backend {
    reply: Mutex<Value>,
    calls: Mutex<Vec<(String, Value)>>,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
    revision: AtomicUsize,
    checked: Mutex<Vec<String>>,
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
    fn search_privilege_credential_revision(&self) -> cumulus_core::Result<Option<String>> {
        Ok(Some(self.revision.load(Ordering::SeqCst).to_string()))
    }

    fn search_default_context_revision(&self) -> cumulus_core::Result<Option<String>> {
        Ok(Some(self.revision.load(Ordering::SeqCst).to_string()))
    }

    fn request_search_with_default_context<'a>(
        &'a self,
        path: &'a str,
        args: Value,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.search_default_context_revision()?.as_deref() != Some(revision) {
                return Err(Error::CredentialChanged);
            }
            self.checked.lock().unwrap().push(revision.into());
            self.request(path, args, false).await
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
        revision: AtomicUsize::new(1),
        checked: Mutex::default(),
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

#[path = "support/search_default_context.rs"]
mod search_default_context;
#[path = "support/search_privilege_refresh.rs"]
mod search_privilege_refresh;
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
        if matches!(category, C::All | C::Voices) {
            let projected = serde_json::to_value(&view).unwrap();
            let voice = projected["sections"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|section| section["items"].as_array().unwrap())
                .find(|item| item["kind"] == "voice")
                .unwrap();
            assert_eq!(voice["listenerCount"], 12345);
        }
        if matches!(category, C::All | C::Playlists) {
            let projected = serde_json::to_value(&view).unwrap();
            let mut playlists = projected["sections"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|section| section["items"].as_array().unwrap())
                .filter(|item| item["kind"] == "playlist");
            let playlist = playlists.next().unwrap();
            assert_eq!(playlist["recommendation"], "合成 推荐理由");
            if category == C::Playlists {
                assert!(playlists.next().unwrap().get("recommendation").is_none());
            }
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
async fn podcast_search_projects_official_table_metadata_without_guessing() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"resources":[
        {"baseInfo":{"id":301,"name":"完整播客","voiceCount":24,"playCount":12345,
                     "score":99,"dj":{"userId":501,"nickname":"主播","signature":"PRIVATE_DJ"}},
         "extInfo":{"scoreDto":{"score":"9.7","private":"PRIVATE_SCORE"}},
         "uiElement":{"desc":{"title":" <em>合成</em>  推荐 "}}},
        {"baseInfo":{"id":302,"name":"不可用主播","programCount":0,"playCount":0,
                     "creator":{"userId":502,"nickname":"不可用","accountStatus":30}},
         "extInfo":{"scoreDto":{"score":"PRIVATE_NOT_SCORE"}},
         "uiElement":{"desc":{"title":"https://private.invalid/PRIVATE_URL"}}},
        {"baseInfo":{"id":303,"name":"缺失字段","userId":503,"creatorName":"未核验",
                     "playCount":9007199254740992_u64},
         "extInfo":{"scoreDto":{"score":0}}},
        {"baseInfo":{"id":"invalid","name":"坏结果"}}
    ],"totalCount":14,"hasMore":true}});
    let mut query = query(C::Podcasts);
    query.offset = 6;
    query.limit = 10;
    let result = client.search_results(&epoch, query).await.unwrap().value;
    assert!(!result.more);
    assert_eq!(result.next_offset, 10);
    assert_eq!(result.unavailable_count, 1);
    let view = serde_json::to_value(result).unwrap();
    let items = view["sections"][0]["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0]["table"],
        json!({
            "creatorId":"501","programCount":24,"playCount":12345,
            "score":"9.7","recommendation":"合成 推荐","subscriptionEligible":true
        })
    );
    assert_eq!(items[1]["table"], json!({"programCount":0,"playCount":0}));
    assert_eq!(items[2]["table"], json!({}));
    assert_eq!(items[1]["podcast"]["creator"], "不可用");
    assert_eq!(items[2]["podcast"]["creator"], "未核验");
    assert!(!view.to_string().contains("PRIVATE"));
    let calls = api.calls.lock().unwrap();
    let (path, args) = calls.last().unwrap();
    assert_eq!(path, "/api/search/voicelist/get");
    assert_eq!(args["offset"], 6);
    assert_eq!(args["limit"], 10);
}

#[tokio::test]
async fn podcast_search_membership_only_accepts_complete_account_snapshots() {
    let (_root, api, session, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    for (response, expected) in [
        (
            json!({"code":200,"djRadios":[{"id":301,"private":"PRIVATE"}],"hasMore":false,"count":1}),
            json!(["301"]),
        ),
        (
            json!({"code":200,"djRadios":[],"hasMore":false,"count":0}),
            json!([]),
        ),
        (json!({"code":200,"djRadios":[{"id":301}]}), Value::Null),
        (json!({"code":200,"djRadios":[],"count":0}), Value::Null),
        (
            json!({"code":200,"djRadios":[],"hasMore":false}),
            Value::Null,
        ),
        (
            json!({"code":200,"djRadios":[{"id":301}],"hasMore":true,"count":1}),
            Value::Null,
        ),
        (
            json!({"code":200,"djRadios":[],"hasMore":false,"count":2}),
            Value::Null,
        ),
        (
            json!({"code":200,"djRadios":[{"id":301},{"id":301}],"hasMore":false,"count":2}),
            Value::Null,
        ),
        (
            json!({"code":200,"djRadios":[{"id":"bad"}],"hasMore":false,"count":1}),
            Value::Null,
        ),
        (
            json!({"code":200,"djRadios":(1..=1000).map(|id|json!({"id":id})).collect::<Vec<_>>(),"hasMore":false,"count":1000}),
            Value::Null,
        ),
        (json!({"code":503}), Value::Null),
    ] {
        *api.reply.lock().unwrap() = response;
        let membership =
            serde_json::to_value(client.podcast_search_membership(&epoch).await.unwrap()).unwrap();
        assert_eq!(membership["value"]["subscribedIds"], expected);
        assert!(!membership.to_string().contains("PRIVATE"));
    }
    assert!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .all(|(path, payload)| path == "/api/djradio/get/subed"
                && payload == &json!({"targetUserId":"2","limit":1000}))
    );
    *api.reply.lock().unwrap() =
        json!({"code":200,"djRadios":[{"id":301}],"hasMore":false,"count":1});
    api.hold.store(true, Ordering::SeqCst);
    let in_flight = client.podcast_search_membership(&epoch);
    tokio::pin!(in_flight);
    tokio::select! {
        _ = api.started.notified() => {}
        result = &mut in_flight => panic!("unexpected early membership reply: {}", result.is_ok()),
    }
    session
        .adopt(api.clone(), json!({"userId":3}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(matches!(
        in_flight.await,
        Err(cumulus_core::Error::StaleResponse)
    ));
}

#[tokio::test]
async fn playlist_search_distinguishes_empty_from_missing_track_evidence() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = json!({"code":200,"result":{"playlists":[
        {"id":1,"name":"Empty","userId":2,"trackCount":0},
        {"id":2,"name":"Unknown","userId":2},
        {"id":3,"name":"Preview","userId":2,"trackCount":0,"trackIds":[{"id":17,"extra":"PRIVATE_TRACK_SENTINEL"}]},
        {"id":4,"name":"Empty by IDs","userId":2,"trackIds":[]},
        {"id":5,"name":"Has songs","userId":2,"trackCount":3,"trackIds":[]}
    ],"playlistCount":5,"hasMore":false}});
    let view = client
        .search_results(&epoch, query(C::Playlists))
        .await
        .unwrap()
        .value;
    let view = serde_json::to_value(view).unwrap();
    let items = view["sections"][0]["items"].as_array().unwrap();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items
            .iter()
            .map(|v| v["knownEmpty"].as_bool().unwrap_or(false))
            .collect::<Vec<_>>(),
        vec![true, false, false, true, false]
    );
    assert!(!view.to_string().contains("trackIds"));
    assert!(!view.to_string().contains("PRIVATE_TRACK_SENTINEL"));
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
async fn lyrics_use_full_raw_batches_not_more_hints_and_do_not_rewrite_the_visit() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let mut request = query(C::Lyrics);
    request.keyword = "合成已纠正词".into();
    request.limit = 10;
    let hit = |id| json!({"song":{"id":id,"name":"合成"},"lyrics":"合成歌词"});
    let mut first: Vec<_> = (1..=9).map(hit).collect();
    first.push(json!({"song":{"name":"缺少标识"},"lyrics":"不可展示"}));
    for (resources, offset, total, has_more, expected) in [
        (json!(first), 0, 9, false, (10, 9, 1, true)),
        (
            json!([hit(9), hit(9), hit(10)]),
            10,
            99,
            true,
            (13, 3, 0, false),
        ),
        (Value::Null, 20, 99, true, (20, 0, 0, false)),
    ] {
        request.offset = offset;
        *api.reply.lock().unwrap() = json!({"code":200,"data":{
            "resources":resources,"totalCount":total,"hasMore":has_more,
            "queryRewriteInfo":{"rewriteQuery":"无关二次纠正"},
            "queryRewrite":{"rewriteQuery":"无关备用词"}
        },"queryRewriteInfo":{"rewriteQuery":"无关外层词"}});
        let page = client
            .search_results(&epoch, request.clone())
            .await
            .unwrap()
            .value;
        let section = &page.sections[0];
        assert_eq!(page.total, Some(total));
        assert_eq!(
            (
                page.next_offset,
                section.items.len(),
                page.unavailable_count,
                page.more
            ),
            expected
        );
        assert_eq!(page.query.keyword, request.keyword);
        assert!(
            page.correction.is_none() && page.correction_parts.is_none() && page.notice.is_none()
        );
        if offset == 10 {
            let items = serde_json::to_value(&section.items).unwrap();
            assert_eq!(items[0]["track"]["id"], "9");
            assert_eq!(items[1]["track"]["id"], "9");
        }
    }
    for data in [json!({"totalCount":10}), json!({"resources":{}})] {
        *api.reply.lock().unwrap() = json!({"code":200,"data":data});
        assert!(matches!(
            client.search_results(&epoch, request.clone()).await,
            Err(Error::Response("missing search result list"))
        ));
    }
    let calls = api.calls.lock().unwrap();
    assert_eq!(calls.len(), 5);
    for ((path, args), offset) in calls.iter().zip([0, 10, 20, 20, 20]) {
        assert_eq!(path, "/api/search/resource/lyric");
        assert_eq!(
            *args,
            json!({"keyword":"合成已纠正词","scene":"normal","limit":10,"offset":offset})
        );
    }
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

#[tokio::test]
async fn unverified_comprehensive_response_cannot_enable_song_hints() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"blocks":[{
        "blockCode":"search_block_song","resources":[
            {"type":"song","baseInfo":{"id":1,"name":"有歌词","newSongExt":{"songAlias":"不要展示"}},
             "extInfo":{"lyrics":{"txt":"🎵合成歌词","range":[{"first":2,"second":4}],
             "url":"https://private.invalid/PRIVATE"}}},
            {"type":"song","baseInfo":{"id":2,"name":"坏匹配范围","newSongExt":{"songAlias":"旧别名"}},
             "extInfo":{"songAlias":"新<em>合成</em>别名","lyrics":{"txt":"🎵合成","range":[{"first":1,"second":4}]}}},
            {"type":"song","baseInfo":{"id":3,"name":"无公开匹配","extInfo":{"songAlias":"https://private.invalid/PRIVATE"}}},
            {"type":"song","baseInfo":{"id":4,"name":"混乱范围","extInfo":{"lyrics":{
                "txt":"合成歌词","range":[{"first":1,"second":3},{"first":0,"second":2}]}}}}
        ]
    }]}});
    let view = client
        .search_results(&epoch, query(C::All))
        .await
        .unwrap()
        .value;
    let projected = serde_json::to_value(&view).unwrap();
    let rows = &projected["sections"][0]["items"];
    assert_eq!(rows.as_array().unwrap().len(), 4);
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("hint").is_none())
    );
    // Positive normalization and UTF-16 assertions now use the real signed
    // transport in pc_experiment_transport/search_footer.rs, not this Api.
    assert!(!projected.to_string().contains("PRIVATE"));
    assert_eq!(api.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn comprehensive_song_badges_respect_official_numeric_values() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"blocks":[{
        "blockCode":"search_block_song","resources":[
            {"type":"song","baseInfo":{"id":1,"name":"歌曲标记","mark":1048576,"originCoverType":1}},
            {"type":"song","baseInfo":{"id":2,"name":"专辑回退","mark":0,"originCoverType":"1","album":{"mark":1048576}}},
            {"type":"song","baseInfo":{"id":3,"name":"歌曲标记优先","mark":1,"album":{"mark":1048576}}},
            {"type":"song","baseInfo":{"id":4,"name":"不可信标记","mark":"1048576","al":{"mark":1048576},"explicit":true}}
        ]
    }]}});
    let view = client
        .search_results(&epoch, query(C::All))
        .await
        .unwrap()
        .value;
    let projected = serde_json::to_value(&view).unwrap();
    let rows = &projected["sections"][0]["items"];
    assert_eq!(rows[0]["originalRecording"], true);
    assert!(rows[1].get("originalRecording").is_none());
    assert!(!projected.to_string().contains("originCoverType"));
    assert_eq!(rows[0]["explicit"], true);
    assert_eq!(rows[1]["explicit"], true);
    assert!(rows[2].get("explicit").is_none());
    assert!(rows[3].get("explicit").is_none());
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("mark").is_none())
    );
}

#[tokio::test]
async fn unverified_search_has_no_reason_entry_and_existing_explicit_detail_stays_typed() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"blocks":[
        {"blockCode":"search_block_song","resources":[{"type":"song","baseInfo":{
            "id":3,"name":"标签曲目","newSongExt":{"algClickableTags":[
                {"text":"旧标签","clickable":true,"reasonType":1,"reasonId":"old","resourceId":3}
            ]},"extInfo":{"algClickableTags":[
                {"text":"推荐","clickable":true,"reasonType":1,"reasonId":"r-9","resourceId":3},
                {"text":"城市","clickable":true,"reasonType":5,"url":"https://example.invalid/private?chartCode=top-1&targetId=77&targetType=4"},
                {"text":"无路由","clickable":true,"reasonType":5,"url":"https://example.invalid/?chartCode=a&chartCode=b&targetId=77&targetType=4"}
            ]}
        }}]}
    ]}});
    let response = client.search_results(&epoch, query(C::All)).await.unwrap();
    let value = serde_json::to_value(response.value).unwrap();
    assert!(value["sections"][0]["items"][0].get("reasons").is_none());
    // This existing read-only detail API remains a separate typed use case.
    // A generic search Api cannot advertise its entry; positive tag projection
    // is covered through the native signed full-batch transport separately.
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"title":"推荐详情","playCount":"34",
        "reasonRcmdSongCommonVOS":[{"songData":{"id":21,"name":"推荐曲目","url":"https://private.invalid/token"}}]
    }});
    let target = serde_json::from_value(json!({
        "kind":"recommendation","reasonId":"r-9","songId":"3","name":"推荐"
    }))
    .unwrap();
    let page = client.search_reason_page(&epoch, target).await.unwrap();
    let page = serde_json::to_value(page.value).unwrap();
    assert_eq!(page["tracks"][0]["id"], "21");
    assert_eq!(page["playCount"], 34);
    assert!(!page.to_string().contains("private.invalid"));
    let calls = api.calls.lock().unwrap();
    assert_eq!(
        calls.last().unwrap(),
        &(
            "/api/homepage/rcmd/tag/songs".into(),
            json!({
                "reasonId":"r-9","reasonName":"推荐","reasonType":1,"songId":"3"
            })
        )
    );
}

#[tokio::test]
async fn voice_category_links_only_the_preferred_nested_author_account() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let rows = [
        json!({"id":401,"name":"声音甲","mainSong":{"id":1},
               "dj":{"userId":"90071992547409931230","nickName":" 合成主播 ","signature":"PRIVATE_CREDIT"},
               "creator":{"userId":777,"nickname":"不应合并的作者"},
               "radio":{"id":301,"name":"父播单","dj":{"userId":888,"nickname":"播单作者"}}}),
        json!({"id":402,"name":"声音乙","mainSong":{"id":1},
               "creator":{"userId":502,"userName":"离线账号","accountStatus":"40"}}),
        json!({"id":403,"name":"声音丙","userId":503,"creatorName":"只有文本",
               "radio":{"id":303,"name":"父播单","dj":{"userId":503,"nickname":"父播单作者"}}}),
        json!({"id":404,"name":"声音丁","dj":{"userId":"not-account","nickname":"仅展示"},
               "creator":{"userId":504,"nickname":"不替代无效的 DJ"}}),
        json!({"id":405,"name":"声音戊","creator":{"userId":505,"nickname":"  "}}),
        json!({"id":406,"name":"声音己","creator":{"userId":506,"userName":"第二种作者对象"}}),
    ];
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"resources":rows.iter()
        .map(|row| json!({"type":"voice","baseInfo":row})).collect::<Vec<_>>(),
        "totalCount":6,"hasMore":false}});
    let value = serde_json::to_value(
        client
            .search_results(&epoch, query(C::Voices))
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    let items = value["sections"][0]["items"].as_array().unwrap();
    assert_eq!(items.len(), 6);
    assert_eq!(items[0]["creatorId"], "90071992547409931230");
    assert_eq!(items[0]["voice"]["creator"], "合成主播");
    assert_eq!(items[0]["voice"]["radioId"], "301");
    assert_eq!(items[1]["voice"]["creator"], "离线账号");
    assert_eq!(items[2]["voice"]["creator"], "只有文本");
    assert_eq!(items[3]["voice"]["creator"], "仅展示");
    assert!(items[1..5].iter().all(|row| row.get("creatorId").is_none()));
    assert_eq!(items[5]["creatorId"], "506");
    assert_eq!(items[5]["voice"]["creator"], "第二种作者对象");
    assert!(!value.to_string().contains("PRIVATE"));
    let calls = api.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "/api/search/voice/get");
}

#[tokio::test]
async fn comprehensive_voice_card_does_not_get_dedicated_table_author_action() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let mut fixture: Value =
        serde_json::from_str(include_str!("fixtures/search-results.json")).unwrap();
    let sections = fixture["all"]["data"]["blocks"].as_array_mut().unwrap();
    let voice = sections
        .iter_mut()
        .find(|section| {
            section["resources"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row["type"] == "voice"))
        })
        .unwrap();
    let row = voice["resources"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["type"] == "voice")
        .unwrap();
    row["baseInfo"]["dj"] = json!({"userId":501,"nickname":"作者"});
    *api.reply.lock().unwrap() = fixture["all"].clone();
    let value = serde_json::to_value(
        client
            .search_results(&epoch, query(C::All))
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    let voice = value["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|section| section["items"].as_array().unwrap())
        .find(|row| row["kind"] == "voice")
        .unwrap();
    assert_eq!(voice["voice"]["creator"], "作者");
    assert!(voice.get("creatorId").is_none());
}

#[tokio::test]
async fn dedicated_song_titles_keep_exact_row_alias_and_utf16_lyric_evidence() {
    let (_root, api, _, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = json!({"code":200,"data":{"resources":[
        {
            "alia":["wrong outer alias"],
            "extInfo":{
                "songAlias":true,
                "lyrics":{"txt":"🎵合成歌词","range":[{"first":2,"second":4}]}
            },
            "baseInfo":{"simpleSongData":{
                "id":42,"name":"合成歌名","alia":["首选不是命中","别名合成段"],
                "tns":["不应显示的翻译合成"],"extInfo":{"songAlias":"仅是标记"},
                "lyrics":{"txt":"错误的内层歌词","range":[]}
            }}
        },
        {
            "extInfo":{"songAlias":false,"lyrics":{"txt":"另一行","range":[]}},
            "baseInfo":{"simpleSongData":{
                "id":42,"name":"同 ID 另一行","alia":["别名合成段"],"newSongExt":{"songAlias":true}
            }}
        },
        {
            "extInfo":{"songAlias":true,"lyrics":{"txt":"🎵合成","range":[{"first":1,"second":3}]}},
            "baseInfo":{"simpleSongData":{"id":43,"name":"不完整范围","alia":[],"ala":["不能回退"]}}
        },
        {
            "newSongExt":{"songAlias":true},
            "lyric":{"txt":"旧字段歌词","range":[]},
            "baseInfo":{"simpleSongData":{"id":44,"name":"旧别名键","ala":["第一普通别名","第二普通别名"]}}
        },
        {
            "extInfo":{"songAlias":true},
            "baseInfo":{"simpleSongData":{"id":45,"name":"不替换空别名","alia":["","第二合成别名"]}}
        },
        {
            "extInfo":{"songAlias":true},
            "baseInfo":{"simpleSongData":{"id":46,"name":"没有合并外层标签","extInfo":{},"alia":["合成别名"]}}
        }
    ]}});
    let page = serde_json::to_value(
        client
            .search_results(&epoch, query(C::Songs))
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    let items = page["sections"][0]["items"].as_array().unwrap();
    assert_eq!(items.len(), 6);
    assert_eq!(items[0]["track"]["id"], "42");
    assert_eq!(items[1]["track"]["id"], "42");
    assert_eq!(
        items[0]["tableHint"],
        json!({
            "aliasParts":[{"text":"别名","match":false},{"text":"合成","match":true},{"text":"段","match":false}],
            "lyricParts":[{"text":"🎵","highlight":false},{"text":"合成","highlight":true},{"text":"歌词","highlight":false}]
        })
    );
    assert_eq!(
        items[1]["tableHint"],
        json!({"lyricParts":[{"text":"另一行","highlight":false}]})
    );
    assert_eq!(
        items[3]["tableHint"],
        json!({
            "aliasParts":[{"text":"第一普通别名","match":false}],
            "lyricParts":[{"text":"旧字段歌词","highlight":false}]
        })
    );
    for index in [2, 4, 5] {
        assert!(items[index].get("tableHint").is_none());
    }
    for row in items {
        assert!(
            row.get("hint").is_none(),
            "the comprehensive hint is separate"
        );
        assert!(row.get("reasons").is_none());
    }
    assert_eq!(
        api.calls.lock().unwrap().as_slice(),
        [(
            "/api/search/song/list/page".into(),
            json!({"keyword":"合成","offset":0,"limit":30,"scene":"normal","needCorrect":true,"channel":"suggest"})
        )]
    );
}
