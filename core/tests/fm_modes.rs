#[allow(dead_code)]
#[path = "support/fm.rs"]
mod support;
use cumulus_core::{
    Error,
    fm::{discovery::*, *},
};
use serde_json::{Value, json};
use support::*;
const DETAILS: &str = "/api/v3/song/detail";
const ZONE: &str = "/api/tag/tab/fm/recommends";
const TABS: &str = "/api/zone/tab/get";
fn zone(id: &str) -> FmSource {
    FmSource::Zone {
        rule_id: id.into(),
        trial_mode: None,
    }
}
fn set(f: &Fixture, path: &str, value: Value) {
    f.api.responses.lock().unwrap().insert(path.into(), value);
}
fn page(tab: &str) -> Value {
    json!({"tabKey":tab,"blocks":[{"componentName":"RecommendHorizontal","children":[{"resourceType":"fm","resourceId":"27001","title":"rock"},{"resourceType":"playlist","resourceId":"4"}]}]})
}
#[test]
fn familiar_wire_seeds_and_zone_context_are_validated_without_numeric_precision_loss() {
    for seed in [
        r#"["27566335","473276732","555291080"]"#,
        r#"["9007199254740993123"]"#,
        "123",
    ] {
        FmSource::Familiar {
            source_id: seed.into(),
        }
        .validate()
        .unwrap();
    }
    for seed in [
        "[]",
        r#"["1",0]"#,
        r#"["1","bad"]"#,
        "[1.2]",
        "[null]",
        "{}",
    ] {
        assert!(
            FmSource::Familiar {
                source_id: seed.into()
            }
            .validate()
            .is_err()
        );
    }
    assert_eq!(zone("2").playback_trial_mode(), 121);
    assert!(
        FmSource::Zone {
            rule_id: "2".into(),
            trial_mode: Some(42)
        }
        .validate()
        .is_err()
    );
}
#[tokio::test]
async fn zone_resolves_details_with_privileges_preserves_reference_order_and_does_not_double_short_batches()
 {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    f.api.batch(&[2, 1]);
    set(
        &f,
        DETAILS,
        json!({"code":200,"songs":[song(1),song(2),song(99)],"privileges":[{"id":2,"fee":1}]}),
    );
    let q =
        f.fm.preload_for(&t, zone("27001"), FmRights::default())
            .await
            .unwrap();
    assert_eq!(
        q.tracks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        ["2", "1"]
    );
    assert_eq!(q.tracks[0].song["privilege"]["fee"], 1);
    assert_eq!(f.api.count(ZONE), 1);
    let calls = f.api.calls.lock().unwrap();
    assert_eq!(
        calls[0].1,
        json!({"ruleType":1,"limit":3,"ruleId":"27001","firstSongId":"","firstSongAlg":"","tabTagId":""})
    );
    assert_eq!(calls[1].1["trialMode"], -1);
    let c: Value = serde_json::from_str(calls[1].1["c"].as_str().unwrap()).unwrap();
    assert_eq!(c, json!([{"id":"2","v":0},{"id":"1","v":0}]));
}
#[tokio::test]
async fn modes_keep_independent_queues_and_detail_failure_does_not_commit_partial_refill() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    let personal = f.preload(&[10, 11, 12]).await;
    let seeds = FmSource::Familiar {
        source_id: r#"["10","11"]"#.into(),
    };
    f.api.batch(&[20, 21, 22]);
    let familiar =
        f.fm.preload_for(&t, seeds.clone(), FmRights::default())
            .await
            .unwrap();
    assert_eq!(
        f.api.calls.lock().unwrap().last().unwrap().1["sourceIds"],
        r#"["10","11"]"#
    );
    f.api.batch(&[30]);
    set(&f, DETAILS, json!({"code":200,"songs":[song(30)]}));
    let zone_queue =
        f.fm.preload_for(&t, zone("33"), FmRights::default())
            .await
            .unwrap();
    f.api.batch(&[31]);
    f.api.fault(DETAILS, Fault::Before);
    assert!(
        f.fm.preload_for(&t, zone("33"), FmRights::default())
            .await
            .is_err()
    );
    let reopened = FmService::new(f.session.clone()).unwrap();
    for q in [personal, familiar, zone_queue] {
        assert_eq!(
            serde_json::to_value(reopened.queue_for(&t, &q.source).unwrap().unwrap()).unwrap(),
            serde_json::to_value(q).unwrap()
        );
    }
    f.adopt(10);
    assert!(
        reopened
            .queue_for(&f.session.token().unwrap(), &zone("33"))
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        reopened.queue_for(&t, &seeds),
        Err(Error::StaleResponse)
    ));
}
#[tokio::test]
async fn discovery_parses_official_home_and_navigation_and_cache_reopens_without_http() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    set(
        &f,
        "/api/pc/daily/rcmd/block",
        json!({"code":200,"data":{"items":[
            {"resourceType":"fm","coverText":"私人漫游"},
            {"resourceType":"similarSong","resourceId":"[\"1\",\"2\"]","coverText":"相似歌曲","title":"songs"},
            {"resourceType":"similarSong","resourceId":"[]"},
            {"resourceType":"similarArtist","resourceId":"[\"9\"]"}
        ]}}),
    );
    set(
        &f,
        "/api/secondary/tab/list",
        json!({"code":200,"data":{"hints":[{"position":{"code":"music_top_tab_full"},"data":{"extra":{"nowChannelItems":[
            {"code":"feature","url":"orpheus://rnpage?page=ignore"},
            {"code":"rock","title":"摇滚","url":"orpheus://rnpage?page=abc_12&x=3"},
            {"code":"bad","url":"orpheus://rnpage?page=../wrong"}
        ]}}}]}}),
    );
    let service = FmDiscovery::new(f.session.clone()).unwrap();
    let home = service.page_for(&t, &FmDiscoveryQuery::Home).await.unwrap();
    assert_eq!(home.page.cards.len(), 2);
    assert_eq!(home.page.zones.len(), 1);
    assert_eq!(home.page.zones[0].page_code, "abc_12");
    let count = f.api.calls.lock().unwrap().len();
    let reopened = FmDiscovery::new(f.session.clone()).unwrap();
    assert_eq!(
        reopened.cached_for(&t, &FmDiscoveryQuery::Home).unwrap(),
        Some(home)
    );
    assert_eq!(count, f.api.calls.lock().unwrap().len());
    f.adopt(10);
    assert!(
        reopened
            .cached_for(&f.session.token().unwrap(), &FmDiscoveryQuery::Home)
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn mismatched_zone_tab_uses_official_default_fallback_but_expiration_does_not() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    set(
        &f,
        TABS,
        json!({"code":200,"data":{"currentTabContent":page("wrong"),"tabMetas":[]}}),
    );
    set(
        &f,
        "/api/zone/get",
        json!({"code":200,"data":[page("default")]}),
    );
    let service = FmDiscovery::new(f.session.clone()).unwrap();
    let query = FmDiscoveryQuery::Zone {
        page_code: "rock".into(),
        tab_key: "hot".into(),
    };
    let p = service.page_for(&t, &query).await.unwrap();
    assert!(p.page.default_fallback);
    assert_eq!(p.page.active_tab.as_deref(), Some("default"));
    assert_eq!(p.page.cards.len(), 1);
    f.api.fault(TABS, Fault::Reject(301));
    assert!(service.page_for(&t, &query).await.is_err());
    assert_eq!(f.api.count("/api/zone/get"), 1);
}
#[tokio::test]
async fn late_zone_response_cannot_publish_into_new_account_or_replace_saved_page() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    set(
        &f,
        TABS,
        json!({"code":200,"data":{"currentTabContent":page("default"),"tabMetas":[]}}),
    );
    *f.api.hold.lock().unwrap() = Some(TABS.into());
    let session = f.session.clone();
    let old = t.clone();
    let query = FmDiscoveryQuery::Zone {
        page_code: "rock".into(),
        tab_key: "default".into(),
    };
    let q = query.clone();
    let task =
        tokio::spawn(async move { FmDiscovery::new(session).unwrap().page_for(&old, &q).await });
    f.api.started.notified().await;
    f.adopt(10);
    f.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        FmDiscovery::new(f.session.clone())
            .unwrap()
            .cached_for(&f.session.token().unwrap(), &query)
            .unwrap()
            .is_none()
    );
    assert!(f.store.values("9", "fm-discovery-v1").unwrap().is_empty());
}
