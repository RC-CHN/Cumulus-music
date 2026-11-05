#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    catalog::vertical_zone::ZoneEntry,
    client::{MusicClient, dto::*},
    membership::Quality,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

const V4: &str = "/api/playlist/v4/detail";
const V6: &str = "/api/v6/playlist/detail";
const DYNAMIC: &str = "/api/playlist/detail/dynamic";
const SONG: &str = "/api/v3/song/detail";
const URL: &str = "/api/song/enhance/player/url/v1";

#[derive(Default)]
struct Wire {
    calls: Mutex<Vec<(String, Value)>>,
    revision: AtomicU64,
    removed: AtomicBool,
    own: AtomicBool,
    dynamic_fails: AtomicBool,
}

impl Wire {
    fn take_calls(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}

impl Api for Wire {
    fn verified_pc_experiment_revision(&self) -> cumulus_core::Result<Option<String>> {
        Ok(Some(self.revision.load(Ordering::Acquire).to_string()))
    }

    fn verified_pc_experiment_groups<'a>(
        &'a self,
        _: &'a str,
    ) -> ApiFuture<'a, PcExperimentGroups> {
        Box::pin(async {
            PcExperimentGroups::refreshed(&json!({"data":[
                {"expName":"PH-PC-zhuanqu","expGroupName":"t1"},
                {"expName":"PH-PC-zhuanquTab","expGroupName":"t1"}
            ]}))
        })
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        self.calls.lock().unwrap().push((path.into(), args.clone()));
        Box::pin(async move {
            Ok(match path {
                "/api/secondary/tab/list" => {
                    let page = if self.removed.load(Ordering::Acquire) {
                        "rock"
                    } else {
                        "jazz"
                    };
                    json!({"code":200,"data":{"hints":[{"position":{"code":"music_top_tab_full"},
                        "data":{"extra":{"nowChannelItems":[{"code":page,"title":"专区","url":format!("orpheus://rnpage?page={page}")}]}}}]}})
                }
                "/api/user/setting" => json!({"code":200,"setting":{"personalServiceSetting":0}}),
                "/api/zone/tab/get" => {
                    let rows = (204..210).map(|id| json!({"resourceType":"playlist","id":id,"name":format!("横排 {id}")})).collect::<Vec<_>>();
                    json!({"code":200,"data":{"tabMetas":[{"tabKey":"default","tabKeyName":"全部"}],
                    "currentTabContent":{"tabKey":"default","tabKeyName":"全部","blocks":[
                        {"componentName":"RecommendGrid","children":[{"resourceType":"playlist","id":204,"name":"普通同一歌单"}]},
                        {"componentName":"RecommendHorizontal","hideAfterClosePersonalized":true,"children":rows}
                    ]}}})
                }
                V4 | V6 => json!({"code":200,"playlist":{
                    "id":204,"privacy":0,"name":if path == V4 { "MGC visit" } else { "Normal visit" },
                    "creator":{"userId":if self.own.load(Ordering::Acquire) { 7 } else { 8 },"nickname":"合成歌单作者"},
                    "trackCount":3,"subscribedCount":6,
                    "trackIds":[{"id":101,"v":0},{"id":102,"v":0},{"id":103,"v":-1}]
                }}),
                DYNAMIC if self.dynamic_fails.load(Ordering::Acquire) => json!({"code":503}),
                DYNAMIC => json!({"code":200,"bookedCount":81,"playCount":234,"subscribed":true,
                    "playlist":{"id":999},"trackIds":[{"id":999}]}),
                SONG => {
                    let ids: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    json!({"code":200,"songs":ids.iter().map(|item|
                        json!({"id":item["id"],"name":"合成歌曲","dt":30000})).collect::<Vec<_>>()})
                }
                URL => {
                    let ids: Vec<Value> =
                        serde_json::from_str(args["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,
                        "url":"https://m1.music.126.net/synthetic-zone.mp3","md5":"fixture","expi":120,"level":"standard"}]})
                }
                _ => panic!("unrecognized sealed request {path}"),
            })
        })
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    wire: Arc<Wire>,
    session: Arc<AccountSession>,
    client: MusicClient,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let wire = Arc::new(Wire::default());
        let session = Arc::new(AccountSession::new(wire.clone(), Some(registry.store())));
        let client = MusicClient::with_session(session.clone(), registry)
            .with_audio_factory(|| Ok(engine::audio_engine(Default::default())));
        let f = Self {
            _root: root,
            wire,
            session,
            client,
        };
        f.adopt(7);
        f
    }

    fn adopt(&self, uid: u64) {
        self.session
            .adopt(
                self.wire.clone(),
                json!({"userId":uid}),
                &self.session.token().unwrap(),
            )
            .unwrap();
    }

    fn epoch(&self) -> String {
        self.session.token().unwrap().generation.to_string()
    }

    async fn source(&self) -> (String, String) {
        let epoch = self.epoch();
        let view = self
            .client
            .vertical_zone_page(&epoch, "jazz", ZoneEntry::Tab(""))
            .await
            .unwrap()
            .value;
        assert!(matches!(
            self.client
                .vertical_zone_playlist_origin(&epoch, &view.view_id, "0:0", "204")
                .await,
            Err(Error::InvalidInput(
                "playlist card has no MGC detail origin"
            ))
        ));
        assert!(
            self.client
                .vertical_zone_playlist_origin(&epoch, &view.view_id, "1:0", "205")
                .await
                .is_err()
        );
        let receipt = self
            .client
            .vertical_zone_playlist_origin(&epoch, &view.view_id, "1:0", "204")
            .await
            .unwrap()
            .value
            .receipt;
        self.wire.take_calls();
        (epoch, receipt)
    }
}

fn page(offset: usize, limit: usize, revision: Option<String>) -> PageRequest {
    PageRequest {
        offset,
        limit,
        revision,
    }
}

fn song_request(ids: &[&str], trial: bool) -> (String, Value) {
    let rows = ids
        .iter()
        .map(|id| json!({"id":id,"v":0}))
        .collect::<Vec<_>>();
    let mut payload = json!({"c":serde_json::to_string(&rows).unwrap()});
    if trial {
        payload["trialMode"] = json!(121);
    }
    (SONG.into(), payload)
}

#[tokio::test]
async fn scope_survives_source_view_replacement_but_never_attaches_to_an_ordinary_visit() {
    let f = Fixture::new();
    let (epoch, receipt) = f.source().await;
    let first = f
        .client
        .vertical_zone_playlist_detail(&epoch, &receipt, "204", page(0, 1, None))
        .await
        .unwrap()
        .value;
    assert_eq!(first.tracks[0].id, "101");
    let header = first.playlist.unwrap();
    assert_eq!(
        (
            header.name.as_str(),
            header.subscribed_count,
            header.play_count
        ),
        ("MGC visit", Some(81), Some(234))
    );
    assert_eq!(
        f.wire.take_calls(),
        vec![
            (V4.into(), json!({"id":"204","n":0,"s":0})),
            (DYNAMIC.into(), json!({"id":"204"})),
            song_request(&["101"], true)
        ]
    );
    f.client
        .vertical_zone_page(&epoch, "jazz", ZoneEntry::Tab(""))
        .await
        .unwrap();
    f.wire.take_calls();
    let second = f
        .client
        .vertical_zone_playlist_detail(&epoch, &receipt, "204", page(1, 2, first.revision))
        .await
        .unwrap()
        .value;
    assert_eq!(second.tracks[0].id, "102");
    assert!(second.tracks[1].local && second.tracks[1].missing && !second.more);
    assert_eq!(f.wire.take_calls(), vec![song_request(&["102"], true)]);

    let ordinary = f
        .client
        .playlist_page("204", page(0, 1, None))
        .await
        .unwrap()
        .value;
    assert_eq!(ordinary.playlist.unwrap().name, "Normal visit");
    assert_eq!(
        f.wire.take_calls(),
        vec![
            (V6.into(), json!({"id":"204","n":100000,"s":0})),
            song_request(&["101"], false)
        ]
    );
}

#[tokio::test]
async fn fallback_and_account_credential_registry_fences_keep_the_visit_local() {
    let f = Fixture::new();
    let (epoch, receipt) = f.source().await;
    f.wire.dynamic_fails.store(true, Ordering::Release);
    let detail = f
        .client
        .vertical_zone_playlist_detail(&epoch, &receipt, "204", page(0, 1, None))
        .await
        .unwrap()
        .value;
    assert_eq!(detail.playlist.unwrap().subscribed_count, Some(6));
    assert_eq!(f.wire.take_calls().len(), 3);
    assert!(
        f.client
            .vertical_zone_playlist_detail(&epoch, &receipt, "205", page(0, 1, None))
            .await
            .is_err()
    );
    assert!(matches!(
        f.client
            .vertical_zone_playlist_detail(
                &epoch,
                &receipt,
                "204",
                page(1, 1, Some("forged".into()))
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(f.wire.take_calls().is_empty());
    f.wire.removed.store(true, Ordering::Release);
    f.client.vertical_zone_menu(&epoch).await.unwrap();
    f.wire.take_calls();
    assert!(matches!(
        f.client
            .vertical_zone_playlist_detail(&epoch, &receipt, "204", page(0, 1, None))
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(f.wire.take_calls().is_empty());
    f.wire.removed.store(false, Ordering::Release);
    f.client.vertical_zone_menu(&epoch).await.unwrap();
    let (epoch, receipt) = f.source().await;
    f.wire.revision.store(1, Ordering::Release);
    assert!(matches!(
        f.client
            .vertical_zone_playlist_detail(&epoch, &receipt, "204", page(0, 1, None))
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(f.wire.take_calls().is_empty());
    f.adopt(7);
    assert!(matches!(
        f.client
            .vertical_zone_playlist_detail(&f.epoch(), &receipt, "204", page(0, 1, None))
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(f.wire.take_calls().is_empty());
    let (epoch, receipt) = f.source().await;
    f.wire.own.store(true, Ordering::Release);
    assert!(matches!(
        f.client
            .vertical_zone_playlist_detail(&epoch, &receipt, "204", page(0, 1, None))
            .await,
        Err(Error::State(
            "owned and special MGC playlist details are not supported"
        ))
    ));
    assert_eq!(
        f.wire.take_calls(),
        vec![(V4.into(), json!({"id":"204","n":0,"s":0}))]
    );
}

#[tokio::test]
async fn scoped_row_play_authorizes_trial_121_while_normal_same_id_stays_default() {
    let f = Fixture::new();
    let (epoch, receipt) = f.source().await;
    let revision = f
        .client
        .vertical_zone_playlist_detail(&epoch, &receipt, "204", page(0, 2, None))
        .await
        .unwrap()
        .value
        .revision
        .unwrap();
    f.wire.take_calls();
    let view = f
        .client
        .play_selection(
            &epoch,
            PlaySelection::VerticalZonePlaylistDetail {
                id: "204".into(),
                receipt,
                revision,
                start_id: Some("102".into()),
                double_click: true,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.track.unwrap().id, "102");
    let row_calls = await_url(&f.wire, 121).await;
    assert!(row_calls.contains(&song_request(&["101", "102"], true)));
    assert!(
        !row_calls
            .iter()
            .any(|(path, _)| matches!(path.as_str(), V4 | V6 | DYNAMIC))
    );
    f.client
        .play_selection(
            &epoch,
            PlaySelection::Playlist {
                id: "204".into(),
                start_id: Some("102".into()),
                revision: None,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let ordinary = await_url(&f.wire, -1).await;
    assert!(ordinary.contains(&song_request(&["101", "102"], false)));
    assert!(ordinary.iter().any(|(path, _)| path == V6));
}

async fn await_url(wire: &Wire, trial: i32) -> Vec<(String, Value)> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut calls = Vec::new();
        loop {
            calls.extend(wire.take_calls());
            if calls
                .iter()
                .any(|(path, payload)| path == URL && payload["trialMode"] == trial)
            {
                return calls;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("scoped playback URL")
}
