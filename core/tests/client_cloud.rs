#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod playback_support;
#[path = "support/cloud_upload.rs"]
mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    cloud::upload::*,
    membership::Quality,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
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
#[derive(Default)]
struct Backend {
    rows: Mutex<Vec<Value>>,
    calls: Mutex<Vec<(String, Value)>>,
    input: Mutex<(String, u64)>,
    gate: Mutex<Option<Arc<support::nos::Gate>>>,
    allocation_gate: Mutex<Option<Arc<support::nos::Gate>>>,
    full: AtomicBool,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            if path == support::LIST {
                let gate = self.gate.lock().unwrap().take();
                if let Some(gate) = gate {
                    gate.entered.notify_one();
                    gate.release.acquire().await.unwrap().forget();
                }
                let offset = payload["offset"].as_u64().unwrap() as usize;
                let rows = self.rows.lock().unwrap();
                return Ok(
                    json!({"code":200,"data":rows.iter().skip(offset).take(200).cloned().collect::<Vec<_>>(),"hasMore":offset+200<rows.len(),"size":"1234","maxSize":if self.full.load(Ordering::SeqCst){"1234"}else{"1073741824"}}),
                );
            }
            Ok(match path {
                support::CHECK => {
                    *self.input.lock().unwrap() = (
                        payload["fileName"].as_str().unwrap().into(),
                        payload["length"].as_u64().unwrap(),
                    );
                    json!({"code":200,"needUpload":true,"songId":"private-upload-ticket-SECRET"})
                }
                support::ALLOC => {
                    let gate = self.allocation_gate.lock().unwrap().take();
                    if let Some(gate) = gate {
                        gate.entered.notify_one();
                        gate.release.acquire().await.unwrap().forget();
                    }
                    json!({"code":200,"data":support::allocation("audio",1)})
                }
                support::META => {
                    json!({"code":200,"songId":700,"exists":true,"privateCloud":{"songId":700}})
                }
                support::PUBLISH => {
                    let (name, size) = self.input.lock().unwrap().clone();
                    let mut value = row(700);
                    value["fileName"] = json!(name);
                    value["fileSize"] = json!(size);
                    self.rows.lock().unwrap().push(value);
                    json!({"code":201,"privateCloud":{"songId":700}})
                }
                "/api/cloud/del" => {
                    let ids: Vec<String> =
                        serde_json::from_str(payload["songIds"].as_str().unwrap()).unwrap();
                    self.rows
                        .lock()
                        .unwrap()
                        .retain(|r| !ids.contains(&r["songId"].to_string()));
                    json!({"code":200,"succIds":ids,"failIds":[]})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/audio.mp3?token=SECRET","md5":"abc","expi":120,"level":"standard"}]})
                }
                "/api/cloud/lyric/get" => {
                    json!({"code":200,"lrc":"[00:01]云盘歌词\n[00:02]第二行"})
                }
                _ => panic!("unexpected {path}"),
            })
        })
    }
}
fn row(id: u64) -> Value {
    json!({"songId":id,"fileName":format!("file-{id}.flac"),"fileSize":123,"addTime":id,"bitrate":900000,"simpleSong":{"id":999999,"name":format!("Song {id}"),"ar":[{"name":"Artist","token":"SECRET"}],"al":{"name":"Album","picUrl":"https://p1.music.126.net/image.jpg?token=SECRET"},"dt":120000,"cookie":"SECRET"}})
}
struct Fixture {
    root: tempfile::TempDir,
    api: Arc<Backend>,
    session: Arc<AccountSession>,
    objects: Arc<support::Objects>,
    oss: Arc<support::oss::Backend>,
    clock: Arc<support::Clock>,
}
impl Fixture {
    fn new(n: u64) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let api = Arc::new(Backend::default());
        *api.rows.lock().unwrap() = (1..=n).map(row).collect();
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        Self {
            root,
            api,
            session,
            objects: Arc::new(support::Objects::default()),
            oss: Arc::new(support::oss::Backend::default()),
            clock: Arc::new(support::Clock(AtomicU64::new(1000))),
        }
    }
    fn client(&self) -> MusicClient {
        MusicClient::with_session(
            self.session.clone(),
            ProfileRegistry::open(self.root.path(), ApiOrigin::Pc).unwrap(),
        )
        .with_cloud_adapters(UploadAdapters {
            nos: self.objects.clone(),
            oss: self.oss.clone(),
            probe: Arc::new(support::Probe),
            clock: self.clock.clone(),
            chunk_size: 3,
            oss_part_size: support::oss::PART,
            oss_concurrency: 3,
        })
        .with_audio_factory(|| {
            Ok(playback_support::audio_engine(Arc::new(
                playback_support::Device::default(),
            )))
        })
    }
    fn input(&self) -> CloudUploadInput {
        let path = self.root.path().join("input.flac");
        std::fs::write(&path, support::DATA).unwrap();
        CloudUploadInput {
            path,
            cover: None,
            lyric: None,
            cue: None,
        }
    }
}
fn query(tab: CloudTab) -> CloudQuery {
    CloudQuery {
        tab,
        keyword: String::new(),
        sort: CloudSort::Uploaded,
        descending: true,
        offset: 0,
        limit: 200,
    }
}
#[tokio::test]
async fn cloud_sorted_pages_and_playback_use_official_cloud_fields_and_stable_original_order() {
    let oracle: Value = serde_json::from_str(include_str!("fixtures/cloud_sort.json")).unwrap();
    let f = Fixture::new(0);
    let mut rows = oracle["rows"].as_array().unwrap().clone();
    for row in &mut rows {
        // Display metadata may differ from the user's original cloud tags.
        row["simpleSong"] = json!({"name":"Catalog display name","ar":[{"name":"Different artist"}],"al":{"name":"Different album"},"dt":120000});
    }
    *f.api.rows.lock().unwrap() = rows;
    let client = f.client();
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let revision = page.revision.unwrap();
    for expected in oracle["orders"].as_array().unwrap() {
        let mut q = query(CloudTab::Uploaded);
        q.sort = match expected["id"].as_str() {
            Some("trackName") => CloudSort::Title,
            Some("artist") => CloudSort::Artist,
            Some("albumName") => CloudSort::Album,
            Some("uploadTime") => CloudSort::Uploaded,
            Some("type") => CloudSort::Format,
            Some("size") => CloudSort::Size,
            _ => CloudSort::Natural,
        };
        q.descending = expected["flag"] == "reverse";
        let value = client.cloud_page("1", q.clone(), false).await.unwrap();
        assert_eq!(
            json!(value.tracks.iter().map(|r| &r.track.id).collect::<Vec<_>>()),
            expected["ids"],
            "{expected}"
        );
        // The page limit cannot truncate the queue, and the same artist order
        // must drive playback. Use a smaller visible slice for this assertion.
        if expected["id"] == "artist" && q.descending {
            q.limit = 3;
            client
                .play_selection(
                    "1",
                    PlaySelection::Cloud {
                        query: q,
                        revision: revision.clone(),
                        start_id: None,
                        quality: Quality::Standard,
                    },
                )
                .await
                .unwrap();
            let queue = client.player_queue("1").await.unwrap();
            assert_eq!(
                json!(queue.tracks.iter().map(|r| &r.track.id).collect::<Vec<_>>()),
                expected["ids"]
            );
            client.player_action("1", PlayerAction::Stop).await.unwrap();
        }
    }
}
#[tokio::test]
async fn partial_upload_selection_identifies_the_failed_path_when_basenames_match() {
    let f = Fixture::new(2);
    let client = f.client();
    let good = f.input();
    let bad_path = f.root.path().join("missing").join("input.flac");
    let bad = CloudUploadInput {
        path: bad_path.clone(),
        cover: None,
        lyric: None,
        cue: None,
    };
    let result = client
        .cloud_command(
            "1",
            CloudCommand::Upload {
                files: vec![good, bad],
            },
        )
        .await
        .unwrap();
    assert_eq!(result.accepted.len(), 1);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].id, bad_path.to_str().unwrap());
    let paths = client
        .select_cloud_sources("1", vec![f.root.path().into()])
        .await
        .unwrap();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("input.flac"));
}

#[tokio::test]
async fn an_active_pending_request_is_not_presented_as_an_abandoned_unknown_write() {
    let f = Fixture::new(2);
    let client = f.client();
    let gate = Arc::new(support::nos::Gate {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Semaphore::new(0),
    });
    *f.api.allocation_gate.lock().unwrap() = Some(gate.clone());
    client
        .cloud_command(
            "1",
            CloudCommand::Upload {
                files: vec![f.input()],
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
    let active = client
        .cloud_page("1", query(CloudTab::Uploading), false)
        .await
        .unwrap();
    assert_eq!(active.uploads[0].phase, CloudPhase::AllocationPending);
    assert!(active.uploads[0].running);
    assert!(active.uploads[0].error.is_none());
    assert_ne!(active.uploads[0].label, "等待核对");
    assert!(!active.uploads[0].can_resume);
    assert!(!active.uploads[0].can_cancel);
    gate.release.add_permits(1);
    wait_done(&client).await;
}

async fn wait_done(client: &MusicClient) -> CloudPageView {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let view = client
                .cloud_page("1", query(CloudTab::Uploading), false)
                .await
                .unwrap();
            if view.uploads.iter().any(|v| v.phase == CloudPhase::Complete) {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn full_cloud_blocks_new_upload_and_bulk_resume_before_any_remote_write_but_still_allows_reads()
 {
    let f = Fixture::new(2);
    f.api.full.store(true, Ordering::SeqCst);
    let client = f.client();
    let view = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    assert_eq!(view.size, view.max_size);
    assert_eq!(view.total, 2);
    assert!(matches!(
        client
            .cloud_command(
                "1",
                CloudCommand::Upload {
                    files: vec![f.input()]
                }
            )
            .await,
        Err(Error::State("cloud capacity exhausted"))
    ));
    assert!(matches!(
        client
            .cloud_command(
                "1",
                CloudCommand::ResumeAll {
                    retry_rejected: true
                }
            )
            .await,
        Err(Error::State("cloud capacity exhausted"))
    ));
    let tasks = client
        .cloud_page("1", query(CloudTab::Uploading), false)
        .await
        .unwrap();
    assert!(tasks.uploads.is_empty());
    assert!(
        f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|(path, _)| path == support::LIST)
    );
    assert!(matches!(
        client
            .select_cloud_sources("old", vec![f.root.path().into()])
            .await,
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn catalog_projection_filters_sorts_and_pages_without_disclosing_native_fields_or_refetching_on_poll()
 {
    let f = Fixture::new(205);
    let client = f.client();
    let first = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    assert_eq!(first.total, 205);
    assert_eq!(first.tracks.len(), 200);
    assert_eq!(first.tracks[0].track.id, "205");
    assert_eq!(first.max_size, Some(1073741824));
    let calls = f.api.calls.lock().unwrap().len();
    let mut q = query(CloudTab::Uploaded);
    q.offset = 200;
    let last = client.cloud_page("1", q, false).await.unwrap();
    assert_eq!(last.tracks.len(), 5);
    assert_eq!(last.tracks[4].track.id, "1");
    assert_eq!(f.api.calls.lock().unwrap().len(), calls);
    let mut q = query(CloudTab::Uploaded);
    q.keyword = "Song 15".into();
    q.sort = CloudSort::Title;
    q.descending = false;
    let filtered = client.cloud_page("1", q, false).await.unwrap();
    assert_eq!(filtered.total, 11);
    assert_eq!(filtered.tracks[0].track.id, "15");
    let text = serde_json::to_string(&first).unwrap();
    for secret in ["SECRET", "cookie", "resourceId", "upload-ticket"] {
        assert!(!text.contains(secret));
    }
}
#[tokio::test]
async fn cloud_play_selection_uses_full_filtered_order_and_cloud_lyrics_and_rejects_stale_catalog()
{
    let f = Fixture::new(205);
    let client = f.client();
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let selected = client
        .play_selection(
            "1",
            PlaySelection::Cloud {
                query: query(CloudTab::Uploaded),
                start_id: Some("150".into()),
                revision: page.revision.clone().unwrap(),
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    assert_eq!(selected.queue_len, 205);
    assert_eq!(selected.track.as_ref().unwrap().id, "150");
    assert_eq!(selected.actual_quality.as_deref(), Some("standard"));
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks[0].track.id, "205");
    assert_eq!(queue.tracks[204].track.id, "1");
    let play_id = selected.play_id.unwrap();
    let lyric = client.player_lyrics("1", &play_id).await.unwrap();
    assert_eq!(lyric.lines.len(), 2);
    client.lyric_offset("1", &play_id, 0.2).await.unwrap();
    assert!(
        f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(p, v)| p == "/api/cloud/lyric/get" && v["songId"] == "150")
    );
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(p, _)| p == "/api/v3/song/detail" || p == "/api/song/lyric/v1")
    );
    f.api.rows.lock().unwrap().pop();
    assert!(matches!(
        client
            .play_selection(
                "1",
                PlaySelection::Cloud {
                    query: query(CloudTab::Uploaded),
                    start_id: None,
                    revision: page.revision.unwrap(),
                    quality: Quality::Standard
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 205);
}
#[tokio::test]
async fn native_upload_worker_survives_navigation_pauses_reopens_and_resumes_then_deletes_only_selected_resource()
 {
    let f = Fixture::new(2);
    let client = f.client();
    let gate = support::nos::Gate::new();
    *f.objects.object("audio").gate.lock().unwrap() = Some(gate.clone());
    let created = client
        .cloud_command(
            "1",
            CloudCommand::Upload {
                files: vec![f.input()],
            },
        )
        .await
        .unwrap();
    let id = created.accepted[0].clone();
    tokio::time::timeout(Duration::from_secs(4), gate.entered.notified())
        .await
        .unwrap();
    client
        .cloud_command("1", CloudCommand::PauseAll)
        .await
        .unwrap();
    gate.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let v = client
                .cloud_page("1", query(CloudTab::Uploading), false)
                .await
                .unwrap();
            if !v.uploads[0].running {
                assert_eq!(v.uploads[0].bytes, 3);
                assert!(v.uploads[0].can_resume);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(client);
    let client = f.client();
    let recovered = client
        .cloud_page("1", query(CloudTab::Uploading), false)
        .await
        .unwrap();
    assert!(!recovered.uploads[0].enabled);
    assert_eq!(recovered.uploads[0].id, id);
    client
        .cloud_command(
            "1",
            CloudCommand::ResumeAll {
                retry_rejected: false,
            },
        )
        .await
        .unwrap();
    let done = wait_done(&client).await;
    assert!(done.uploads[0].can_dismiss);
    assert_eq!(
        f.objects.object("audio").state.lock().unwrap().data,
        support::DATA
    );
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    let deleted = client
        .cloud_command(
            "1",
            CloudCommand::Delete {
                ids: vec!["700".into()],
                revision: page.revision.unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(deleted.deletion.unwrap().removed, 1);
    client
        .cloud_command("1", CloudCommand::Dismiss { ids: vec![id] })
        .await
        .unwrap();
    assert!(
        client
            .cloud_page("1", query(CloudTab::Uploading), false)
            .await
            .unwrap()
            .uploads
            .is_empty()
    );
    assert_eq!(
        f.api
            .rows
            .lock()
            .unwrap()
            .iter()
            .map(|r| r["songId"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(
        f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == support::ALLOC)
            .count(),
        1
    );
}
#[tokio::test]
async fn late_list_and_old_commands_cannot_cross_relogin_or_account_boundaries() {
    let f = Fixture::new(1);
    let client = Arc::new(f.client());
    let gate = support::nos::Gate::new();
    *f.api.gate.lock().unwrap() = Some(gate.clone());
    let old = client.clone();
    let worker =
        tokio::spawn(async move { old.cloud_page("1", query(CloudTab::Uploaded), true).await });
    tokio::time::timeout(Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    gate.release.add_permits(1);
    assert!(matches!(worker.await.unwrap(), Err(Error::StaleResponse)));
    assert!(matches!(
        client
            .cloud_command(
                "1",
                CloudCommand::Upload {
                    files: vec![f.input()]
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    let other = client
        .cloud_page("2", query(CloudTab::Uploaded), false)
        .await
        .unwrap();
    assert_eq!(other.total, 0);
    assert!(other.revision.is_none());
    assert!(other.deletions.is_empty());
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(p, _)| p == support::CHECK)
    );
}
