mod support;
use cumulus_core::{
    Error,
    account::credentials::Credentials,
    catalog::{
        search::results::SearchHotZoneSource,
        vertical_zone::{ZoneEntry, ZoneLayout, ZoneTab},
    },
    client::{
        MusicClient,
        dto::{SearchCategory, SearchHotTarget, SearchResultQuery, SearchResultView},
        pc_experiments::OrdinarySkipAssignment,
    },
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{ApiOrigin, ClientProfile, OFFICIAL_WINDOWS_USER_AGENT},
        mconfig::{ConfigBackend, ConfigUrl, PcConfig},
        pc_experiments::{ASSIGNMENT_PATH, assignment_arguments},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::http::{HttpResponse, ScriptHttp, gate, reply};

struct ClosedConfig;
impl ConfigBackend for ClosedConfig {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("fixture must not fetch remote configuration") })
    }
}
impl TokenProvider for ClosedConfig {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async { panic!("assignment is not protected in the fixture") })
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    client: Arc<MusicClient>,
    http: Arc<ScriptHttp>,
    epoch: String,
}
impl Fixture {
    async fn new(responses: impl IntoIterator<Item = HttpResponse>, native: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let account = root.path().join("profile");
        let store = Arc::new(StateStore::open(&account).unwrap());
        let credentials = Credentials::open(store, ApiOrigin::Pc).unwrap();
        let target = ApiOrigin::Pc.url();
        let original = credentials.request_credentials(&target).unwrap();
        credentials
            .record_response(
                &target,
                &["MUSIC_U=synthetic-pc-experiment; Path=/; Secure".into()],
                None,
                &original.revision,
            )
            .unwrap();
        let http = Arc::new(ScriptHttp::new(
            [json!({"code":200,"profile":{"userId":71,"nickname":"合成账号"}}).into()]
                .into_iter()
                .chain(responses),
        ));
        let context = native.then(|| {
            let service = PcConfig::new(
                Arc::new(StateStore::open(root.path().join("config")).unwrap()),
                Arc::new(ClosedConfig),
            )
            .unwrap();
            Arc::new(
                RequestContext::from_pc(service, "3.1.42.205594", Arc::new(ClosedConfig)).unwrap(),
            )
        });
        let profile = if native {
            ClientProfile {
                user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
                os_version: "Microsoft Windows 10".into(),
                client_sign: Some("synthetic-signature".into()),
            }
        } else {
            ClientProfile::default()
        };
        let client = Arc::new(
            MusicClient::open_with_request_context(
                &root.path().join("host"),
                Some(&account),
                profile,
                Some(http.clone()),
                context,
            )
            .unwrap()
            .with_official_search_experiment(),
        );
        let epoch = client.restore_account().await.unwrap().epoch;
        Self {
            _root: root,
            client,
            http,
            epoch,
        }
    }
}

fn enabled() -> Value {
    json!({"code":200,"data":[{"expName":"PH-PC-skipSongs2","expGroupName":"t1"}]})
}

#[tokio::test(start_paused = true)]
async fn signed_native_pc_read_keeps_its_scope_and_expires_at_exact_refresh_boundary() {
    let f = Fixture::new(
        [enabled().into(), json!({"code":200,"data":[]}).into()],
        true,
    )
    .await;
    let client = &f.client;
    assert_eq!(
        client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .unwrap(),
        OrdinarySkipAssignment::Enabled
    );
    tokio::time::advance(Duration::from_secs(1799)).await;
    assert_eq!(
        client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .unwrap(),
        OrdinarySkipAssignment::Enabled
    );
    assert_eq!(f.http.calls().len(), 2);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(
        client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .unwrap(),
        OrdinarySkipAssignment::Control
    );
    let calls = f.http.calls();
    assert_eq!(calls.len(), 3);
    for call in &calls[1..] {
        assert_eq!(call.path, ASSIGNMENT_PATH);
        assert_eq!(call.payload["expNames"], assignment_arguments()["expNames"]);
        assert!(
            call.cookie
                .as_ref()
                .is_some_and(|cookie| cookie.contains("MUSIC_U=synthetic-pc-experiment"))
        );
        assert!(call.login_chain.is_none());
    }
}

#[tokio::test(start_paused = true)]
async fn failures_after_expiry_do_not_grant_an_old_group_or_send_a_fallback() {
    let f = Fixture::new([enabled().into(), json!({"code":403}).into()], true).await;
    assert_eq!(
        f.client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .unwrap(),
        OrdinarySkipAssignment::Enabled
    );
    tokio::time::advance(Duration::from_secs(1800)).await;
    assert!(
        f.client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .is_err()
    );
    assert_eq!(f.http.calls().len(), 3);
    let unsigned = Fixture::new([], false).await;
    assert!(
        unsigned
            .client
            .official_pc_ordinary_skip_assignment(&unsigned.epoch)
            .await
            .is_err()
    );
    assert_eq!(unsigned.http.calls().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn v3_hotspot_requires_current_signed_zone_registry_and_reuses_it_for_tab_navigation() {
    const SEARCH_TABS: &str = "/api/search/pc/result/tab";
    const SEARCH_V3: &str = "/api/search/pc/complex/page/v3";
    const ZONE_MENU: &str = "/api/secondary/tab/list";
    const ZONE_TAB: &str = "/api/zone/tab/get";
    const PAGE: &str = "Rock_12";
    const DISPLAY: &str = "独立音乐";
    const KEY: &str = "indie_37";
    const SHORT_GROUPS: &str = "PH-PC-searchHot,PH-PC-searchMore2,PH-PC-searchPageEfficiency";
    const WORD: &str = "合成专区热点";

    let tabs = || json!({"code":200,"data":{"algTabs":[{"title":"综合"},{"title":"单曲"}]}});
    let v3 = |action: &str| {
        json!({"code":200,"data":{"blocks":[
            {"blockCode":"search_block_hot_result_hot_explain","resources":[{
                "baseInfo":{"hotExplainModuleData":{
                    "title":"合成热点","text":"说明","textAction":action,"canClick":true,"urlText":"查看详情"
                }},
                "relatedResources":[{"resourceType":"album","baseInfo":{"albumData":{"id":99}}}]
            }]},
            {"blockCode":"search_block_song","resources":[{"baseInfo":{"simpleSongData":{"id":77,"name":"独立歌曲"}}}]}
        ]}})
    };
    let zone_action = |page: &str| {
        v3(&format!(
            "orpheus://rnpage/private-id?component=cube-renderer-rn&page={page}&tabKey=%E7%8B%AC%E7%AB%8B%E9%9F%B3%E4%B9%90&from=private-source&url=https%3A%2F%2Fprivate.invalid"
        ))
    };
    let zone_groups = |group| {
        json!({"code":200,"data":[
            {"expName":"PH-PC-zhuanqu","expGroupName":group},
            {"expName":"PH-PC-zhuanquTab","expGroupName":"t1"}
        ]})
    };
    let menu = json!({"code":200,"data":{"hints":[
        {"position":{"code":"music_sidebar"},"data":{"extra":{"nowChannelItems":[
            {"code":"ghost","title":"非专区入口","url":"orpheus://rnpage?page=Ghost"}
        ]}}},
        {"position":{"code":"music_top_tab_full"},"data":{"extra":{"nowChannelItems":[
            {"code":"rock","title":"合成专区","url":"orpheus://rnpage?page=Rock_12&token=private-menu"}
        ]}}}
    ]}});
    let zone_tab = |key, name| {
        json!({"code":200,"data":{
            "tabMetas":[{"tabKey":"default","tabKeyName":"全部"},{"tabKey":KEY,"tabKeyName":DISPLAY}],
            "currentTabContent":{"tabKey":key,"tabKeyName":name,"blocks":[]}
        }})
    };
    let f = Fixture::new(
        [
            json!({"code":200,"data":[{"expName":"PH-PC-searchHot","expGroupName":"t1"}]}).into(),
            tabs().into(),
            v3("orpheus://album/33").into(),
            tabs().into(),
            v3("orpheus://rnpage?component=unknown&page=Rock_12").into(),
            tabs().into(),
            zone_action(PAGE).into(),
            zone_groups("t1").into(),
            menu.into(),
            json!({"code":200,"setting":{"personalServiceSetting":0}}).into(),
            zone_tab("default", "全部").into(),
            zone_tab(KEY, DISPLAY).into(),
            tabs().into(),
            zone_action("Ghost").into(),
            tabs().into(),
            zone_action(PAGE).into(),
            tabs().into(),
            zone_action(PAGE).into(),
            json!({"code":403}).into(),
            tabs().into(),
            zone_action(PAGE).into(),
            zone_groups("c").into(),
        ],
        true,
    )
    .await;

    let query = || SearchResultQuery {
        keyword: WORD.into(),
        category: SearchCategory::All,
        offset: 0,
        limit: 30,
        correct: true,
        channel: None,
    };
    let search = || async {
        let result = f.client.search_results(&f.epoch, query()).await.unwrap();
        assert_eq!(result.epoch, f.epoch);
        assert_eq!(result.value.sections[0].items.len(), 1);
        result.value
    };
    let search_tab = || (SEARCH_TABS, json!({"keyword":WORD}));
    let search_v3 = || {
        (
            SEARCH_V3,
            json!({"keyword":WORD,"scene":"normal","needCorrect":"true","channel":"suggest","bizQueryInfo":""}),
        )
    };
    let mut cursor = f.http.calls().len();
    assert_eq!(cursor, 1, "account restore only");
    let mut check = |expected: &[(&str, Value)]| {
        let calls = f.http.calls();
        assert_eq!(
            calls.len(),
            cursor + expected.len(),
            "request order or count differs"
        );
        for (call, (path, payload)) in calls[cursor..].iter().zip(expected) {
            let mut actual = call.payload.clone();
            let fields = actual.as_object_mut().unwrap();
            let header = fields.remove("header").unwrap();
            fields.remove("e_r");
            assert_eq!((call.path.as_str(), &actual), (*path, payload));
            assert!(
                call.cookie
                    .as_ref()
                    .is_some_and(|cookie| cookie.contains("MUSIC_U=synthetic-pc-experiment"))
            );
            assert!(call.login_chain.is_none());
            if *path == ASSIGNMENT_PATH {
                let header = header
                    .as_str()
                    .map_or(header.clone(), |wire| serde_json::from_str(wire).unwrap());
                assert_eq!(header["clientSign"], "synthetic-signature");
            }
        }
        cursor = calls.len();
    };

    let ordinary = search().await;
    assert!(
        matches!(ordinary.hot_explanation.unwrap().action.unwrap().target, SearchHotTarget::Album { id } if id == "33")
    );
    check(&[
        (ASSIGNMENT_PATH, json!({"expNames":SHORT_GROUPS})),
        search_tab(),
        search_v3(),
    ]);
    assert!(search().await.hot_explanation.is_none());
    check(&[search_tab(), search_v3()]);

    let hotspot = search().await;
    check(&[
        search_tab(),
        search_v3(),
        (ASSIGNMENT_PATH, assignment_arguments()),
        (ZONE_MENU, json!({})),
    ]);
    let target = |view: SearchResultView| {
        let safe = serde_json::to_string(&view).unwrap();
        assert!(
            !safe.contains("private-")
                && !safe.contains("private.invalid")
                && !safe.contains("orpheus://")
        );
        let explanation = view.hot_explanation.unwrap();
        assert!(explanation.play_target.is_none());
        let SearchHotTarget::VerticalZone { zone } = explanation.action.unwrap().target else {
            panic!("expected an authorized vertical-zone target");
        };
        assert_eq!(zone.page_code, PAGE);
        assert_eq!(zone.tab_key_name.as_deref(), Some(DISPLAY));
        assert_eq!(zone.from, SearchHotZoneSource::SearchHotExplain);
        zone
    };
    let zone = target(hotspot);
    let page = f
        .client
        .vertical_zone_page(
            &f.epoch,
            &zone.page_code,
            ZoneEntry::SearchTabName(zone.tab_key_name.as_deref().unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(page.epoch, f.epoch);
    assert_eq!(page.value.layout, ZoneLayout::Tabs);
    assert_eq!(
        page.value.active_tab,
        Some(ZoneTab {
            key: KEY.into(),
            name: DISPLAY.into()
        })
    );
    assert!(!page.value.view_id.is_empty());
    check(&[
        ("/api/user/setting", json!({})),
        (ZONE_TAB, json!({"pageCode":PAGE,"tabKey":"default"})),
        (ZONE_TAB, json!({"pageCode":PAGE,"tabKey":KEY})),
    ]);

    assert!(search().await.hot_explanation.is_none());
    assert!(matches!(
        f.client
            .vertical_zone_page(&f.epoch, "Ghost", ZoneEntry::Tab(""))
            .await,
        Err(Error::InvalidInput("unregistered vertical zone page"))
    ));
    check(&[search_tab(), search_v3()]);
    tokio::time::advance(Duration::from_secs(1799)).await;
    target(search().await);
    check(&[search_tab(), search_v3()]);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(search().await.hot_explanation.is_none());
    check(&[
        search_tab(),
        search_v3(),
        (ASSIGNMENT_PATH, assignment_arguments()),
    ]);
    assert!(search().await.hot_explanation.is_none());
    assert!(matches!(
        f.client
            .vertical_zone_page(&f.epoch, PAGE, ZoneEntry::Tab(""))
            .await,
        Err(Error::State(
            "vertical zones are unavailable for this account"
        ))
    ));
    check(&[
        search_tab(),
        search_v3(),
        (ASSIGNMENT_PATH, assignment_arguments()),
    ]);
}

#[tokio::test]
async fn logout_cancels_held_response_without_carrying_group_to_new_epoch() {
    let (held, answer) = gate();
    let f = Fixture::new([held], true).await;
    let client = f.client.clone();
    let epoch = f.epoch.clone();
    let pending =
        tokio::spawn(async move { client.official_pc_ordinary_skip_assignment(&epoch).await });
    f.http.wait(2).await;
    f.client.logout_account(&f.epoch, false).await.unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert!(answer.send(Ok(reply(enabled()))).is_err());
    let new_epoch = f.client.account().unwrap().epoch;
    assert_ne!(new_epoch, f.epoch);
    assert!(
        f.client
            .official_pc_ordinary_skip_assignment(&new_epoch)
            .await
            .is_err()
    );
    assert_eq!(f.http.calls().len(), 2);
}
