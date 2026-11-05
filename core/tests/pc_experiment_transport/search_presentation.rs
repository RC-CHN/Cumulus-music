//! Re-project safe candidates against live native cohorts without re-searching.
use super::{HttpResponse, reply, vinyl_http, vinyl_http_with_credentials};
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{SearchCategory, SearchItem, SearchResultQuery},
    },
    protocol::http::ApiOrigin,
};
use serde_json::json;
use std::sync::Arc;

fn groups(enabled: bool) -> HttpResponse {
    json!({"code":200,"data":[
        {"expName":"PH-PC-searchHot","expGroupName":"t1"},
        {"expName":"PH-PC-search-result","expGroupName":if enabled {"t1"} else {"c"}},
        {"expName":"PH-PC-searchMore2","expGroupName":if enabled {"t1"} else {"c"}},
        {"expName":"PH-PC-searchPageEfficiency","expGroupName":if enabled {"t1"} else {"c"}}
    ]})
    .into()
}

#[tokio::test]
async fn current_and_cached_candidates_follow_live_cohorts_without_search_or_privilege_replacement()
{
    let root = tempfile::tempdir().unwrap();
    let (api, http) = vinyl_http(root.path(), &["MUSIC_U=synthetic-presentation"], [
        groups(true),
        json!({"code":200,"data":{"algTabs":[{"title":"综合"}]}}).into(),
        json!({"code":200,"data":{"blocks":[{"blockCode":"search_block_song","resources":[{
            "type":"song","baseInfo":{"simpleSongData":{"id":1,"name":"合成","mark":1048576}},
            "extInfo":{"songAlias":"<em>合成</em>别名","algClickableTags":[{
                "text":"原因","clickable":true,"reasonType":1,"reasonId":"reason","resourceId":1}]}
        }]}]}}).into(),
        groups(false), groups(true),
    ]);
    let registry = ProfileRegistry::open(&root.path().join("host"), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":71}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client =
        MusicClient::with_session(session.clone(), registry).with_official_search_experiment();
    let query = SearchResultQuery {
        keyword: "合成".into(),
        category: SearchCategory::All,
        offset: 0,
        limit: 30,
        correct: true,
        channel: None,
    };
    let page = client.search_results(&epoch, query).await.unwrap().value;
    let receipt = page
        .presentation_receipt
        .as_deref()
        .expect("native candidate receipt");
    let original_revision = page.presentation_revision.as_deref().unwrap();
    assert!(matches!(
        &page.sections[0].items[0],
        SearchItem::Song {
            hint: Some(_),
            explicit: true,
            ..
        }
    ));
    let first = client
        .search_presentation(&epoch, receipt, original_revision)
        .unwrap()
        .value;
    assert!(first.sections[0].items[0].hint.is_some());
    assert!(first.preview_layout.is_some());
    assert_eq!(http.calls().len(), 3);

    let disabled = client.refresh_pc_presentation().await.unwrap();
    assert!(!disabled.search_footer && !disabled.search_expanded && !disabled.search_dense);
    assert!(
        client
            .search_presentation(&epoch, receipt, original_revision)
            .is_err()
    );
    let patch = client
        .search_presentation(&epoch, receipt, &disabled.revision)
        .unwrap()
        .value;
    assert!(patch.preview_layout.is_none());
    assert!(patch.sections[0].items[0].hint.is_none());
    assert!(patch.sections[0].items[0].reasons.is_empty());
    assert_eq!(http.calls().len(), 4);

    let enabled = client.refresh_pc_presentation().await.unwrap();
    let restored = client
        .search_presentation(&epoch, receipt, &enabled.revision)
        .unwrap()
        .value;
    assert!(restored.sections[0].items[0].hint.is_some());
    assert!(restored.preview_layout.is_some());
    assert_eq!(http.calls().len(), 5);
    let encoded = serde_json::to_value(restored).unwrap();
    assert_eq!(encoded["sections"][0]["items"][0]["trackId"], "1");
    for key in ["track", "privilege", "playAllIds", "vipBadge", "coverUrl"] {
        assert!(encoded.get(key).is_none());
    }
    let current = session.token().unwrap();
    session.adopt(api, json!({"userId":72}), &current).unwrap();
    assert!(
        client
            .search_presentation(&epoch, receipt, &enabled.revision)
            .is_err()
    );
    assert_eq!(http.calls().len(), 5);
}

#[tokio::test]
async fn rotated_credentials_retire_old_candidates_even_after_fresh_treatment_without_research() {
    let root = tempfile::tempdir().unwrap();
    let (api, http, credentials) = vinyl_http_with_credentials(
        root.path(),
        &["MUSIC_U=synthetic-candidate-original"],
        [
            groups(true),
            json!({"code":200,"data":{"algTabs":[{"title":"综合"}]}}).into(),
            json!({"code":200,"data":{"blocks":[{"blockCode":"search_block_song","resources":[{
                "type":"song","baseInfo":{"simpleSongData":{"id":1,"name":"合成保留曲目"}},
                "extInfo":{"songAlias":"<em>合成</em>旧候选"}
            }]}]}})
            .into(),
            groups(true),
        ],
    );
    let registry = ProfileRegistry::open(&root.path().join("host"), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":71}), &session.token().unwrap())
        .unwrap();
    let token = session.token().unwrap();
    let epoch = token.generation.to_string();
    let client =
        MusicClient::with_session(session.clone(), registry).with_official_search_experiment();
    let page = client
        .search_results(
            &epoch,
            SearchResultQuery {
                keyword: "合成".into(),
                category: SearchCategory::All,
                offset: 0,
                limit: 30,
                correct: true,
                channel: None,
            },
        )
        .await
        .unwrap()
        .value;
    let receipt = page.presentation_receipt.as_deref().unwrap();
    let original = serde_json::to_value(&page).unwrap();
    let first = client
        .search_presentation(
            &epoch,
            receipt,
            page.presentation_revision.as_deref().unwrap(),
        )
        .unwrap();
    assert!(first.value.sections[0].items[0].hint.is_some());
    assert_eq!(http.calls().len(), 3);

    let target = ApiOrigin::Pc.url();
    let revision = credentials.request_credentials(&target).unwrap().revision;
    credentials
        .record_response(
            &target,
            &["MUSIC_U=synthetic-candidate-rotated; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    assert_eq!(session.token().unwrap(), token);
    let refreshed = client.refresh_pc_presentation().await.unwrap();
    assert!(refreshed.search_footer);
    assert_ne!(
        refreshed.revision,
        page.presentation_revision.as_deref().unwrap()
    );
    assert_eq!(http.calls().len(), 4);
    assert!(matches!(
        client.search_presentation(&epoch, receipt, &refreshed.revision),
        Err(cumulus_core::Error::StaleResponse)
    ));
    assert_eq!(
        http.calls().len(),
        4,
        "local stale-receipt handling cannot replay search"
    );
    assert_eq!(
        serde_json::to_value(&page).unwrap(),
        original,
        "ordinary returned rows are unchanged"
    );
    let paths = http
        .calls()
        .iter()
        .map(|call| call.path.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "/api/rtrs/abt/front/expinfo/list",
            "/api/search/pc/result/tab",
            "/api/search/pc/complex/page/v3",
            "/api/rtrs/abt/front/expinfo/list",
        ]
    );
}

#[tokio::test]
async fn cookie_rotating_search_response_cannot_issue_candidates_for_the_new_cohort() {
    let root = tempfile::tempdir().unwrap();
    let tabs = json!({"code":200,"data":{"algTabs":[{"title":"综合"}]}});
    let result = |id| {
        json!({"code":200,"data":{"blocks":[{"blockCode":"search_block_song","resources":[{
            "type":"song","baseInfo":{"simpleSongData":{"id":id,"name":"合成保留曲目","mark":1048576}},
            "extInfo":{"songAlias":"<em>合成</em>提示","algClickableTags":[{
                "text":"原因","clickable":true,"reasonType":1,"reasonId":"synthetic","resourceId":id}]}
        }]}]}})
    };
    let mut rotates = reply(result(1));
    rotates.set_cookies = vec!["MUSIC_U=synthetic-search-new; Path=/; Secure".into()];
    let (api, http) = vinyl_http(
        root.path(),
        &["MUSIC_U=synthetic-search-old; Path=/; Secure"],
        [
            groups(true),
            tabs.clone().into(),
            HttpResponse::Ready(Ok(rotates)),
            groups(true),
            tabs.into(),
            result(2).into(),
        ],
    );
    let registry = ProfileRegistry::open(&root.path().join("host"), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":71}), &session.token().unwrap())
        .unwrap();
    let token = session.token().unwrap();
    let epoch = token.generation.to_string();
    let client =
        MusicClient::with_session(session.clone(), registry).with_official_search_experiment();
    let query = SearchResultQuery {
        keyword: "合成换凭据".into(),
        category: SearchCategory::All,
        offset: 0,
        limit: 30,
        correct: true,
        channel: None,
    };

    let old = client
        .search_results(&epoch, query.clone())
        .await
        .unwrap()
        .value;
    assert_eq!(
        session.token().unwrap(),
        token,
        "Cookie rotation stays in the same account epoch"
    );
    assert!(old.presentation_receipt.is_none());
    assert!(old.presentation_revision.is_none());
    assert!(old.preview_layout.is_none());
    assert!(matches!(
        &old.sections[0].items[0],
        SearchItem::Song { track, hint: None, reasons, explicit: true, .. }
            if track.id == "1" && reasons.is_empty()
    ));
    assert_eq!(http.calls().len(), 3, "no retry or extra experiment read");

    let current = client.refresh_pc_presentation().await.unwrap();
    assert!(current.search_footer && current.search_expanded);
    let fresh = client.search_results(&epoch, query).await.unwrap().value;
    let receipt = fresh
        .presentation_receipt
        .as_deref()
        .expect("fresh candidate");
    assert_eq!(
        fresh.presentation_revision.as_deref(),
        Some(current.revision.as_str())
    );
    let patch = client
        .search_presentation(&epoch, receipt, &current.revision)
        .unwrap()
        .value;
    assert_eq!(patch.sections[0].items[0].track_id, "2");
    assert!(patch.sections[0].items[0].hint.is_some());
    assert!(!patch.sections[0].items[0].reasons.is_empty());
    let calls = http.calls();
    assert_eq!(calls.len(), 6);
    assert_eq!(calls[2].path, "/api/search/pc/complex/page/v3");
    assert!(
        calls[2]
            .cookie
            .as_deref()
            .unwrap()
            .contains("MUSIC_U=synthetic-search-old")
    );
    assert_eq!(calls[5].path, "/api/search/pc/complex/page/v3");
    assert!(
        calls[5]
            .cookie
            .as_deref()
            .unwrap()
            .contains("MUSIC_U=synthetic-search-new")
    );
}
