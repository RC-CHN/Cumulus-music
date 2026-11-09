use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{SearchDiscoveryQuery as Q, SearchHistoryCommand as H},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;
struct Backend {
    calls: Mutex<Vec<(String, Value)>>,
    invalid: AtomicBool,
    malformed_audio: AtomicBool,
    hold: AtomicBool,
    hold_assignment: AtomicBool,
    assignment_calls: AtomicUsize,
    group: Mutex<Option<&'static str>>,
    started: Notify,
    release: Notify,
}
impl Api for Backend {
    fn search_audio_toplist_assignment(&self) -> ApiFuture<'_, bool> {
        Box::pin(async move {
            self.assignment_calls.fetch_add(1, Ordering::SeqCst);
            let group = *self.group.lock().unwrap();
            if self.hold_assignment.load(Ordering::SeqCst) {
                self.started.notify_one();
                self.release.notified().await;
            }
            let rows = group
                .map(|group| json!({"expName":"pc-search-audio-toplist","expGroupName":group}));
            Ok(PcExperimentGroups::refreshed(
                &json!({"data": rows.into_iter().collect::<Vec<_>>()}),
            )?
            .search_audio_toplist_assigned())
        })
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), args.clone()));
            if self.hold.load(Ordering::SeqCst) {
                self.started.notify_one();
                self.release.notified().await;
            }
            if self.invalid.load(Ordering::SeqCst) {
                return Ok(json!({"code":200,"data":{"unexpected":[]}}));
            }
            let data = match path {
                "/api/search/default/keyword/get" => {
                    json!({"refreshTime":1,"keywords":[{"showKeyword":"显示词","realkeyword":"实际词","bizQueryInfo":"PRIVATE"}]})
                }
                "/api/search/pc/rcmd/keyword/get" => {
                    json!({"operateWords":{"first":{"keyword":"运营词"}},"algWords":[{"keyword":"推荐词","toUserWord":"推荐显示","url":"https://private.invalid"}]})
                }
                "/api/search/pc/chart/list" => {
                    let audio = if self.malformed_audio.load(Ordering::SeqCst) {
                        json!({"malformed":true})
                    } else {
                        json!([{"toUserWord":"合成播客","url":"orpheus://audio/detail?id=301&proof=PRIVATE"}])
                    };
                    json!({"tabInfo":{"list":[{"id":"HOT_SEARCH_SONG#@#","name":"热搜榜","needPlay":true},{"id":"HOT_SEARCH_DJ#@#","name":"播客榜","needPlay":true},{"id":"HOT_SEARCH_DJ_VOICE_BOOK#@#","name":"听书榜"},{"id":"unknown","name":"不可用分支"}]},"items":{"HOT_SEARCH_DJ#@#":audio,"HOT_SEARCH_SONG#@#":[{"searchWord":"热词","realkeyword":"备用词","toUserWord":"长标题","iconType":1,"tabId":"HOT_SEARCH_SONG#@#","url":"PRIVATE"},{"searchWord":"普通榜词","tabId":"HOT_SEARCH_STYLE_ROCK#@#"}]}})
                }
                "/api/search/pc/chart/detail" if args["id"] == "HOT_SEARCH_DJ_VOICE_BOOK#@#" => {
                    json!({"itemList":[
                        {"toUserWord":"合成有声书","url":"orpheus://audio/detail?id=302&proof=PRIVATE"},
                        {"toUserWord":"外部链接","url":"https://private.invalid/?id=303"},
                        {"toUserWord":"重复参数","url":"orpheus://audio/detail?id=304&id=305"},
                        {"toUserWord":"非法编号","url":"orpheus://audio/detail?id=../304"}]})
                }
                "/api/search/pc/chart/detail" => {
                    json!({"itemList":[{"searchWord":"榜单补读","toUserWord":"榜单补读"}]})
                }
                "/api/hot/search/relate/song/get" => {
                    json!({"songs":[{"song":{"id":88,"url":"PRIVATE"}},{"song":{"id":"67"}},{"song":{"id":88}}]})
                }
                "/api/search/pc/suggest/keyword/get" if args["keyword"] == "A+B" => {
                    json!({
                        "recTitle":"猜你想搜",
                        "recs":[{"searchWord":"真实推荐","keyword":"备用推荐","name":"推荐卡片"}],
                        "suggests":[
                            {"searchWord":"真实联想","keyword":"备用词","name":"候选 A+B",
                             "toUserWord":"输入栏文案","realkeyword":"不要默认词",
                             "showKeyword":"不要默认显示","cookie":"PRIVATE"},
                            {"searchWord":"","keyword":"无专用展示词","name":"次条候选"},
                            {"toUserWord":"唯一可选词"},
                            {"searchWord":{"invalid":true},"keyword":"不降级到其他词"}
                        ]
                    })
                }
                "/api/search/pc/suggest/keyword/get" => {
                    json!({
                        "recTitle":"猜你想搜",
                        "recs":[{"keyword":"推荐建议","name":"合成：组合词与樂隊、乐队"}],
                        "suggests":[{"keyword":"联想词","name":"显示名称：乐队","cookie":"PRIVATE"}]
                    })
                }
                _ => return Err(Error::Response("unexpected search fixture request")),
            };
            Ok(json!({"code":200,"data":data}))
        })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    MusicClient,
    Arc<cumulus_core::storage::StateStore>,
) {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let api = Arc::new(Backend {
        calls: Mutex::default(),
        invalid: AtomicBool::new(false),
        malformed_audio: AtomicBool::new(false),
        hold: AtomicBool::new(false),
        hold_assignment: AtomicBool::new(false),
        assignment_calls: AtomicUsize::new(0),
        group: Mutex::new(None),
        started: Notify::new(),
        release: Notify::new(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry);
    (root, api, session, client, store)
}
#[tokio::test]
async fn official_pc_search_shapes_remain_typed_and_do_not_expose_tracking_or_urls() {
    let (_r, api, session, client, _) = fixture();
    let e = session.token().unwrap().generation.to_string();
    let queries = [
        Q::Defaults,
        Q::Recommendations {
            last_query: " 上次输入 ".into(),
        },
        Q::Charts,
        Q::Chart {
            id: "HOT_SEARCH_SONG#@#".into(),
        },
        Q::Suggestions {
            keyword: " 组合词 樂隊 ".into(),
        },
    ];
    let mut values = vec![];
    for query in queries {
        values
            .push(serde_json::to_value(client.search_discovery(&e, query).await.unwrap()).unwrap());
    }
    assert_eq!(values[0]["value"]["words"][0]["keyword"], "实际词");
    assert_eq!(values[0]["value"]["refreshMs"], 2000);
    assert_eq!(values[1]["value"]["words"].as_array().unwrap().len(), 2);
    assert_eq!(values[2]["value"]["charts"].as_array().unwrap().len(), 1);
    for word in [
        &values[0]["value"]["words"][0],
        &values[1]["value"]["words"][0],
        &values[2]["value"]["charts"][0]["items"][0],
    ] {
        assert!(word.is_object());
        assert!(word.get("labelParts").is_none());
    }
    let suggestions = &values[4]["value"]["suggestions"][0];
    assert_eq!(suggestions["label"], "显示名称：乐队");
    assert_eq!(suggestions["keyword"], "联想词");
    assert_eq!(
        suggestions["labelParts"],
        json!([
            {"text":"显示名称：","match":false},
            {"text":"乐队","match":true}
        ])
    );
    let recommended = &values[4]["value"]["recommended"][0];
    assert_eq!(recommended["keyword"], "推荐建议");
    assert_eq!(
        recommended["labelParts"],
        json!([
            {"text":"合成：","match":false},
            {"text":"组合词","match":true},
            {"text":"与樂隊、","match":false},
            {"text":"乐队","match":true}
        ])
    );
    let encoded = serde_json::to_string(&values).unwrap();
    assert!(
        !encoded.contains("PRIVATE")
            && !encoded.contains("https://")
            && !encoded.contains("bizQueryInfo")
    );
    let calls = api.calls.lock().unwrap();
    assert_eq!(calls.len(), 5);
    assert_eq!(
        calls[4],
        (
            "/api/search/pc/suggest/keyword/get".into(),
            json!({"keyword":"组合词 樂隊"})
        )
    );
    assert_eq!(calls[1].1, json!({"lastQuery":"上次输入"}));
}
#[tokio::test]
async fn suggestion_projects_popup_submitted_query_and_input_text_independently() {
    let (_r, api, session, client, _) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    let reply = client
        .search_discovery(
            &epoch,
            Q::Suggestions {
                keyword: "A+B".into(),
            },
        )
        .await
        .unwrap();
    let value = serde_json::to_value(reply).unwrap()["value"].clone();
    let ordinary = value["suggestions"].as_array().unwrap();
    let recommended = value["recommended"].as_array().unwrap();
    assert_eq!(ordinary.len(), 3);
    assert_eq!(recommended.len(), 1);
    assert_eq!(ordinary[0]["label"], "候选 A+B");
    assert_eq!(ordinary[0]["keyword"], "真实联想");
    assert_eq!(ordinary[0]["inputLabel"], "输入栏文案");
    assert_eq!(
        ordinary[0]["labelParts"],
        json!([{"text":"候选 ","match":false},{"text":"A+B","match":true}])
    );
    assert_eq!(ordinary[1]["keyword"], "无专用展示词");
    assert_eq!(ordinary[1]["label"], "次条候选");
    assert_eq!(ordinary[1]["inputLabel"], "无专用展示词");
    assert_eq!(ordinary[2]["keyword"], "唯一可选词");
    assert_eq!(ordinary[2]["label"], "唯一可选词");
    assert!(ordinary[2].get("inputLabel").is_none());
    assert_eq!(recommended[0]["keyword"], "真实推荐");
    assert_eq!(recommended[0]["label"], "推荐卡片");
    assert_eq!(recommended[0]["inputLabel"], "真实推荐");
    let encoded = serde_json::to_string(&value).unwrap();
    for private in ["PRIVATE", "不要默认词", "不要默认显示", "不降级到其他词"] {
        assert!(!encoded.contains(private));
    }
    assert_eq!(
        *api.calls.lock().unwrap(),
        [(
            "/api/search/pc/suggest/keyword/get".into(),
            json!({"keyword":"A+B"})
        )]
    );
    assert!(
        client
            .search_history(&epoch, H::Read)
            .unwrap()
            .value
            .is_empty()
    );
}

#[test]
fn history_is_ordered_bounded_durable_and_account_isolated() {
    let (_r, api, session, client, store) = fixture();
    let e = session.token().unwrap().generation.to_string();
    for i in 0..35 {
        client
            .search_history(
                &e,
                H::Remember {
                    keyword: format!("词{i}"),
                },
            )
            .unwrap();
    }
    let value = client
        .search_history(
            &e,
            H::Remember {
                keyword: " 词20 ".into(),
            },
        )
        .unwrap()
        .value;
    assert_eq!(value.len(), 30);
    assert_eq!(value[0], "词20");
    client
        .search_history(
            &e,
            H::Remove {
                keyword: "词20".into(),
            },
        )
        .unwrap();
    assert_eq!(client.search_history(&e, H::Read).unwrap().value.len(), 29);
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    let next = session.token().unwrap().generation.to_string();
    assert!(client.search_history(&e, H::Clear).is_err());
    assert!(
        client
            .search_history(&next, H::Read)
            .unwrap()
            .value
            .is_empty()
    );
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let again = session.token().unwrap().generation.to_string();
    assert_eq!(
        client.search_history(&again, H::Read).unwrap().value.len(),
        29
    );
    store
        .put(
            "9",
            "search-history-v1",
            "keywords",
            &json!({"broken":true}),
        )
        .unwrap();
    assert!(client.search_history(&again, H::Read).is_err());
    assert!(
        client
            .search_history(&again, H::Clear)
            .unwrap()
            .value
            .is_empty()
    );
    assert!(api.calls.lock().unwrap().is_empty());
}
#[tokio::test]
async fn malformed_search_is_not_an_empty_success_and_late_account_results_are_rejected() {
    let (_r, api, session, client, _) = fixture();
    let e = session.token().unwrap().generation.to_string();
    api.invalid.store(true, Ordering::SeqCst);
    assert!(client.search_discovery(&e, Q::Charts).await.is_err());
    api.invalid.store(false, Ordering::SeqCst);
    api.hold.store(true, Ordering::SeqCst);
    let request = client.search_discovery(&e, Q::Defaults);
    tokio::pin!(request);
    tokio::select! {v=&mut request=>panic!("unexpected completion: {}",v.is_ok()),_ = api.started.notified()=>{}}
    session.detach().unwrap();
    api.release.notify_one();
    assert!(request.await.is_err());
}

#[tokio::test]
async fn hot_chart_play_resolves_only_ordered_song_ids_for_exact_keywords() {
    let (_r, api, session, client, _) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    let chart =
        serde_json::to_value(client.search_discovery(&epoch, Q::Charts).await.unwrap()).unwrap();
    assert_eq!(chart["value"]["charts"][0]["needPlay"], true);
    assert_eq!(
        chart["value"]["charts"][0]["playKeywords"],
        json!(["热词", "普通榜词"])
    );
    let items = &chart["value"]["charts"][0]["items"];
    assert_eq!(items[0]["keyword"], "热词");
    assert_eq!(items[0]["channel"], "hotquery");
    assert_eq!(items[1]["channel"], Value::Null);
    let value = serde_json::to_value(
        client
            .search_discovery(
                &epoch,
                Q::ChartSongs {
                    keywords: vec![" 热词 ".into(), "另一首".into(), "热词".into()],
                },
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        value["value"],
        json!({"kind":"chart_songs","ids":["88","67","88"]})
    );
    assert_eq!(
        api.calls.lock().unwrap()[1],
        (
            "/api/hot/search/relate/song/get".into(),
            json!({"keywords":"[\"热词\",\"另一首\",\"热词\"]"})
        )
    );
    for keywords in [vec![], vec!["过量".into(); 21], vec!["无\n效".into()]] {
        assert!(matches!(
            client
                .search_discovery(&epoch, Q::ChartSongs { keywords })
                .await,
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(api.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn audio_chart_rows_only_export_valid_internal_radio_ids() {
    let (_r, api, session, client, _store) = fixture();
    *api.group.lock().unwrap() = Some("t1");
    let epoch = session.token().unwrap().generation.to_string();
    let list = serde_json::to_value(
        client
            .search_discovery(&epoch, Q::Charts)
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    let audio = &list["charts"][1];
    assert_eq!(audio["audio"], true);
    assert_eq!(audio["needPlay"], false);
    assert!(audio["playKeywords"].is_null());
    assert_eq!(audio["items"][0]["radioId"], "301");
    assert!(list["charts"][0]["items"][0]["radioId"].is_null());
    let detail = serde_json::to_value(
        client
            .search_discovery(
                &epoch,
                Q::Chart {
                    id: "HOT_SEARCH_DJ_VOICE_BOOK#@#".into(),
                },
            )
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_eq!(detail["items"][0]["radioId"], "302");
    assert_eq!(detail["items"].as_array().unwrap().len(), 4);
    for row in &detail["items"].as_array().unwrap()[1..] {
        assert!(row["radioId"].is_null());
    }
    assert!(detail["playKeywords"].is_null());
    let encoded = serde_json::to_string(&(list, detail)).unwrap();
    assert!(
        !encoded.contains("PRIVATE")
            && !encoded.contains("orpheus://")
            && !encoded.contains("private.invalid")
    );
}

#[tokio::test]
async fn audio_toplist_is_filtered_before_projection_and_detail_io() {
    use cumulus_core::catalog::search::SearchDiscovery;

    let (_r, api, session, client, _) = fixture();
    *api.group.lock().unwrap() = Some("c");
    api.malformed_audio.store(true, Ordering::SeqCst);
    let first = session.token().unwrap().generation.to_string();
    let SearchDiscovery::Charts { charts } = client
        .search_discovery(&first, Q::Charts)
        .await
        .unwrap()
        .value
    else {
        panic!("expected chart list");
    };
    assert_eq!(
        charts
            .iter()
            .map(|chart| chart.id.as_str())
            .collect::<Vec<_>>(),
        ["HOT_SEARCH_SONG#@#"]
    );
    assert_eq!(api.calls.lock().unwrap()[0].0, "/api/search/pc/chart/list");
    for id in ["HOT_SEARCH_DJ#@#", "HOT_SEARCH_DJ_VOICE_BOOK#@#"] {
        assert!(matches!(
            client
                .search_discovery(&first, Q::Chart { id: id.into() })
                .await,
            Err(Error::InvalidInput("search audio chart unavailable"))
        ));
    }
    assert_eq!(api.calls.lock().unwrap().len(), 1);
    assert_eq!(api.assignment_calls.load(Ordering::SeqCst), 1);
    *api.group.lock().unwrap() = Some("t1");
    api.malformed_audio.store(false, Ordering::SeqCst);
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    let second = session.token().unwrap().generation.to_string();
    let SearchDiscovery::Charts { charts } = client
        .search_discovery(&second, Q::Charts)
        .await
        .unwrap()
        .value
    else {
        panic!("expected chart list");
    };
    assert_eq!(
        charts
            .iter()
            .map(|chart| chart.id.as_str())
            .collect::<Vec<_>>(),
        [
            "HOT_SEARCH_SONG#@#",
            "HOT_SEARCH_DJ#@#",
            "HOT_SEARCH_DJ_VOICE_BOOK#@#"
        ]
    );
    assert!(
        client
            .search_discovery(
                &second,
                Q::Chart {
                    id: "HOT_SEARCH_DJ_VOICE_BOOK#@#".into()
                }
            )
            .await
            .is_ok()
    );
    assert_eq!(api.assignment_calls.load(Ordering::SeqCst), 2);
    assert_eq!(api.calls.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn late_toplist_assignment_never_crosses_account_generation() {
    use cumulus_core::catalog::search::SearchDiscovery;

    let (_r, api, session, client, _) = fixture();
    *api.group.lock().unwrap() = Some("t1");
    api.hold_assignment.store(true, Ordering::SeqCst);
    let epoch = session.token().unwrap().generation.to_string();
    let first = client.search_discovery(&epoch, Q::Charts);
    tokio::pin!(first);
    tokio::select! {v=&mut first=>panic!("unexpected completion: {}",v.is_ok()),_ = api.started.notified()=>{}}
    assert_eq!(api.calls.lock().unwrap()[0].0, "/api/search/pc/chart/list");
    *api.group.lock().unwrap() = None;
    api.hold_assignment.store(false, Ordering::SeqCst);
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(first.await.is_err());
    let new_epoch = session.token().unwrap().generation.to_string();
    let SearchDiscovery::Charts { charts } = client
        .search_discovery(&new_epoch, Q::Charts)
        .await
        .unwrap()
        .value
    else {
        panic!("expected chart list");
    };
    assert_eq!(charts.len(), 1);
    assert_eq!(api.assignment_calls.load(Ordering::SeqCst), 2);
}
