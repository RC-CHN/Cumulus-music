use super::*;
use crate::{
    account::{AccountSession, profiles::ProfileRegistry},
    catalog::vertical_zone::ZoneEntry,
    fm::{FmRights, FmSource},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

const RECOMMENDS: &str = "/api/tag/tab/fm/recommends";
const DETAILS: &str = "/api/v3/song/detail";

#[derive(Default)]
struct Wire {
    fm_calls: Mutex<Vec<(&'static str, Value)>>,
    hold_detail: AtomicBool,
    entered: Notify,
    release: Notify,
}

impl Wire {
    fn calls(&self) -> Vec<(&'static str, Value)> {
        self.fm_calls.lock().unwrap().clone()
    }
}

impl Api for Wire {
    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        Ok(Some("fixture-preview-pc".into()))
    }

    fn verified_pc_experiment_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, PcExperimentGroups> {
        Box::pin(async move {
            assert_eq!(revision, "fixture-preview-pc");
            PcExperimentGroups::refreshed(&json!({"data":[
                {"expName":"PH-PC-zhuanqu","expGroupName":"t1"},
                {"expName":"PH-PC-zhuanquTab","expGroupName":"t1"}
            ]}))
        })
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        assert!(!cache, "{path}");
        Box::pin(async move {
            Ok(match path {
                "/api/secondary/tab/list" => json!({"code":200,"data":{"hints":[{
                    "position":{"code":"music_top_tab_full"},
                    "data":{"extra":{"nowChannelItems":[
                        {"code":"rock","title":"摇滚","url":"orpheus://rnpage?page=ROCK"}
                    ]}}
                }]}}),
                "/api/user/setting" => json!({"code":200,"setting":{"personalServiceSetting":0}}),
                "/api/zone/tab/get" => json!({"code":200,"data":{
                    "currentTabContent":{"tabKey":args["tabKey"],"tabKeyName":"全部","blocks":[{
                        "componentName":"RecommendHorizontal","children":[
                            {"resourceType":"fm","resourceId":11,"title":"漫游 11","hideAfterClosePersonalized":true},
                            {"resourceType":"fm","resourceId":11,"title":"同一个规则"},
                            {"resourceType":"fm","resourceId":12,"title":"封面未确认"},
                            {"resourceType":"fm","resourceId":13,"title":"没有推荐歌曲"},
                            {"resourceType":"playlist","id":51,"title":"另一种卡片"},
                            {"resourceType":"playlist","id":52,"title":"补全横栏"}
                        ]
                    }]},
                    "tabMetas":[{"tabKey":"default","tabKeyName":"全部"},{"tabKey":"other","tabKeyName":"其它"}]
                }}),
                RECOMMENDS => {
                    self.fm_calls
                        .lock()
                        .unwrap()
                        .push((RECOMMENDS, args.clone()));
                    let data = match args["ruleId"].as_str() {
                        Some("11") => json!([{"id":81,"v":5},{"id":"84","v":null}]),
                        Some("12") => json!([{"id":91}]),
                        Some("13") => json!([]),
                        _ => panic!("unverified synthetic rule"),
                    };
                    json!({"code":200,"data":data})
                }
                DETAILS => {
                    self.fm_calls.lock().unwrap().push((DETAILS, args.clone()));
                    if self.hold_detail.swap(false, Ordering::AcqRel) {
                        self.entered.notify_one();
                        self.release.notified().await;
                    }
                    match args["c"].as_str().unwrap() {
                        r#"[{"id":91,"v":0}]"# => json!({"code":200,"songs":[
                            {"id":91,"al":{"picUrl":"https://outside.invalid/unsafe"}}
                        ]}),
                        r#"[{"id":81,"v":5},{"id":"84","v":0}]"# => json!({"code":200,"songs":[
                            {"id":84,"album":{"picUrl":"https://p1.music.126.net/wrong-order.jpg"}},
                            {"id":81,"al":{"picUrl":"http://p1.music.126.net/first.jpg?secret=private#fragment"},
                                "alg":"private_algorithm"}
                        ],"privileges":[{"id":81,"pl":320000}]}),
                        _ => panic!("unexpected detail references: {args}"),
                    }
                }
                _ => panic!("passive zone FM accessed unexpected path {path}"),
            })
        })
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    session: Arc<AccountSession>,
    wire: Arc<Wire>,
    client: Arc<MusicClient>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let wire = Arc::<Wire>::default();
        let session = Arc::new(AccountSession::new(wire.clone(), Some(registry.store())));
        let client = MusicClient::with_session(session.clone(), registry)
            .with_audio_factory(|| panic!("passive FM initialized native audio"));
        Self {
            _dir: root,
            session,
            wire,
            client: Arc::new(client),
        }
    }

    fn account(&self, id: u64) -> String {
        self.session
            .adopt(
                self.wire.clone(),
                json!({"userId":id}),
                &self.session.token().unwrap(),
            )
            .unwrap();
        self.session.token().unwrap().generation.to_string()
    }

    async fn page(&self, epoch: &str, tab: &str) -> String {
        self.client
            .vertical_zone_page(epoch, "ROCK", ZoneEntry::Tab(tab))
            .await
            .unwrap()
            .value
            .view_id
    }
}

#[tokio::test]
async fn passive_preview_is_once_per_rule_and_explicit_preload_reuses_its_order() {
    let f = Fixture::new();
    let epoch = f.account(51);
    let view = f.page(&epoch, "default").await;
    let invalid = f
        .client
        .vertical_zone_fm_preview(&epoch, &view, "0:4")
        .await;
    assert!(matches!(invalid, Err(Error::InvalidInput(_))));
    assert!(f.wire.calls().is_empty());

    let (left, right) = tokio::join!(
        f.client.vertical_zone_fm_preview(&epoch, &view, "0:0"),
        f.client.vertical_zone_fm_preview(&epoch, &view, "0:1"),
    );
    let cover = "https://p1.music.126.net/first.jpg?param=320y320";
    for preview in [left.unwrap(), right.unwrap()] {
        assert_eq!(preview.epoch, epoch);
        assert_eq!(
            serde_json::to_value(preview.value).unwrap(),
            json!({"coverUrl":cover})
        );
    }
    assert_eq!(
        f.wire.calls(),
        vec![
            (
                RECOMMENDS,
                json!({"ruleType":1,"limit":3,"ruleId":"11","firstSongId":"",
                "firstSongAlg":"","tabTagId":""})
            ),
            (
                DETAILS,
                json!({"c":r#"[{"id":81,"v":5},{"id":"84","v":0}]"#,"trialMode":-1})
            ),
        ]
    );
    let token = f.session.token().unwrap();
    let service = f.client.player.fm_service().await.unwrap();
    let explicit = FmSource::Zone {
        rule_id: "11".into(),
        trial_mode: Some(121),
    };
    assert!(service.queue_for(&token, &explicit).unwrap().is_none());
    assert!(f.client.player.controller.get().is_none());
    let queue = service
        .preload_for(&token, explicit.clone(), FmRights::default())
        .await
        .unwrap();
    assert_eq!(queue.source, explicit);
    assert_eq!(queue.source.playback_trial_mode(), 121);
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|track| track.id.as_str())
            .collect::<Vec<_>>(),
        ["81", "84"]
    );
    assert_eq!(queue.current().unwrap().song["privilege"]["pl"], 320000);
    f.session
        .persistent_store()
        .unwrap()
        .put(
            "51",
            "fm-blacklist-v1",
            "current",
            &json!({"songs":["81"],"artists":[]}),
        )
        .unwrap();
    let filtered = service
        .playable_for(&token, &explicit, queue.revision)
        .await
        .unwrap();
    assert_eq!(filtered.current().unwrap().id, "84");
    let card = f
        .client
        .vertical_zone_fm_preview(&epoch, &view, "0:0")
        .await
        .unwrap();
    assert_eq!(card.value.cover_url.as_deref(), Some(cover));
    assert_eq!(f.wire.calls().len(), 2);
    assert!(f.client.player.controller.get().is_none());
}

#[tokio::test]
async fn missing_or_invalid_album_art_keeps_fallback_without_exposing_any_track() {
    let f = Fixture::new();
    let epoch = f.account(51);
    let view = f.page(&epoch, "default").await;
    for _ in 0..2 {
        let preview = f
            .client
            .vertical_zone_fm_preview(&epoch, &view, "0:2")
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(preview.value).unwrap(),
            json!({"coverUrl":null})
        );
    }
    // An unsafe picture still belongs to an existing first track; don't
    // request again simply because the renderer must use its fallback art.
    assert_eq!(f.wire.calls().len(), 2);
    for _ in 0..2 {
        let preview = f
            .client
            .vertical_zone_fm_preview(&epoch, &view, "0:3")
            .await
            .unwrap();
        assert!(preview.value.cover_url.is_none());
    }
    assert_eq!(
        f.wire
            .calls()
            .iter()
            .filter(|(path, _)| *path == DETAILS)
            .count(),
        1
    );
    assert_eq!(f.wire.calls().len(), 4);
}

#[tokio::test]
async fn discarded_view_or_account_cannot_publish_a_late_first_batch() {
    let f = Fixture::new();
    let mut epoch = f.account(51);
    let mut view = f.page(&epoch, "default").await;
    for account_changed in [false, true] {
        f.wire.hold_detail.store(true, Ordering::Release);
        let (client, old_epoch, old_view) = (f.client.clone(), epoch.clone(), view.clone());
        let pending = tokio::spawn(async move {
            client
                .vertical_zone_fm_preview(&old_epoch, &old_view, "0:0")
                .await
        });
        f.wire.entered.notified().await;
        if account_changed {
            epoch = f.account(52);
        } else {
            view = f.page(&epoch, "other").await;
        }
        f.wire.release.notify_waiters();
        assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    }
    assert_ne!(view, f.page(&epoch, "default").await);
    assert_eq!(f.wire.calls().len(), 4);
    view = f.page(&epoch, "default").await;
    assert!(
        f.client
            .vertical_zone_fm_preview(&epoch, &view, "0:0")
            .await
            .unwrap()
            .value
            .cover_url
            .is_some()
    );
    assert_eq!(f.wire.calls().len(), 6);
}
