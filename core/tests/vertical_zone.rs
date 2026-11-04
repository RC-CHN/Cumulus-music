use cumulus_core::{
    Result,
    catalog::vertical_zone::{ZoneBlockKind, ZoneEntry, ZoneLayout, ZoneTarget, read},
    protocol::{
        api::{Api, ApiFuture},
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};

type Exchange = (&'static str, Value, Value);
struct Fixture(Mutex<VecDeque<Exchange>>);
impl Fixture {
    fn new(expected: Vec<Exchange>) -> Self {
        Self(Mutex::new(expected.into()))
    }
    fn finished(&self) {
        assert!(self.0.lock().unwrap().is_empty());
    }
}
impl Api for Fixture {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (expected_path, expected_payload, response) = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request");
            assert_eq!(
                (path, payload, cache),
                (expected_path, expected_payload, false)
            );
            Ok(response)
        })
    }
}

fn groups() -> Result<PcExperimentGroups> {
    PcExperimentGroups::refreshed(
        &json!({"data":[{"expName":"PH-PC-zhuanquTab","expGroupName":"t1"}]}),
    )
}

#[tokio::test]
async fn classic_uses_the_default_endpoint_and_projects_official_modules_in_order() {
    let contents = json!([
        {"componentName":"RecommendTabCarousel","children":[
            {"targetType":1000,"targetId":"10","title":"站内歌单","imgUrl":"http://p1.music.126.net/x.jpg?ticket=synthetic"},
            {"targetType":3000,"targetId":"11","url":"https://example.invalid/?ticket=synthetic"},
            {"targetType":1004,"targetId":"12","title":"未知视频"}]},
        {"componentName":"RecommendHorizontal","childrens":[
            {"resourceType":"album","resourceId":"20"},
            {"resourceType":"fm","resourceId":"21","hideAfterClosePersonalized":true},
            {"resourceType":"playlist","resourceId":"22"},
            {"resourceType":"playlist","resourceId":"23"},
            {"resourceType":"playlist","resourceId":"24"},
            {"resourceType":"playlist","resourceId":"25"}]},
        {"componentName":"MusiclibraryRank","children":[
            {"targetType":"PLAYLIST","nameShowStyle":"doubleRow","id":"30",
             "trackRankList":[{"songName":"原名称","artistName":"原歌手"}],
             "tracks":[{"first":"展示名"}]},
            {"targetType":"PLAYLIST","nameShowStyle":"unknown","id":"31"}]},
        {"componentName":"RecommendGrid","title":"个性化","hideAfterClosePersonalized":true,
         "children":[{"resourceType":"playlist","resourceId":"40","title":"私有推荐"},
                     {"resourceType":"album","resourceId":"41"}]},
        {"componentName":"Other","children":[{"resourceId":"50"}]}
    ]);
    let api = Fixture::new(vec![
        (
            "/api/zone/get",
            json!({"pageCode":"jazz"}),
            json!({"code":200,"data":[{"tabKey":"default","blocks":contents}]}),
        ),
        (
            "/api/zone/get",
            json!({"pageCode":"jazz"}),
            json!({"code":200,"data":[{"tabKey":"default","blocks":contents}]}),
        ),
    ]);
    let page = read(&api, None, true, "jazz", ZoneEntry::Tab("songs"))
        .await
        .unwrap();
    assert_eq!(page.layout, ZoneLayout::Classic);
    assert!(page.tabs.is_empty());
    assert_eq!(
        page.blocks
            .iter()
            .map(|block| &block.kind)
            .collect::<Vec<_>>(),
        [
            &ZoneBlockKind::Carousel,
            &ZoneBlockKind::Horizontal,
            &ZoneBlockKind::Ranking,
            &ZoneBlockKind::Grid
        ]
    );
    assert_eq!(page.blocks[0].cards.len(), 1);
    assert_eq!(
        page.blocks[0].cards[0].cover.as_deref(),
        Some("https://p1.music.126.net/x.jpg")
    );
    assert_eq!(
        page.blocks[1].cards[1].target,
        ZoneTarget::Fm {
            id: "21".into(),
            personalized: true
        }
    );
    assert_eq!(page.blocks[2].cards[0].previews[0].title, "展示名");
    assert_eq!(
        page.blocks[2].cards[0].previews[0].artist.as_deref(),
        Some("原歌手")
    );
    assert_eq!(page.blocks[3].cards.len(), 1);
    let off = read(&api, None, false, "jazz", ZoneEntry::Tab("default"))
        .await
        .unwrap();
    // The unmarked rail drops below the original six visible items; a marked
    // module retains only the disabled privacy affordance, never card data.
    assert_eq!(off.blocks.len(), 3);
    assert_eq!(off.blocks[2].kind, ZoneBlockKind::Grid);
    assert!(off.blocks[2].requires_recommendation && off.blocks[2].cards.is_empty());
    assert!(!serde_json::to_string(&off).unwrap().contains("私有推荐"));
    api.finished();
}

#[tokio::test]
async fn named_search_retries_only_an_exact_label_and_mismatch_falls_back() {
    let plan = vec![
        (
            "/api/zone/tab/get",
            json!({"pageCode":"jazz","tabKey":"default"}),
            json!({
            "code":200,"data":{"tabMetas":[{"tabKey":"swing","tabKeyName":" 摇摆"}],
            "currentTabContent":{"tabKey":"default","tabKeyName":"推荐","blocks":[]}}}),
        ),
        (
            "/api/zone/tab/get",
            json!({"pageCode":"jazz","tabKey":"swing"}),
            json!({
            "code":200,"data":{"tabMetas":[{"tabKey":"swing","tabKeyName":" 摇摆"}],
            "currentTabContent":{"tabKey":"swing","tabKeyName":" 摇摆","blocks":[]}}}),
        ),
        (
            "/api/zone/tab/get",
            json!({"pageCode":"jazz","tabKey":"later"}),
            json!({
            "code":200,"data":{"currentTabContent":{"tabKey":"default","blocks":[]}}}),
        ),
        (
            "/api/zone/get",
            json!({"pageCode":"jazz"}),
            json!({"code":200,"data":[]}),
        ),
        (
            "/api/zone/tab/get",
            json!({"pageCode":"jazz","tabKey":"later"}),
            json!({"code":301}),
        ),
    ];
    let api = Fixture::new(plan);
    let groups = groups().unwrap();
    let by_name = read(
        &api,
        Some(&groups),
        true,
        "jazz",
        ZoneEntry::SearchTabName(" 摇摆"),
    )
    .await
    .unwrap();
    assert_eq!(by_name.layout, ZoneLayout::Tabs);
    assert_eq!(by_name.active_tab.unwrap().key, "swing");
    let fallback = read(&api, Some(&groups), true, "jazz", ZoneEntry::Tab("later"))
        .await
        .unwrap();
    assert_eq!(fallback.layout, ZoneLayout::Fallback);
    assert!(fallback.tabs.is_empty() && fallback.active_tab.is_none());
    let denied = read(&api, Some(&groups), true, "jazz", ZoneEntry::Tab("later"))
        .await
        .unwrap_err();
    assert!(denied.is_service_code(301));
    api.finished();
}
