//! Real native selection/actor boundaries with synthetic service and decoder.
#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod playback;
mod support;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    download::service::{DownloadOptions, DownloadOrigin, DownloadTask},
    playback::{controller::PlaybackStatus, engine::MediaInput},
    podcast::Episode,
    protocol::{
        api::{Api, ApiFuture, VinylPlaybackReadPermission, VinylVoiceRecommendationsRead},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

const CARD: &str = "/api/voice/play/page/rcmd";
const REVISION: &str = "sealed-voice-source-recommendation";
const AUDIO: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");

#[derive(Default)]
struct SourceApi {
    download: Option<Arc<support::download::Server>>,
    requests: Mutex<Vec<(String, Value)>>,
    hold_permission: AtomicBool,
    permission_started: Notify,
    permission_released: Notify,
}

fn episode(voice_id: u64, song_id: u64) -> Value {
    json!({
        "id":voice_id,"name":format!("合成节目 {voice_id}"),"duration":120000,
        "mainSong":{"id":song_id,"duration":120000},
        "radio":{"id":80,"name":"合成播单"},
    })
}

impl Api for SourceApi {
    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        Ok(Some(REVISION.into()))
    }

    fn request_vinyl_voice_recommendations<'a>(
        &'a self,
        read: VinylVoiceRecommendationsRead,
        revision: &'a str,
        permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            permission()?;
            assert_eq!(revision, REVISION);
            let (path, body) = read.into_request();
            assert_eq!(path, CARD);
            assert!(matches!(
                body["voiceId"].as_str(),
                Some("601" | "602" | "71")
            ));
            self.requests.lock().unwrap().push((path.into(), body));
            Ok(json!({"code":200,"data":[{"id":80},{"id":81},{"id":82},{"id":83}]}))
        })
    }

    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        if let Some(download) = &self.download {
            return download.request(path, body, cache);
        }
        Box::pin(async move {
            Ok(match path {
                "/api/voice/homepage/block/content" => {
                    assert_eq!(body, json!({"blockCode":"VOICE_HOMEPAGE_BLOCK_SCENE_FM"}));
                    json!({"code":200,"data":[{"creatives":[{
                        "creativeId":"OP_1","uiElement":{"mainTitle":{"title":"合成场景"}},
                        "creativeExtInfoVO":{"djPrograms":[episode(601, 1601),episode(602, 1602)]}
                    }]}]})
                }
                "/api/dj/program/detail" => {
                    let voice = body["id"].as_str().unwrap().parse::<u64>().unwrap();
                    assert!(matches!(voice, 601 | 602));
                    json!({"code":200,"program":episode(voice, 1000+voice)})
                }
                "/api/dj/play/url/check" => {
                    let voice = body["voiceId"].as_str().unwrap().parse::<u64>().unwrap();
                    assert!(matches!(voice, 601 | 602));
                    if voice == 601 && self.hold_permission.swap(false, Ordering::SeqCst) {
                        self.permission_started.notify_one();
                        self.permission_released.notified().await;
                    }
                    json!({"code":200,"data":{"voiceId":voice,"songId":1000+voice,"canPlay":true}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(body["ids"].as_str().unwrap()).unwrap();
                    assert!(ids == [json!("1601")] || ids == [json!("1602")]);
                    json!({"code":200,"data":[{"id":ids[0],"code":200,
                        "url":"https://m1.music.126.net/scene-fixture.mp3?key=SYNTHETIC",
                        "md5":"synthetic","expi":120,"level":"standard"}]})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":0}})
                }
                _ => panic!("unexpected sealed request {path}"),
            })
        })
    }
}

struct Fixture {
    root: tempfile::TempDir,
    store: Arc<cumulus_core::storage::StateStore>,
    client: Arc<MusicClient>,
    api: Arc<SourceApi>,
    device: Arc<playback::Device>,
}

impl Fixture {
    fn new(download: Option<Arc<support::download::Server>>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(&root.path().join("profile"), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let api = Arc::new(SourceApi {
            download,
            ..Default::default()
        });
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let device = Arc::new(playback::Device::default());
        let native = device.clone();
        let client = MusicClient::with_session(session, registry)
            .with_download_backend(Arc::new(support::download::Backend::new(AUDIO.into())))
            .with_audio_factory(move || Ok(playback::audio_engine(native.clone())));
        Self {
            root,
            store,
            client: Arc::new(client),
            api,
            device,
        }
    }

    fn recommendation_seeds(&self) -> Vec<String> {
        self.api
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|(path, args)| {
                assert_eq!(path, CARD);
                let id = args["voiceId"].as_str().unwrap();
                assert_eq!(args, &json!({"voiceId":id}));
                id.to_owned()
            })
            .collect()
    }
}

#[tokio::test]
async fn recommendations_require_committed_scene_voice_and_follow_actual_next_episode() {
    let f = Fixture::new(None);
    let directory = f.client.scene_fm_view("1", true).await.unwrap().value;
    f.api.hold_permission.store(true, Ordering::SeqCst);
    let pending = {
        let client = f.client.clone();
        let revision = directory.revision.unwrap();
        tokio::spawn(async move {
            client
                .scene_fm_command("1", SceneFmCommand::Play { revision })
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), f.api.permission_started.notified())
        .await
        .unwrap();
    let uncommitted = f.client.player_snapshot("1").await.unwrap();
    assert!(matches!(
        uncommitted.state,
        PlaybackStatus::Preparing | PlaybackStatus::Loading
    ));
    assert_eq!(uncommitted.track.unwrap().voice_id.as_deref(), Some("601"));
    assert!(matches!(
        f.client
            .player_vinyl_voice_recommendations("1", uncommitted.play_id.as_deref().unwrap())
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(f.recommendation_seeds().is_empty());
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
    f.api.permission_released.notify_one();
    let scene = pending.await.unwrap().unwrap().value;
    assert!(scene.active && scene.playing && !scene.pending);
    let first = f.client.player_snapshot("1").await.unwrap();
    assert_eq!(first.state, PlaybackStatus::Playing);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    let first_play = first.play_id.as_deref().unwrap();
    let first_card = f
        .client
        .player_vinyl_voice_recommendations("1", first_play)
        .await
        .unwrap();
    assert_eq!(
        (first_card.voice_id.as_str(), first_card.cards.len()),
        ("601", 4)
    );
    let next = f
        .client
        .player_action("1", PlayerAction::Next)
        .await
        .unwrap();
    assert_eq!(next.state, PlaybackStatus::Playing);
    assert_eq!(
        next.track.as_ref().unwrap().voice_id.as_deref(),
        Some("602")
    );
    assert_eq!(f.device.state.lock().unwrap().loads, 2);
    assert!(matches!(
        f.client
            .player_vinyl_voice_recommendations_next("1", first_play, &first_card.receipt),
        Err(Error::StaleResponse)
    ));
    let second = f
        .client
        .player_vinyl_voice_recommendations("1", next.play_id.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(second.voice_id, "602");
    assert_ne!(second.receipt, first_card.receipt);
    assert_eq!(f.recommendation_seeds(), ["601", "602"]);
}

#[tokio::test]
async fn recommendations_use_typed_voice_download_not_embedded_song_metadata() {
    let download = Arc::new(support::download::Server::new(AUDIO, "flac"));
    let f = Fixture::new(Some(download.clone()));
    f.client
        .download_command(
            "1",
            DownloadCommand::Configure {
                root: f.root.path().join("media").to_string_lossy().into(),
                path_type: 2,
                file_type: 1,
            },
        )
        .await
        .unwrap();
    let started = f
        .client
        .download_command(
            "1",
            DownloadCommand::Start {
                song_ids: vec!["123".into()],
                options: DownloadOptions {
                    tags: false,
                    cover: false,
                    lyrics: false,
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();
    let id = &started.accepted[0];
    let completed = tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let downloads = f
                .client
                .downloads(
                    "1",
                    DownloadQuery {
                        tab: DownloadTab::Completed,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            if let Some(task) = downloads.tasks.first().filter(|task| !task.cleanup_pending) {
                break task.path.clone().unwrap();
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read(completed).unwrap(), AUDIO);
    let mut stored: DownloadTask =
        serde_json::from_value(f.store.get("1", "download_tasks_v1", id).unwrap().unwrap())
            .unwrap();
    stored.song = Some(
        Episode::from_official(&episode(71, 123))
            .unwrap()
            .download_metadata()
            .unwrap(),
    );
    let select = |category| PlaySelection::Downloads {
        query: DownloadQuery {
            tab: DownloadTab::Completed,
            category,
            ..Default::default()
        },
        start_id: Some(id.clone()),
    };
    let call_count = download.calls.lock().unwrap().len();
    f.store
        .put(
            "1",
            "download_tasks_v1",
            id,
            &serde_json::to_value(&stored).unwrap(),
        )
        .unwrap();
    let song = f
        .client
        .play_selection("1", select(DownloadCategory::Song))
        .await
        .unwrap();
    assert_eq!(song.resource_kind, Some(PlayerResourceKind::Downloaded));
    assert!(song.voice_detail.is_none());
    assert!(matches!(
        f.client
            .player_vinyl_voice_recommendations("1", song.play_id.as_deref().unwrap())
            .await,
        Err(Error::StaleResponse)
    ));
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    assert!(f.recommendation_seeds().is_empty());
    stored.origin = DownloadOrigin::Voice {
        voice_id: "71".into(),
    };
    f.store
        .put(
            "1",
            "download_tasks_v1",
            id,
            &serde_json::to_value(&stored).unwrap(),
        )
        .unwrap();
    let voice = f
        .client
        .play_selection("1", select(DownloadCategory::Voice))
        .await
        .unwrap();
    assert_eq!(voice.state, PlaybackStatus::Playing);
    assert_eq!(
        voice.track.as_ref().unwrap().voice_id.as_deref(),
        Some("71")
    );
    assert!(matches!(
        f.device.state.lock().unwrap().last_source.as_ref(),
        Some(MediaInput::File(_))
    ));
    let play_id = voice.play_id.as_deref().unwrap();
    let card = f
        .client
        .player_vinyl_voice_recommendations("1", play_id)
        .await
        .unwrap();
    assert_eq!((&*card.voice_id, &*card.cards[0].radio_id), ("71", "80"));
    assert_eq!(
        f.client
            .player_vinyl_voice_recommendations_next("1", play_id, &card.receipt)
            .unwrap()
            .page,
        1
    );
    assert_eq!(f.recommendation_seeds(), ["71"]);
    assert_eq!(
        download.calls.lock().unwrap().len(),
        call_count,
        "offline audio was not authorized or downloaded again"
    );
}
