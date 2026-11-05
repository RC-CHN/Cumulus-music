use super::*;
use crate::{
    account::{AccountSession, profiles::ProfileRegistry},
    catalog::vertical_zone::ZoneTab,
    protocol::{api::ApiFuture, http::ApiOrigin},
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

struct Sealed;
impl Api for Sealed {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        unreachable!("tab-cache proof must not touch a transport")
    }
}

fn owner(generation: u64) -> SessionToken {
    SessionToken {
        generation,
        uid: Some("10".into()),
    }
}

fn page(key: &str, name: &str) -> VerticalZonePage {
    let tab = ZoneTab {
        key: key.into(),
        name: name.into(),
    };
    VerticalZonePage {
        view_id: String::new(),
        card_size: crate::catalog::vertical_zone::ZoneCardSize::Default,
        layout: ZoneLayout::Tabs,
        tabs: vec![tab.clone()],
        active_tab: Some(tab),
        blocks: Vec::new(),
    }
}

#[test]
fn reused_tabs_fence_account_generation_transport_privacy_and_route_order() {
    let host = ZoneHost::default();
    let api: Arc<dyn Api> = Arc::new(Sealed);
    let alternative: Arc<dyn Api> = Arc::new(Sealed);
    let first = host.enter(&owner(1), &api, "initial").unwrap();
    let later = host.enter(&owner(1), &api, "initial").unwrap();
    let item = page("default", " 热门");
    assert!(matches!(
        host.finish(first, "jazz", true, &item),
        Err(Error::StaleResponse)
    ));
    host.finish(later, "jazz", true, &item).unwrap();
    assert!(
        host.load(later, "jazz", &ZoneEntry::SearchTabName("热门"), true, true)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        host.load(
            later,
            "jazz",
            &ZoneEntry::SearchTabName(" 热门"),
            true,
            true
        )
        .unwrap(),
        Some(item.clone())
    );
    assert!(
        host.load(later, "jazz", &ZoneEntry::Tab(""), false, true)
            .unwrap()
            .is_none()
    );
    let replaced = host.enter(&owner(1), &alternative, "initial").unwrap();
    assert!(
        host.load(replaced, "jazz", &ZoneEntry::Tab(""), true, true)
            .unwrap()
            .is_none()
    );
    host.finish(replaced, "jazz", true, &item).unwrap();
    let same_uid_new_generation = host.enter(&owner(2), &alternative, "initial").unwrap();
    assert!(
        host.load(
            same_uid_new_generation,
            "jazz",
            &ZoneEntry::Tab(""),
            true,
            true
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn eight_tabs_refresh_recency_and_classic_disables_cache() {
    let host = ZoneHost::default();
    let api: Arc<dyn Api> = Arc::new(Sealed);
    for key in 0..8 {
        let sequence = host.enter(&owner(1), &api, "initial").unwrap();
        host.finish(sequence, "jazz", true, &page(&key.to_string(), "标签"))
            .unwrap();
    }
    let sequence = host.enter(&owner(1), &api, "initial").unwrap();
    assert!(
        host.load(sequence, "jazz", &ZoneEntry::Tab("0"), true, true)
            .unwrap()
            .is_some()
    );
    host.finish(sequence, "jazz", true, &page("new", "新标签"))
        .unwrap();
    assert!(
        host.load(sequence, "jazz", &ZoneEntry::Tab("1"), true, true)
            .unwrap()
            .is_none()
    );
    assert!(
        host.load(sequence, "jazz", &ZoneEntry::Tab("0"), true, true)
            .unwrap()
            .is_some()
    );
    assert!(
        host.load(sequence, "jazz", &ZoneEntry::Tab("0"), true, false)
            .unwrap()
            .is_none()
    );
    assert!(
        host.load(sequence, "jazz", &ZoneEntry::Tab("0"), true, true)
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_zone_playlist_receipt_preserves_the_clicked_occurrence_and_cannot_cross_views() {
    use crate::catalog::vertical_zone::{ZoneBlock, ZoneBlockKind, ZoneCard, ZoneTarget};
    let host = ZoneHost::default();
    let api: Arc<dyn Api> = Arc::new(Sealed);
    let current = owner(1);
    let sequence = host.enter(&current, &api, "revision").unwrap();
    let playlist = |personalized| ZoneCard {
        card_key: String::new(),
        title: None,
        cover: None,
        target: ZoneTarget::Playlist {
            id: "17".into(),
            personalized,
        },
        can_play: true,
        mgc_special: personalized,
        previews: vec![],
    };
    let mut current_page = page("default", "推荐");
    current_page.blocks = vec![
        ZoneBlock {
            kind: ZoneBlockKind::Grid,
            title: None,
            requires_recommendation: false,
            cards: vec![playlist(false)],
        },
        ZoneBlock {
            kind: ZoneBlockKind::Horizontal,
            title: None,
            requires_recommendation: false,
            cards: vec![playlist(true)],
        },
    ];
    host.publish(sequence, "jazz", &mut current_page).unwrap();
    let view = &current_page.view_id;
    let binding = Binding::new(&current, &api, "revision");
    let other = Binding::new(&owner(2), &api, "revision");
    assert_eq!(
        host.playlist_context(&binding, view, "0:0", "17", |_| true)
            .unwrap(),
        (false, false)
    );
    assert_eq!(
        host.playlist_context(&binding, view, "1:0", "17", |_| true)
            .unwrap(),
        (true, true)
    );
    assert!(
        host.playlist_context(&binding, view, "1:0", "18", |_| true)
            .is_err()
    );
    assert!(
        host.playlist_context(&other, view, "1:0", "17", |_| true)
            .is_err()
    );
    host.enter(&current, &api, "revision").unwrap();
    assert!(matches!(
        host.playlist_context(&binding, view, "1:0", "17", |_| true),
        Err(Error::StaleResponse)
    ));
}

const MENU: &str = "/api/secondary/tab/list";
const PRIVACY: &str = "/api/user/setting";
const CLASSIC: &str = "/api/zone/get";
const TABS: &str = "/api/zone/tab/get";

struct Wire {
    revision: Mutex<String>,
    assigned: AtomicBool,
    tabbed: bool,
    calls: Mutex<Vec<(String, Value)>>,
    menu: Mutex<Value>,
    hold_menu: AtomicBool,
    menu_entered: Notify,
    menu_release: Notify,
    hold_page: AtomicBool,
    page_entered: Notify,
    page_release: Notify,
}

impl Wire {
    fn new(assigned: bool, tabbed: bool, menu: Value) -> Self {
        Self {
            revision: Mutex::new("pc-a".into()),
            assigned: AtomicBool::new(assigned),
            tabbed,
            calls: Mutex::default(),
            menu: Mutex::new(menu),
            hold_menu: AtomicBool::new(false),
            menu_entered: Notify::new(),
            menu_release: Notify::new(),
            hold_page: AtomicBool::new(false),
            page_entered: Notify::new(),
            page_release: Notify::new(),
        }
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }

    fn push(&self, name: &str, args: Value) {
        self.calls.lock().unwrap().push((name.into(), args));
    }
}

impl Api for Wire {
    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        Ok(Some(self.revision.lock().unwrap().clone()))
    }

    fn verified_pc_experiment_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, PcExperimentGroups> {
        self.push("signed_assignment", json!(revision));
        Box::pin(async move {
            PcExperimentGroups::refreshed(&json!({"data":[
                {"expName":"PH-PC-zhuanqu","expGroupName":if self.assigned.load(Ordering::Acquire) { "t1" } else { "c" }},
                {"expName":"PH-PC-zhuanquTab","expGroupName":if self.tabbed { "t1" } else { "c" }}
            ]}))
        })
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        assert!(!cache);
        self.push(path, args.clone());
        Box::pin(async move {
            Ok(match path {
                MENU => {
                    if self.hold_menu.swap(false, Ordering::AcqRel) {
                        self.menu_entered.notify_one();
                        self.menu_release.notified().await;
                    }
                    self.menu.lock().unwrap().clone()
                }
                PRIVACY => json!({"code":200,"setting":{"personalServiceSetting":0}}),
                CLASSIC => {
                    if self.hold_page.swap(false, Ordering::AcqRel) {
                        self.page_entered.notify_one();
                        self.page_release.notified().await;
                    }
                    json!({"code":200,"data":[{"tabKey":"default","tabKeyName":"全部","blocks":[]}]})
                }
                TABS => {
                    let tab = if args["tabKey"] == "latin" {
                        json!({"tabKey":"latin","tabKeyName":"拉丁","blocks":[]})
                    } else {
                        json!({"tabKey":"default","tabKeyName":"全部","blocks":[]})
                    };
                    json!({"code":200,"data":{"currentTabContent":tab,"tabMetas":[
                        {"tabKey":"default","tabKeyName":"全部"},{"tabKey":"latin","tabKeyName":"拉丁"}
                    ]}})
                }
                _ => panic!("unexpected vertical zone fixture request: {path}"),
            })
        })
    }
}

struct ClientFixture {
    _root: tempfile::TempDir,
    session: Arc<AccountSession>,
    api: Arc<Wire>,
    client: Arc<MusicClient>,
}

impl ClientFixture {
    fn new(assigned: bool, tabbed: bool, menu: Value) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let api = Arc::new(Wire::new(assigned, tabbed, menu));
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        let client = Arc::new(MusicClient::with_session(session.clone(), registry));
        Self {
            _root: root,
            session,
            api,
            client,
        }
    }

    fn adopt(&self, uid: u64) -> String {
        self.session
            .adopt(
                self.api.clone(),
                json!({"userId":uid}),
                &self.session.token().unwrap(),
            )
            .unwrap();
        self.session.token().unwrap().generation.to_string()
    }
}

fn menu(rows: Vec<Value>) -> Value {
    json!({"code":200,"data":{"hints":[
        {"position":{"code":"music_sidebar"},"data":{"extra":{"nowChannelItems":[tag("side","side","side")]}}},
        {"position":{"code":"music_top_tab_full"},"data":{"extra":{"nowChannelItems":rows}}},
        {"position":{"code":"music_top_tab_full"},"data":{"extra":{"nowChannelItems":[tag("second","second","second")]}}}
    ]}})
}

fn tag(key: &str, name: &str, page: &str) -> Value {
    json!({"code":key,"title":name,"url":format!("orpheus://rnpage?page={page}&x=PRIVATE_URL"),"s_ctrp":"PRIVATE_LOG"})
}

#[tokio::test]
async fn menu_preserves_the_first_five_and_ordered_more_from_only_the_assigned_channel() {
    let mut rows: Vec<_> = ["feature", "playlist", "chart", "singer", "vip", "origin"]
        .into_iter()
        .map(|key| tag(key, key, key))
        .collect();
    rows.extend((1..=7).map(|index| {
        tag(
            &format!("key-{index}"),
            &format!(" {index} 专区"),
            &format!("PAGE_{index}"),
        )
    }));
    rows.extend([
        tag("key-1", "重复键", "duplicate-key"),
        tag("another", "重复页", "PAGE_2"),
        tag("broken", "路径穿越", "../secret"),
        tag("ambiguous", "多页", "x&page=y"),
    ]);
    let f = ClientFixture::new(true, false, menu(rows));
    let epoch = f.adopt(51);
    let view = f.client.vertical_zone_menu(&epoch).await.unwrap();
    assert_eq!(view.epoch, epoch);
    let value = serde_json::to_value(view.value).unwrap();
    assert_eq!(value["available"], true);
    assert_eq!(value["defaultZoneTags"].as_array().unwrap().len(), 5);
    assert_eq!(
        value["moreZoneTags"],
        json!([
            {"key":"key-6","name":" 6 专区","pageCode":"PAGE_6"},
            {"key":"key-7","name":" 7 专区","pageCode":"PAGE_7"}
        ])
    );
    assert_eq!(
        value["defaultZoneTags"][0],
        json!({"key":"key-1","name":" 1 专区","pageCode":"PAGE_1"})
    );
    assert!(!value.to_string().contains("PRIVATE_"));
    assert_eq!(
        f.api.calls(),
        vec![
            ("signed_assignment".into(), json!("pc-a")),
            (MENU.into(), json!({}))
        ]
    );
}

#[tokio::test]
async fn guest_control_and_non_registry_pages_never_reach_content() {
    let f = ClientFixture::new(false, true, menu(vec![tag("j", "爵士", "jazz")]));
    let guest = f.session.token().unwrap().generation.to_string();
    assert!(
        !f.client
            .vertical_zone_menu(&guest)
            .await
            .unwrap()
            .value
            .available
    );
    assert!(
        f.client
            .vertical_zone_page(&guest, "jazz", ZoneEntry::Tab(""))
            .await
            .is_err()
    );
    assert!(f.api.calls().is_empty());
    let owner = f.adopt(51);
    assert!(
        !f.client
            .vertical_zone_menu(&owner)
            .await
            .unwrap()
            .value
            .available
    );
    assert!(
        f.client
            .vertical_zone_page(&owner, "jazz", ZoneEntry::Tab(""))
            .await
            .is_err()
    );
    assert_eq!(
        f.api.calls(),
        vec![("signed_assignment".into(), json!("pc-a"))]
    );

    let f = ClientFixture::new(true, true, menu(vec![tag("j", "爵士", "jazz")]));
    let epoch = f.adopt(52);
    assert!(matches!(
        f.client
            .vertical_zone_page(&epoch, "forged", ZoneEntry::Tab(""))
            .await,
        Err(Error::InvalidInput("unregistered vertical zone page"))
    ));
    assert_eq!(
        f.api.calls(),
        vec![
            ("signed_assignment".into(), json!("pc-a")),
            (MENU.into(), json!({}))
        ]
    );
    let result = f
        .client
        .vertical_zone_page(&epoch, "jazz", ZoneEntry::SearchTabName("拉丁"))
        .await
        .unwrap();
    assert_eq!(result.value.active_tab.unwrap().key, "latin");
    assert_eq!(
        &f.api.calls()[2..],
        &[
            (PRIVACY.into(), json!({})),
            (TABS.into(), json!({"pageCode":"jazz","tabKey":"default"})),
            (TABS.into(), json!({"pageCode":"jazz","tabKey":"latin"})),
        ]
    );
}

#[tokio::test]
async fn menu_demotes_only_within_the_same_generation_transport_and_credential_revision() {
    let f = ClientFixture::new(true, false, menu(vec![tag("j", "爵士", "jazz")]));
    let epoch = f.adopt(53);
    let first = f.client.vertical_zone_menu(&epoch).await.unwrap().value;
    *f.api.menu.lock().unwrap() = menu(vec![]);
    assert_eq!(
        f.client.vertical_zone_menu(&epoch).await.unwrap().value,
        first
    );
    *f.api.menu.lock().unwrap() = json!({"code":503});
    assert_eq!(
        f.client.vertical_zone_menu(&epoch).await.unwrap().value,
        first
    );
    *f.api.menu.lock().unwrap() = menu(vec![]);
    *f.api.revision.lock().unwrap() = "pc-b".into();
    assert!(matches!(
        f.client
            .vertical_zone_page(&epoch, "jazz", ZoneEntry::Tab(""))
            .await,
        Err(Error::InvalidInput("unregistered vertical zone page"))
    ));
    assert_eq!(
        &f.api.calls()[4..],
        &[
            ("signed_assignment".into(), json!("pc-b")),
            (MENU.into(), json!({}))
        ]
    );
    let next = f.adopt(53);
    assert_ne!(next, epoch);
    assert!(matches!(
        f.client.vertical_zone_menu(&epoch).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        f.client
            .vertical_zone_menu(&next)
            .await
            .unwrap()
            .value
            .is_empty()
    );
}

#[tokio::test]
async fn removing_a_zone_or_switching_account_discards_in_flight_authority() {
    let f = ClientFixture::new(true, false, menu(vec![tag("j", "爵士", "jazz")]));
    let epoch = f.adopt(54);
    f.client.vertical_zone_menu(&epoch).await.unwrap();
    f.api.hold_page.store(true, Ordering::Release);
    let client = f.client.clone();
    let pending_epoch = epoch.clone();
    let pending = tokio::spawn(async move {
        client
            .vertical_zone_page(&pending_epoch, "jazz", ZoneEntry::Tab(""))
            .await
    });
    f.api.page_entered.notified().await;
    *f.api.menu.lock().unwrap() = menu(vec![tag("r", "摇滚", "rock")]);
    f.client.vertical_zone_menu(&epoch).await.unwrap();
    f.api.page_release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));

    f.api.hold_menu.store(true, Ordering::Release);
    let client = f.client.clone();
    let pending = tokio::spawn(async move { client.vertical_zone_menu(&epoch).await });
    f.api.menu_entered.notified().await;
    let next = f.adopt(55);
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    *f.api.menu.lock().unwrap() = menu(vec![]);
    assert!(
        f.client
            .vertical_zone_menu(&next)
            .await
            .unwrap()
            .value
            .is_empty()
    );
}
