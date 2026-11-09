//! Real signed transport, frozen synthetic data, and no network backend.
use super::{HttpResponse, ScriptHttp, gate, reply, vinyl_http};
use cumulus_core::{
    Error,
    account::AccountSession,
    client::{
        MusicClient,
        dto::{SearchCategory, SearchResultQuery},
        pc_experiments::OrdinarySkipAssignment,
    },
    protocol::{
        http::MusicHttpClient,
        pc_experiments::{ASSIGNMENT_PATH, assignment_arguments},
    },
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const LEGACY: &str = "/api/search/pc/complex/page";
const V3: &str = "/api/search/pc/complex/page/v3";
const TABS: &str = "/api/search/pc/result/tab";
const WORD: &str = "合成页脚";
const COOKIE: &str = "MUSIC_U=synthetic-search-footer";

struct Fixture {
    _root: tempfile::TempDir,
    client: Arc<MusicClient>,
    native: Arc<MusicHttpClient>,
    session: Arc<AccountSession>,
    http: Arc<ScriptHttp>,
    epoch: String,
}

impl Fixture {
    fn new(responses: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let (native, http) = vinyl_http(root.path(), &[COOKIE], responses);
        let registry = super::fixture_registry(&root.path().join("host"));
        let session = Arc::new(AccountSession::new(native.clone(), Some(registry.store())));
        session
            .adopt(
                native.clone(),
                json!({"userId":71}),
                &session.token().unwrap(),
            )
            .unwrap();
        let epoch = session.token().unwrap().generation.to_string();
        let client = Arc::new(
            MusicClient::with_session(session.clone(), registry).with_official_search_experiment(),
        );
        Self {
            _root: root,
            client,
            native,
            session,
            http,
            epoch,
        }
    }

    async fn search(&self) -> Value {
        serde_json::to_value(
            self.client
                .search_results(&self.epoch, query())
                .await
                .unwrap()
                .value,
        )
        .unwrap()
    }
}

fn query() -> SearchResultQuery {
    SearchResultQuery {
        keyword: WORD.into(),
        category: SearchCategory::All,
        offset: 0,
        limit: 30,
        correct: true,
        channel: None,
    }
}

fn assignment(group: Option<Value>, v3: bool) -> Value {
    let mut rows = vec![
        json!({"expName":"PH-PC-searchHot","expGroupName":if v3 {"t1"} else {"c"}}),
        json!({"expName":"PH-PC-searchMore2","expGroupName":"t1"}),
        json!({"expName":"PH-PC-searchPageEfficiency","expGroupName":"t1"}),
    ];
    if let Some(group) = group {
        rows.push(json!({"expName":"PH-PC-search-result","expGroupName":group,
            "log":"PRIVATE_ASSIGNMENT","clientConfig":{"private":"PRIVATE_CONFIG"}}));
    }
    json!({"code":200,"data":rows})
}

fn tabs() -> Value {
    json!({"code":200,"data":{"algTabs":[{"title":"综合"}]}})
}

fn page(v3: bool) -> Value {
    let mut rows = vec![
        json!({"type":"song","baseInfo":{"id":1,"name":"有歌词","mark":1048576,"originCoverType":1,
            "newSongExt":{"songAlias":"不要展示"}},
            "extInfo":{"lyrics":{"txt":"🎵合成歌词","range":[{"first":2,"second":4}],
            "url":"https://private.invalid/PRIVATE"},"algClickableTags":[{
                "text":"歌词胜过推荐","reasonType":1,"reasonId":"hidden","resourceId":1,"clickable":true}]}}),
        json!({"type":"song","baseInfo":{"id":2,"name":"别名回退","newSongExt":{"songAlias":"旧别名"}},
            "extInfo":{"songAlias":"新<em>合成</em>别名","lyrics":{"txt":"🎵合成","range":[{"first":1,"second":4}]},
            "algClickableTags":[{"text":"别名胜过推荐","reasonType":1,"reasonId":"hidden","resourceId":2,"clickable":true}]}}),
        json!({"type":"song","baseInfo":{"id":3,"name":"无公开匹配",
            "extInfo":{"songAlias":"https://private.invalid/PRIVATE"}}}),
        json!({"type":"song","baseInfo":{"id":4,"name":"混乱范围","extInfo":{"lyrics":{
            "txt":"合成歌词","range":[{"first":1,"second":3},{"first":0,"second":2}]}}}}),
        json!({"type":"song","baseInfo":{"id":5,"name":"标签曲目","newSongExt":{"algClickableTags":[
            {"text":"旧标签","clickable":true,"reasonType":1,"reasonId":"old","resourceId":5}]},
            "extInfo":{"algClickableTags":[
                {"text":"推荐","clickable":true,"reasonType":1,"reasonId":"r-9","resourceId":5},
                {"text":"城市","clickable":true,"reasonType":5,"url":"https://example.invalid/private?chartCode=top-1&targetId=77&targetType=4"},
                {"text":"无路由","clickable":true,"reasonType":5,"url":"https://example.invalid/?chartCode=a&chartCode=b&targetId=77&targetType=4"}]}}}),
    ];
    if v3 {
        for row in &mut rows {
            let song = row["baseInfo"].take();
            row["baseInfo"] = json!({"simpleSongData":song});
        }
    }
    // Neither response self-reports an experiment grant. Even these explicit
    // claims are inert: only the separate signed assignment may enable it.
    json!({"code":200,"PH-PC-search-result":"t1","data":{"footerEnabled":true,
        "blocks":[{"blockCode":"search_block_song","resources":rows}]}})
}

fn assert_footer(view: &Value, enabled: bool) {
    let rows = view["sections"][0]["items"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0]["track"]["id"], "1");
    assert_eq!(rows[0]["explicit"], true);
    assert_eq!(rows[0]["originalRecording"], true);
    if enabled {
        assert_eq!(
            rows[0]["hint"],
            json!({"kind":"lyric","parts":[
            {"text":"🎵","highlight":false},{"text":"合成","highlight":true},{"text":"歌词","highlight":false}]})
        );
        assert_eq!(rows[1]["hint"], json!({"kind":"alias","text":"新合成别名"}));
        assert!(rows[0].get("reasons").is_none());
        assert!(rows[1].get("reasons").is_none());
        assert!(rows[2].get("hint").is_none());
        assert!(rows[3].get("hint").is_none());
        assert_eq!(
            rows[4]["reasons"],
            json!([
            {"label":"推荐","target":{"kind":"recommendation","reasonId":"r-9","songId":"5","name":"推荐"}},
            {"label":"城市","target":{"kind":"city","chartCode":"top-1","targetId":"77","targetType":"4"}},
            {"label":"无路由"}])
        );
    } else {
        for row in rows {
            assert!(row.get("hint").is_none());
            assert!(row.get("reasons").is_none());
        }
    }
    let wire = view.to_string();
    assert!(!wire.contains("PRIVATE"));
    assert!(!wire.contains("example.invalid"));
    assert!(!wire.contains("private.invalid"));
    assert!(!wire.contains("expGroupName"));
    assert!(!wire.contains("footerEnabled"));
}

fn assert_full_batch(f: &Fixture, expected_paths: &[&str]) {
    let calls = f.http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        expected_paths
    );
    for call in &calls {
        assert!(
            call.cookie
                .as_ref()
                .is_some_and(|cookie| cookie.contains(COOKIE))
        );
        if call.path == ASSIGNMENT_PATH {
            assert_eq!(call.payload["expNames"], assignment_arguments()["expNames"]);
            let header = &call.payload["header"];
            let header: Value = header
                .as_str()
                .map_or(header.clone(), |text| serde_json::from_str(text).unwrap());
            assert_eq!(header["clientSign"], "synthetic-signature");
        }
        assert!(call.login_chain.is_none());
    }
}

#[tokio::test]
async fn exact_signed_footer_group_controls_v1_and_v3_without_changing_other_badges() {
    for v3 in [false, true] {
        for group in [
            None,
            Some(json!("t1")),
            Some(json!("c")),
            Some(json!("t2")),
            Some(json!(true)),
            Some(Value::Null),
        ] {
            let enabled = group.as_ref() == Some(&json!("t1"));
            let mut responses = vec![assignment(group, v3).into()];
            if v3 {
                responses.push(tabs().into());
            }
            responses.push(page(v3).into());
            let f = Fixture::new(responses);
            let view = f.search().await;
            assert_footer(&view, enabled);
            assert_eq!(view["previewLayout"], "compact_carousel");
            if v3 {
                assert_full_batch(&f, &[ASSIGNMENT_PATH, TABS, V3]);
                assert_eq!(view["tabs"], json!(["all"]));
            } else {
                assert_full_batch(&f, &[ASSIGNMENT_PATH, LEGACY]);
                assert!(view.get("tabs").is_none());
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn unknown_assignment_keeps_search_usable_and_does_not_add_a_short_retry() {
    for failed in [json!({"code":403}), json!({"code":200,"data":{}})] {
        let f = Fixture::new([
            failed.into(),
            page(false).into(),
            page(false).into(),
            assignment(Some(json!("t1")), false).into(),
            page(false).into(),
        ]);
        assert_footer(&f.search().await, false);
        assert_footer(&f.search().await, false);
        assert_full_batch(&f, &[ASSIGNMENT_PATH, LEGACY, LEGACY]);
        tokio::time::advance(Duration::from_secs(1800)).await;
        assert_footer(&f.search().await, true);
        assert_full_batch(
            &f,
            &[ASSIGNMENT_PATH, LEGACY, LEGACY, ASSIGNMENT_PATH, LEGACY],
        );
    }
}

#[tokio::test(start_paused = true)]
async fn failed_refresh_retains_only_the_same_live_native_assignment_as_official_catch() {
    let f = Fixture::new([
        assignment(Some(json!("t1")), false).into(),
        page(false).into(),
        json!({"code":403}).into(),
        page(false).into(),
        page(false).into(),
    ]);
    assert_footer(&f.search().await, true);
    tokio::time::advance(Duration::from_secs(1800)).await;
    assert_footer(&f.search().await, true);
    assert_footer(&f.search().await, true);
    assert_full_batch(
        &f,
        &[ASSIGNMENT_PATH, LEGACY, ASSIGNMENT_PATH, LEGACY, LEGACY],
    );
}

#[tokio::test(start_paused = true)]
async fn consuming_a_prewarmed_batch_does_not_extend_its_original_refresh_deadline() {
    let f = Fixture::new([
        assignment(Some(json!("t1")), false).into(),
        page(false).into(),
        assignment(Some(json!("c")), false).into(),
        page(false).into(),
    ]);
    f.client
        .official_pc_ordinary_skip_assignment(&f.epoch)
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(1799)).await;
    assert_footer(&f.search().await, true);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_footer(&f.search().await, false);
    assert_full_batch(&f, &[ASSIGNMENT_PATH, LEGACY, ASSIGNMENT_PATH, LEGACY]);
}

#[tokio::test(start_paused = true)]
async fn another_native_consumer_can_replace_failed_refresh_without_waiting_for_search_backoff() {
    let f = Fixture::new([
        assignment(Some(json!("t1")), false).into(),
        page(false).into(),
        json!({"code":403}).into(),
        page(false).into(),
        assignment(Some(json!("c")), false).into(),
        page(false).into(),
    ]);
    assert_footer(&f.search().await, true);
    tokio::time::advance(Duration::from_secs(1800)).await;
    assert_footer(&f.search().await, true);
    f.client
        .official_pc_ordinary_skip_assignment(&f.epoch)
        .await
        .unwrap();
    assert_footer(&f.search().await, false);
    assert_full_batch(
        &f,
        &[
            ASSIGNMENT_PATH,
            LEGACY,
            ASSIGNMENT_PATH,
            LEGACY,
            ASSIGNMENT_PATH,
            LEGACY,
        ],
    );
}

#[tokio::test(start_paused = true)]
async fn expired_global_control_survives_search_catch_without_becoming_fresh_permission() {
    let mut control = assignment(Some(json!("c")), false);
    control["data"]
        .as_array_mut()
        .unwrap()
        .push(json!({"expName":"PH-PC-skipSongs2","expGroupName":"t1"}));
    let f = Fixture::new([
        assignment(Some(json!("t1")), true).into(),
        tabs().into(),
        page(true).into(),
        control.into(),
        json!({"code":403}).into(),
        page(false).into(),
        json!({"code":403}).into(),
        assignment(Some(json!("t1")), true).into(),
        tabs().into(),
        page(true).into(),
    ]);
    // t=0: search consumes a verified t1 footer and the V3 endpoint.
    let first = f.search().await;
    assert_footer(&first, true);
    assert_eq!(first["tabs"], json!(["all"]));
    tokio::time::advance(Duration::from_secs(1800)).await;
    // t=1800: a different native consumer refreshes global footer=c and
    // searchHot=c. Search deliberately does not consume that successful batch.
    assert_eq!(
        f.client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .unwrap(),
        OrdinarySkipAssignment::Enabled
    );
    tokio::time::advance(Duration::from_secs(1800)).await;
    // t=3600: the global c batch is now expired and the new read fails.
    // M147's presentation catch keeps that latest global c, not search's t1.
    let failed = f.search().await;
    assert_footer(&failed, false);
    assert!(
        failed.get("tabs").is_none(),
        "old V3 must not revive either"
    );
    // The passive catch snapshot does not extend TTL or grant an authorization
    // consumer its expired skip=t1. It must try a fresh full batch and fail.
    assert!(
        f.client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .unwrap_err()
            .is_service_code(403)
    );
    // A later genuinely fresh successful assignment still wins immediately,
    // even while search is backing off after its failed optional refresh.
    f.client
        .official_pc_ordinary_skip_assignment(&f.epoch)
        .await
        .unwrap();
    let recovered = f.search().await;
    assert_footer(&recovered, true);
    assert_eq!(recovered["tabs"], json!(["all"]));
    assert_full_batch(
        &f,
        &[
            ASSIGNMENT_PATH,
            TABS,
            V3,
            ASSIGNMENT_PATH,
            ASSIGNMENT_PATH,
            LEGACY,
            ASSIGNMENT_PATH,
            ASSIGNMENT_PATH,
            TABS,
            V3,
        ],
    );
}

#[tokio::test]
async fn presentation_catch_cannot_retain_a_global_t1_from_another_credential_revision() {
    let mut rotated = reply(page(false));
    rotated.set_cookies = vec!["MUSIC_U=synthetic-search-footer-rotated; Path=/; Secure".into()];
    let f = Fixture::new([
        assignment(Some(json!("t1")), false).into(),
        HttpResponse::Ready(Ok(rotated)),
        json!({"code":403}).into(),
        page(false).into(),
    ]);
    assert_footer(&f.search().await, false);
    assert_footer(&f.search().await, false);
    let calls = f.http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [ASSIGNMENT_PATH, LEGACY, ASSIGNMENT_PATH, LEGACY]
    );
    assert!(
        calls[2]
            .cookie
            .as_ref()
            .unwrap()
            .contains("MUSIC_U=synthetic-search-footer-rotated")
    );
    for call in [&calls[0], &calls[2]] {
        assert_eq!(call.payload["expNames"], assignment_arguments()["expNames"]);
    }
}

#[tokio::test]
async fn held_assignment_or_search_response_cannot_deliver_t1_after_credentials_rotate() {
    for hold_assignment in [false, true] {
        let (held, release) = gate();
        let initial = if hold_assignment {
            vec![held, page(false).into()]
        } else {
            vec![assignment(Some(json!("t1")), false).into(), held]
        };
        let f = Fixture::new(initial.into_iter().chain([
            assignment(Some(json!("t1")), false).into(),
            page(false).into(),
        ]));
        let client = f.client.clone();
        let epoch = f.epoch.clone();
        let pending = tokio::spawn(async move { client.search_results(&epoch, query()).await });
        f.http.wait(if hold_assignment { 1 } else { 2 }).await;
        let mut response = reply(if hold_assignment {
            assignment(Some(json!("t1")), false)
        } else {
            page(false)
        });
        response.set_cookies =
            vec!["MUSIC_U=synthetic-search-footer-rotated; Path=/; Secure".into()];
        assert!(release.send(Ok(response)).is_ok());
        let stale = serde_json::to_value(pending.await.unwrap().unwrap().value).unwrap();
        assert_footer(&stale, false);
        assert_footer(&f.search().await, true);
        let calls = f.http.calls();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].path, ASSIGNMENT_PATH);
        assert_eq!(calls[1].path, LEGACY);
        assert_eq!(calls[2].path, ASSIGNMENT_PATH);
        assert_eq!(calls[3].path, LEGACY);
        assert!(
            calls[2]
                .cookie
                .as_ref()
                .unwrap()
                .contains("MUSIC_U=synthetic-search-footer-rotated")
        );
        assert_eq!(
            calls[0].payload["expNames"],
            assignment_arguments()["expNames"]
        );
        assert_eq!(
            calls[2].payload["expNames"],
            assignment_arguments()["expNames"]
        );
    }
}

#[tokio::test]
async fn complex_request_reselects_current_cohort_after_tabs_without_caching_control_tabs() {
    let treatment = || assignment(Some(json!("t1")), true);
    // The actual HTTP adapter serializes this profile's requests. Model a
    // tab response's own credential rotation rather than a second wire
    // request that cannot overtake the held response.
    let mut rotated = reply(json!({"code":200,"data":{"algTabs":[
        {"title":"综合"},{"title":"歌单"}
    ]}}));
    rotated.set_cookies =
        vec!["MUSIC_U=synthetic-search-footer-new-tab-scope; Path=/; Secure".into()];
    let f = Fixture::new([
        treatment().into(),
        json!({"code":200,"data":{"algTabs":[{"title":"综合"},{"title":"单曲"}]}}).into(),
        page(true).into(),
        HttpResponse::Ready(Ok(rotated)),
        page(false).into(),
        treatment().into(),
        json!({"code":403}).into(),
        page(true).into(),
    ]);
    let first = f.search().await;
    assert_eq!(first["tabs"], json!(["all", "songs"]));
    let second = f
        .client
        .search_results(&f.epoch, query())
        .await
        .unwrap()
        .value;
    assert!(!f.client.pc_presentation().unwrap().search_footer);
    assert!(second.tabs.is_none(), "control uses static tabs");
    assert!(!second.sections.is_empty());
    assert_eq!(http_paths(&f), [ASSIGNMENT_PATH, TABS, V3, TABS, LEGACY]);

    f.client.refresh_pc_presentation().await.unwrap();
    let restored = f.search().await;
    assert_eq!(
        restored["tabs"], first["tabs"],
        "last accepted V3 tabs survive; the dropped control result was never cached"
    );
    assert_eq!(
        http_paths(&f),
        [
            ASSIGNMENT_PATH,
            TABS,
            V3,
            TABS,
            LEGACY,
            ASSIGNMENT_PATH,
            TABS,
            V3
        ]
    );
}

fn http_paths(f: &Fixture) -> Vec<String> {
    f.http.calls().into_iter().map(|call| call.path).collect()
}

#[tokio::test]
async fn account_change_cancels_held_assignment_before_any_old_account_search() {
    let (held, release) = gate();
    let f = Fixture::new([
        held,
        assignment(Some(json!("c")), false).into(),
        page(false).into(),
    ]);
    let client = f.client.clone();
    let epoch = f.epoch.clone();
    let pending = tokio::spawn(async move { client.search_results(&epoch, query()).await });
    f.http.wait(1).await;
    f.session
        .adopt(
            f.native.clone(),
            json!({"userId":72}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        release
            .send(Ok(reply(assignment(Some(json!("t1")), false))))
            .is_err()
    );
    let epoch = f.client.account().unwrap().epoch;
    let view = serde_json::to_value(
        f.client
            .search_results(&epoch, query())
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_footer(&view, false);
    assert_full_batch(&f, &[ASSIGNMENT_PATH, ASSIGNMENT_PATH, LEGACY]);
}

#[tokio::test]
async fn separate_lyric_category_does_not_request_or_require_the_comprehensive_footer_group() {
    let f = Fixture::new([json!({"code":200,"data":{"resources":[{
        "song":{"id":9,"name":"歌词分类"},"lyrics":"合成第一行\n合成第二行"
    }],"totalCount":1}})
    .into()]);
    let view = serde_json::to_value(
        f.client
            .search_results(
                &f.epoch,
                SearchResultQuery {
                    category: SearchCategory::Lyrics,
                    ..query()
                },
            )
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_eq!(
        view["sections"][0]["items"][0]["excerpt"],
        json!(["合成第一行", "合成第二行"])
    );
    assert_full_batch(&f, &["/api/search/resource/lyric"]);
}
