#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    catalog::vertical_zone::ZoneEntry,
    client::{
        MusicClient,
        dto::{FmCommand, PlaySelection, PlayerAction, ZoneFmCardGesture, ZoneFmCardResult},
    },
    fm::FmSource,
    membership::Quality,
    playback::controller::PlaybackStatus,
    protocol::{
        api::{Api, ApiFuture, identifier},
        http::ApiOrigin,
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;

const AUTH: &str = "/api/song/enhance/player/url/v1";
type MenuGate = (oneshot::Sender<()>, oneshot::Receiver<()>);

#[derive(Default)]
struct Wire {
    calls: Mutex<Vec<(String, Value)>>,
    menu_gate: Mutex<Option<MenuGate>>,
}
impl Wire {
    fn record(&self, path: &str, args: Value) {
        self.calls.lock().unwrap().push((path.into(), args));
    }
    fn trace(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }
    fn hold_next_menu(&self) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (arrived, entered) = oneshot::channel();
        let (resume, held) = oneshot::channel();
        assert!(
            self.menu_gate
                .lock()
                .unwrap()
                .replace((arrived, held))
                .is_none()
        );
        (entered, resume)
    }
    fn urls(&self) -> Vec<(String, Value)> {
        self.trace()
            .into_iter()
            .filter(|(path, _)| path == AUTH)
            .collect()
    }
}

impl Api for Wire {
    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        Ok(Some("sealed-explicit-zone".into()))
    }
    fn verified_pc_experiment_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, PcExperimentGroups> {
        Box::pin(async move {
            self.record("signed-assignment", json!(revision));
            PcExperimentGroups::refreshed(&json!({"data":[
                {"expName":"PH-PC-zhuanqu","expGroupName":"t1"},
                {"expName":"PH-PC-zhuanquTab","expGroupName":"t1"}
            ]}))
        })
    }
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        self.record(path, args.clone());
        Box::pin(async move {
            if path == "/api/secondary/tab/list" {
                let gate = self.menu_gate.lock().unwrap().take();
                if let Some((arrived, release)) = gate {
                    arrived.send(()).unwrap();
                    release.await.unwrap();
                }
            }
            Ok(match path {
                "/api/secondary/tab/list" => json!({"code":200,"data":{"hints":[{
                    "position":{"code":"music_top_tab_full"},"data":{"extra":{"nowChannelItems":[
                        {"code":"rock","title":"摇滚","url":"orpheus://rnpage?page=ROCK"}
                    ]}}
                }]}}),
                "/api/user/setting" => json!({"code":200,"setting":{"personalServiceSetting":0}}),
                "/api/zone/tab/get" => json!({"code":200,"data":{
                    "currentTabContent":{"tabKey":args["tabKey"],"tabKeyName":"全部","blocks":[{
                        "componentName":"RecommendHorizontal","children":[
                            {"resourceType":"fm","resourceId":11,"title":"A"},
                            {"resourceType":"fm","resourceId":12,"title":"B","hideAfterClosePersonalized":true},
                            {"resourceType":"fm","resourceId":13,"title":"C"},
                            {"resourceType":"fm","resourceId":14,"title":"D"},
                            {"resourceType":"playlist","id":55,"title":"非漫游"},
                            {"resourceType":"playlist","id":56,"title":"补全横栏"}
                        ]
                    }]},"tabMetas":[{"tabKey":"default","tabKeyName":"全部"},{"tabKey":"other","tabKeyName":"其它"}]
                }}),
                "/api/music/dislike/get" => {
                    json!({"code":200,"data":{"songIdList":[],"artistIdList":[]}})
                }
                "/api/v3/radio/trash/get" => json!({"code":200,"count":0,"data":[],"addTime":""}),
                "/api/music-vip-membership/client/vip/info" => {
                    json!({"code":200,"data":{"now":1793030000000_u64,"userId":51}})
                }
                "/api/tag/tab/fm/recommends" => {
                    let id = match args["ruleId"].as_str() {
                        Some("11" | "12") => 81,
                        Some("13") => 83,
                        Some("14") => 99,
                        _ => panic!("unexpected zone rule"),
                    };
                    json!({"code":200,"data":[{"id":id,"v":0}]})
                }
                "/api/v3/song/detail" => {
                    let values: Vec<Value> = serde_json::from_str(args["c"].as_str().unwrap())?;
                    let songs = values
                        .iter()
                        .map(|v| {
                            let id = identifier(&v["id"]).unwrap();
                            json!({"id":id,"name":format!("测试歌曲 {id}"),"dt":100000,"fee":0,
                            "al":{"id":7,"name":"专辑"},"ar":[{"id":9,"name":"歌手"}]})
                        })
                        .collect::<Vec<_>>();
                    json!({"code":200,"songs":songs})
                }
                AUTH => {
                    let values: Vec<Value> = serde_json::from_str(args["ids"].as_str().unwrap())?;
                    let id = identifier(&values[0]).unwrap();
                    if id == "99" {
                        return Ok(json!({"code":200,"data":[{"id":id,"code":404,"url":null}]}));
                    }
                    json!({"code":200,"data":[{"id":id,"code":200,
                        "url":"https://m1.music.126.net/fixture.mp3","md5":"synthetic","level":"standard","expi":120}]})
                }
                _ => panic!("unexpected explicit zone wire route {path}"),
            })
        })
    }
}

#[tokio::test]
async fn cover_and_overlay_use_actual_transport_and_only_compatible_fm_reuses_a_decoder() {
    use ZoneFmCardGesture::{Cover, Overlay};
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let wire = Arc::<Wire>::default();
    let session = Arc::new(AccountSession::new(wire.clone(), Some(registry.store())));
    session
        .adopt(
            wire.clone(),
            json!({"userId":51}),
            &session.token().unwrap(),
        )
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let device = Arc::<engine::Device>::default();
    let player = Arc::new(
        MusicClient::with_session(session, registry)
            .with_audio_factory(move || Ok(engine::audio_engine(device.clone()))),
    );
    let view = player
        .vertical_zone_page(&epoch, "ROCK", ZoneEntry::Tab("default"))
        .await
        .unwrap()
        .value
        .view_id;
    let run = |key: &'static str, gesture| {
        let (player, epoch, view) = (&player, epoch.as_str(), view.as_str());
        async move {
            player
                .vertical_zone_fm_card(epoch, view, key, gesture, Quality::Standard)
                .await
                .map(|result| result.value)
        }
    };
    let ordinary = player
        .play_selection(
            &epoch,
            PlaySelection::Songs {
                ids: vec!["7".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert!(!ordinary.fm && ordinary.state == PlaybackStatus::Playing);

    let a = run("0:0", Cover).await.unwrap();
    assert_open(&a, "11", "81");
    let a_play = a.playback.play_id;
    assert_ne!(a_play, ordinary.play_id);
    let (entered, resume) = wire.hold_next_menu();
    let menu_refresh = {
        let player = player.clone();
        let epoch = epoch.clone();
        tokio::spawn(async move { player.vertical_zone_menu(&epoch).await })
    };
    tokio::time::timeout(Duration::from_secs(2), entered)
        .await
        .expect("menu refresh entered the transport")
        .unwrap();
    let before = wire.trace();
    let cover_b = tokio::time::timeout(Duration::from_secs(2), run("0:1", Cover))
        .await
        .expect("cover does not wait for a pending menu refresh")
        .unwrap();
    assert_open(&cover_b, "11", "81");
    assert_eq!(cover_b.playback.play_id, a_play);
    assert_eq!(
        wire.trace(),
        before,
        "a different cover opens actual Zone FM without reads"
    );
    resume.send(()).unwrap();
    menu_refresh.await.unwrap().unwrap();
    let paused = run("0:0", Overlay).await.unwrap();
    assert_eq!(paused.playback.state, PlaybackStatus::Paused);
    assert_eq!(paused.playback.play_id, a_play);
    assert!(paused.detail_play_id.is_none());
    let resumed = run("0:0", Cover).await.unwrap();
    assert_open(&resumed, "11", "81");
    assert_eq!(resumed.playback.play_id, a_play);

    let url_calls = wire.urls();
    let cross_rule = run("0:1", Overlay).await.unwrap();
    assert_eq!(cross_rule.playback.state, PlaybackStatus::Paused);
    assert_eq!(cross_rule.playback.zone_fm_rule_id.as_deref(), Some("12"));
    assert_eq!(cross_rule.playback.play_id, a_play);
    assert!(cross_rule.detail_play_id.is_none());
    assert_eq!(
        wire.urls(),
        url_calls,
        "same authorized song/quality/trial retains decoder"
    );
    let b = run("0:1", Overlay).await.unwrap();
    assert_eq!(b.playback.state, PlaybackStatus::Playing);
    assert!(b.detail_play_id.is_none());
    assert_eq!(b.playback.play_id, a_play);

    let c = run("0:2", Overlay).await.unwrap();
    assert_eq!(c.playback.state, PlaybackStatus::Playing);
    assert_eq!(c.playback.zone_fm_rule_id.as_deref(), Some("13"));
    assert_ne!(c.playback.play_id, a_play);
    assert!(c.detail_play_id.is_none());
    assert_eq!(wire.urls().last().unwrap().1["trialMode"], 121);

    player
        .fm_command(
            &epoch,
            FmSource::Zone {
                rule_id: "11".into(),
                trial_mode: Some(-1),
            },
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let legacy = player.player_snapshot(&epoch).await.unwrap();
    assert_eq!(legacy.zone_fm_rule_id.as_deref(), Some("11"));
    assert_eq!(legacy.track.unwrap().id, "81");
    assert_eq!(wire.urls().last().unwrap().1["trialMode"], -1);
    let incompatible = run("0:1", Overlay).await.unwrap();
    assert_eq!(incompatible.playback.state, PlaybackStatus::Playing);
    assert_eq!(incompatible.playback.zone_fm_rule_id.as_deref(), Some("12"));
    assert_ne!(
        incompatible.playback.play_id, legacy.play_id,
        "cannot carry another trial's decoder"
    );
    assert!(incompatible.detail_play_id.is_none());

    player
        .player_action(&epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(
        run("0:3", Cover).await.is_err(),
        "unavailable media cannot issue detail receipt"
    );
    let new_view = player
        .vertical_zone_page(&epoch, "ROCK", ZoneEntry::Tab("other"))
        .await
        .unwrap()
        .value
        .view_id;
    assert_ne!(view, new_view);
    let stale_before = wire.trace();
    assert!(matches!(run("0:0", Cover).await, Err(Error::StaleResponse)));
    assert_eq!(wire.trace(), stale_before);
}

fn assert_open(value: &ZoneFmCardResult, rule: &str, id: &str) {
    assert_eq!(value.playback.state, PlaybackStatus::Playing);
    assert!(value.playback.fm);
    assert_eq!(value.playback.zone_fm_rule_id.as_deref(), Some(rule));
    assert_eq!(value.playback.track.as_ref().unwrap().id, id);
    assert!(value.detail_play_id.is_some());
    assert_eq!(value.detail_play_id, value.playback.play_id);
}
