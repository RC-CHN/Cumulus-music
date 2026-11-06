#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    membership::Quality,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Notify;

#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<String>>,
    hold_id: AtomicU64,
    hold_url_id: AtomicU64,
    deny_url_id: AtomicU64,
    playlist_cloud_song_id: AtomicU64,
    unavailable_url_id: AtomicU64,
    deny_voice: std::sync::atomic::AtomicBool,
    started: Notify,
    release: Notify,
    url_started: Notify,
    url_release: Notify,
    trial: Mutex<Option<Value>>,
    playlist_tracks: Mutex<Option<Vec<Value>>>,
    album_tracks: Mutex<Option<Vec<Value>>>,
    album_privileges: Mutex<Vec<Value>>,
    daily_tracks: Mutex<Option<Vec<Value>>>,
    style_tag_state: AtomicU64,
    style_tag_write_mode: AtomicU64,
    style_tag_writes: AtomicU64,
    style_tag_started: Notify,
    style_tag_after_ack: std::sync::atomic::AtomicBool,
    quality_trial_state: AtomicU64,
    quality_start_mode: AtomicU64,
    quality_writes: AtomicU64,
    quality_url_denied: AtomicU64,
    quality_standard_assignment: AtomicU64,
    quality_trial_source: std::sync::atomic::AtomicBool,
    quality_started: Notify,
    quality_release: Notify,
    vinyl_composer: vinyl_composer::Server,
    artist_cover: artist_search_cover::CoverFixture,
    vinyl_comment_group: AtomicU64,
    vinyl_recommendation_group: AtomicU64,
    vinyl_similar: vinyl_similar::Server,
    vinyl_related_playlists: vinyl_related_playlists::Server,
    vinyl_comment_calls: Mutex<Vec<(String, Value)>>,
    vinyl_comment_hold: AtomicU64,
    vinyl_comment_started: Notify,
    vinyl_comment_release: Notify,
}
impl Api for Backend {
    fn verified_pc_experiment_revision(&self) -> cumulus_core::Result<Option<String>> {
        if self.artist_cover.is_enabled() {
            return Ok(self.artist_cover.revision());
        }
        let group = self.vinyl_comment_group.load(Ordering::SeqCst);
        Ok((group != 0).then(|| group.to_string()))
    }
    fn verified_pc_experiment_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, cumulus_core::protocol::pc_experiments::PcExperimentGroups> {
        Box::pin(async move {
            if self.artist_cover.is_enabled() {
                return self.artist_cover.groups(revision);
            }
            let group = if matches!(revision, "1" | "3") {
                "t1"
            } else {
                "c"
            };
            let recommendation = match self.vinyl_recommendation_group.load(Ordering::SeqCst) {
                0 => json!(group),
                1 => json!("t1"),
                2 => json!("c"),
                3 => json!("t2"),
                4 => Value::Null,
                5 => json!(""),
                _ => unreachable!(),
            };
            cumulus_core::protocol::pc_experiments::PcExperimentGroups::refreshed(&json!({
                "data":[{"expName":"PH-PC-optimizeComment","expGroupName":group},
                    {"expName":"PH-PC-playPage-recommendMore-new","expGroupName":recommendation}]
            }))
        })
    }
    fn request_vinyl_similar_songs<'a>(
        &'a self,
        read: cumulus_core::protocol::api::VinylSimilarSongsRead,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.verified_pc_experiment_revision()?.as_deref() != Some(revision) {
                return Err(Error::CredentialChanged);
            }
            self.vinyl_similar.response(read).await
        })
    }

    fn request_vinyl_related_playlists<'a>(
        &'a self,
        read: cumulus_core::protocol::api::VinylRelatedPlaylistsRead,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.verified_pc_experiment_revision()?.as_deref() != Some(revision) {
                return Err(Error::CredentialChanged);
            }
            self.vinyl_related_playlists.response(read).await
        })
    }

    fn request_vinyl_comment_page<'a>(
        &'a self,
        read: cumulus_core::protocol::api::VinylCommentRead,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.verified_pc_experiment_revision()?.as_deref() != Some(revision) {
                return Err(Error::CredentialChanged);
            }
            let (path, args) = read.into_request();
            if args["limit"] == 1 {
                assert_eq!(path, "/api/v1/resource/comments/R_SO_4_1");
                assert_eq!(
                    args,
                    json!({"limit":1,"offset":0,"beforeTime":0,"commentId":args["commentId"],
                    "compareUserLocation":false,"composeConcert":false,"markReplied":false,
                    "forceFlatComment":false,"showInner":false})
                );
                self.vinyl_composer
                    .scoped_like_reads
                    .fetch_add(1, Ordering::SeqCst);
            }
            self.request(&path, args, false).await
        })
    }

    fn request_vinyl_comment_write<'a>(
        &'a self,
        write: cumulus_core::protocol::api::VinylCommentWrite,
        revision: &'a str,
        permission: &'a cumulus_core::protocol::api::VinylCommentWritePermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.vinyl_composer.wait_dispatch().await;
            if self.verified_pc_experiment_revision()?.as_deref() != Some(revision) {
                return Err(Error::RequestNotSent(Box::new(Error::CredentialChanged)));
            }
            permission().map_err(|error| Error::RequestNotSent(Box::new(error)))?;
            let (path, args) = write.into_request();
            self.request(path, args, false).await
        })
    }

    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(path.into());
            if let Some(value) = self.vinyl_composer.response(path, &payload).await {
                assert!(!cache);
                return value;
            }
            if let Some(response) = self.artist_cover.request(path, &payload, cache).await {
                return response;
            }
            Ok(match path {
                "/api/v1/resource/comments/R_SO_4_1" => {
                    assert!(!cache);
                    self.vinyl_comment_calls
                        .lock()
                        .unwrap()
                        .push((path.into(), payload.clone()));
                    if self.vinyl_comment_hold.swap(0, Ordering::SeqCst) == 1 {
                        self.vinyl_comment_started.notify_one();
                        self.vinyl_comment_release.notified().await;
                    }
                    let first = payload["offset"] == 0;
                    let row = |id: u32| json!({"commentId":id,"content":"合成评论","time":1700000000123_u64,"liked":false,"likedCount":2});
                    let comments = if first {
                        vec![row(1), row(2)]
                    } else {
                        vec![row(3)]
                    };
                    json!({"code":200,"comments":comments,"hotComments":(4..10).map(row).collect::<Vec<_>>(),
                           "topComments":[row(10)],"total":3,"more":first,"moreHot":true})
                }
                "/api/search/song/list/page" => search_queue_drop::response(&payload),
                "/api/w/nuser/account/get" => json!({"code":200,"profile":{"userId":9}}),
                "/api/rtrs/abt/front/expinfo/list" => {
                    assert_eq!(payload, json!({"expNames":"StandardAudio_VIPLogo_PCV2"}));
                    let group = if self.quality_standard_assignment.load(Ordering::SeqCst) == 1 {
                        "t1"
                    } else {
                        "t2"
                    };
                    json!({"code":200,"data":[{"expName":"StandardAudio_VIPLogo_PCV2","expGroupName":group}]})
                }
                "/api/dj/program/detail" => {
                    if payload["id"].as_str().and_then(|id| id.parse::<u64>().ok())
                        == Some(self.hold_id.load(Ordering::SeqCst))
                    {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    json!({"code":200,"program":{"id":payload["id"],"name":"声音","mainTrackId":181,"duration":120000}})
                }
                "/api/dj/play/url/check" => {
                    json!({"code":200,"data":{"voiceId":payload["voiceId"],"songId":181,"canPlay":!self.deny_voice.load(Ordering::SeqCst)}})
                }
                "/api/v2/artist/songs" => {
                    let offset = payload["offset"].as_u64().unwrap();
                    let limit = payload["limit"].as_u64().unwrap();
                    json!({"code":200,"total":55,"more":offset+limit<55,"songs":((offset+1)..=(offset+limit).min(55)).map(|id|json!({"id":id,"name":"曲目"})).collect::<Vec<_>>()})
                }
                "/api/artist/v3/detail" => {
                    json!({"code":200,"artist":{"id":payload["id"],"name":"已核对的歌手页"},"hotSongs":[{"id":1,"name":"曲目"}]})
                }
                "/api/album/detail/dynamic" => json!({"code":200,"isSub":false}),
                "/api/album/privilege" => {
                    json!({"code":200,"data":self.album_privileges.lock().unwrap().clone()})
                }
                "/api/resource-exposure/config" => {
                    json!({"code":200,"data":{"resourceType":"other"}})
                }
                "/api/album/v3/detail" => {
                    json!({"code":200,"album":{"id":7,"name":"长专辑"},"songs":self.album_tracks.lock().unwrap().clone().unwrap_or_else(||(1..=501).map(|id|json!({"id":id,"name":"曲目","dt":120000})).collect::<Vec<_>>())})
                }
                "/api/v6/playlist/detail" => {
                    json!({"code":200,"playlist":{"id":payload["id"],"name":"已核对的歌单","trackIds":self.playlist_tracks.lock().unwrap().clone().unwrap_or_else(|| (1..=205).map(|id|json!({"id":id})).collect::<Vec<_>>())}})
                }
                "/api/homepage/category/daily/song/list" => {
                    let expected = if self.style_tag_after_ack.swap(false, Ordering::SeqCst) {
                        json!({})
                    } else {
                        json!({"source":"","categoryId":"","tagId":"","songId":""})
                    };
                    assert_eq!(payload, expected);
                    assert!(!cache);
                    let mut response = json!({"code":200,"data":{"dailySongs":[{"id":72,"name":"风格乙"},{"id":71,"name":"风格甲"}]}});
                    match self.style_tag_state.load(Ordering::SeqCst) {
                        0 => {}
                        n => {
                            response["data"]["tags"] = json!({"categoryId":7,"categoryName":"类型","tagVOList":[{"tagId":n,"tagName":"标签"}]})
                        }
                    }
                    response
                }
                "/api/homepage/daily/song/config/get" => {
                    assert!(!cache);
                    assert_eq!(payload, json!({"limit":20}));
                    json!({"code":200,"data":{"categorys":[{"categoryId":7,"categoryName":"类型","tagVOList":[{"tagId":1,"tagName":"甲"},{"tagId":2,"tagName":"乙"}]}]}})
                }
                "/api/homepage/daily/song/tag/save" => {
                    assert!(!cache);
                    assert_eq!(payload, json!({"tags":{"categoryId":7,"tagIds":[2]}}));
                    self.style_tag_writes.fetch_add(1, Ordering::SeqCst);
                    match self.style_tag_write_mode.load(Ordering::SeqCst) {
                        1 => {
                            self.style_tag_state.store(2, Ordering::SeqCst);
                            self.style_tag_started.notify_one();
                            std::future::pending::<()>().await;
                        }
                        2 => {}
                        _ => self.style_tag_state.store(2, Ordering::SeqCst),
                    }
                    self.style_tag_after_ack.store(true, Ordering::SeqCst);
                    json!({"code":200})
                }
                "/api/v3/discovery/recommend/songs" => {
                    assert_eq!(payload, json!({"limit":30}));
                    let tracks = self
                        .daily_tracks
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(|| {
                            vec![
                                json!({"id":74,"name":"日推乙"}),
                                json!({"id":73,"name":"日推甲"}),
                            ]
                        });
                    json!({"code":200,"data":{"dailySongs":tracks}})
                }
                "/api/v3/song/detail" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["c"].as_str().unwrap()).unwrap();
                    let id = ids[0]["id"].as_str().unwrap().parse::<u64>().unwrap();
                    if self.hold_id.load(Ordering::SeqCst) == id {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    let mut response = json!({"code":200,"songs":ids.iter().map(|v|json!({"id":v["id"],"name":"曲目","ar":[{"id":12,"name":"歌手"}],"al":{"id":7,"name":"专辑","picUrl":"http://p1.music.126.net/image.jpg?token=SECRET"},"dt":120000,"url":"https://private/?secret=SECRET","cookie":"SECRET"})).collect::<Vec<_>>()});
                    if id == self.playlist_cloud_song_id.load(Ordering::SeqCst) {
                        response["privileges"] = json!([{"id":id,"cs":1}]);
                    }
                    response
                }
                "/api/song/enhance/player/url/v1" => {
                    *self.trial.lock().unwrap() = Some(payload["trialMode"].clone());
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    if ids[0].as_u64().or_else(|| ids[0].as_str()?.parse().ok())
                        == Some(self.hold_url_id.load(Ordering::SeqCst))
                    {
                        self.url_started.notify_one();
                        self.url_release.notified().await;
                    }
                    if ids[0].as_u64().or_else(|| ids[0].as_str()?.parse().ok())
                        == Some(self.deny_url_id.load(Ordering::SeqCst))
                    {
                        return Ok(json!({"code":200,"data":[{"id":ids[0],"code":403,
                            "url":"https://secret.invalid/denied?token=SECRET","md5":"SECRET"}]}));
                    }
                    if ids[0].as_u64().or_else(|| ids[0].as_str()?.parse().ok())
                        == Some(self.unavailable_url_id.load(Ordering::SeqCst))
                    {
                        return Ok(json!({"code":200,"data":[{"id":ids[0],"code":"503",
                            "url":"https://secret.invalid/transient?token=SECRET","md5":"SECRET"}]}));
                    }
                    json!({"code":200,"data":[{"id":ids[0],"code":if payload["level"]=="jymaster"&&self.quality_url_denied.load(Ordering::SeqCst)>0 {403}else{200},"url":"https://m1.music.126.net/a.mp3?key=SECRET","md5":"abc","expi":120,"level":"standard",
                        "freeTrialInfo": if self.quality_trial_source.load(Ordering::SeqCst) {json!({"start":20,"end":40})} else {Value::Null}
                    }]})
                }
                "/api/song/enhance/privilege" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":ids.iter().map(|id|json!({"id":id,"flag":(1<<12)|(1<<16)|(1<<17)|(1<<18)|(1<<21),"maxbr":999000,"fl":128000,"payed":0,"fee":0})).collect::<Vec<_>>()})
                }
                "/api/song/music/detail/get" => {
                    json!({"code":200,"data":{"id":payload["songId"],"sq":{"size":1234},"url":"https://private/?SECRET"}})
                }
                "/api/music-vip-membership/client/vip/info" => {
                    json!({"code":200,"data":{"userId":9,"now":1000000,"associator":{"expireTime":2000000},"cookie":"SECRET"}})
                }
                "/api/vipauth/app/auth/query" => {
                    let state = self.quality_trial_state.load(Ordering::SeqCst);
                    json!({"code":200,"data":[{"resourceId":0,"resourceType":payload["resourceTypeList"],"canUse":state!=2,"userSrc":0,"trialStatus":if state==1 {2}else{1},"leftTime":60000,"cookie":"SECRET"}]})
                }
                "/api/vipauth/app/auth/set" => {
                    self.quality_writes.fetch_add(1, Ordering::SeqCst);
                    self.quality_started.notify_one();
                    let mode = self.quality_start_mode.load(Ordering::SeqCst);
                    if mode == 1 {
                        self.quality_release.notified().await;
                    }
                    self.quality_trial_state.store(1, Ordering::SeqCst);
                    if mode == 2 {
                        return Err(Error::State("synthetic lost activation response"));
                    }
                    json!({"code":200,"data":true})
                }
                "/api/song/lyric/v1" => {
                    json!({"code":200,"lrc":{"lyric":"[00:01]第一行\n[00:03]第二行","version":1},"yrc":{"lyric":"[1000,1000](1000,500,0)你(1500,500,0)好","version":1},"ytlrc":{"lyric":"[1000,1000]hello"},"yromalrc":{"lyric":"[1000,1000]ni hao"},"lyricContributor":{"cookie":"SECRET"}})
                }
                _ => panic!("unexpected {path}"),
            })
        })
    }
}
type PlayerFixture = (
    tempfile::TempDir,
    Arc<Backend>,
    Arc<AccountSession>,
    Arc<MusicClient>,
    Arc<AtomicU64>,
);
fn fixture() -> PlayerFixture {
    fixture_with_device(Arc::new(support::Device::default()))
}
fn fixture_with_device(device: Arc<support::Device>) -> PlayerFixture {
    let dir = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let count = Arc::new(AtomicU64::new(0));
    let calls = count.clone();
    let client =
        MusicClient::with_session(session.clone(), registry).with_audio_factory(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(support::audio_engine(device.clone()))
        });
    (dir, api, session, Arc::new(client), count)
}
fn songs(id: &str) -> PlaySelection {
    PlaySelection::Songs {
        ids: vec![id.into()],
        index: 0,
        quality: Quality::Lossless,
    }
}

#[path = "client_player/artist_search_cover.rs"]
mod artist_search_cover;

#[path = "client_player/prepared_selection.rs"]
mod prepared_selection;

#[path = "client_player/voice_selection.rs"]
mod voice_selection;

#[path = "client_player/vinyl_comments.rs"]
mod vinyl_comments;

#[path = "client_player/vinyl_related_playlists.rs"]
mod vinyl_related_playlists;

#[path = "client_player/vinyl_similar.rs"]
mod vinyl_similar;

#[path = "client_player/vinyl_composer.rs"]
mod vinyl_composer;

#[path = "client_player/search_queue_drop.rs"]
mod search_queue_drop;

#[path = "client_player/queue_click.rs"]
mod queue_click;

#[path = "client_player/album_duration_sort.rs"]
mod album_duration_sort;
#[path = "client_player/album_queue_drop.rs"]
mod album_queue_drop;
#[path = "client_player/queue_album.rs"]
mod queue_album;
#[path = "client_player/queue_daily.rs"]
mod queue_daily;
#[path = "client_player/source_control.rs"]
mod source_control;
#[path = "client_player/style_tag_intent.rs"]
mod style_tag_intent;

#[tokio::test]
async fn rejected_online_selection_projects_no_source_and_allows_a_different_song() {
    use cumulus_core::{client::ClientError, playback::controller::PlaybackStatus};
    let (_dir, api, _session, client, _) = fixture();
    api.deny_url_id.store(1, Ordering::SeqCst);
    let denied = client.play_selection("1", songs("1")).await.err().unwrap();
    let denial = ClientError::from(denied);
    assert_eq!(denial.code, "playback_source_unavailable");
    assert!(!denial.retryable);
    let stopped = client.player_snapshot("1").await.unwrap();
    assert_eq!(stopped.state, PlaybackStatus::Error);
    assert_eq!(stopped.error, Some(denial.message));
    assert_eq!(stopped.error_retryable, Some(false));
    for public in [
        serde_json::to_string(&denial).unwrap(),
        serde_json::to_string(&stopped).unwrap(),
    ] {
        assert!(!public.contains("secret.invalid"));
        assert!(!public.contains("SECRET"));
    }
    let playing = client.play_selection("1", songs("2")).await.unwrap();
    assert_eq!(playing.state, PlaybackStatus::Playing);
    assert!(playing.error.is_none());
}

#[tokio::test]
async fn a_temporary_url_service_failure_keeps_the_queue_for_an_explicit_same_song_retry() {
    use cumulus_core::{client::ClientError, playback::controller::PlaybackStatus};
    let (_dir, api, _session, client, _) = fixture();
    api.unavailable_url_id.store(1, Ordering::SeqCst);
    let error = client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into()],
                index: 0,
                quality: Quality::Lossless,
            },
        )
        .await
        .err()
        .unwrap();
    let diagnostic = ClientError::from(error);
    assert_eq!(diagnostic.code, "playback_source_temporary");
    assert!(diagnostic.retryable);
    let failed = client.player_snapshot("1").await.unwrap();
    assert_eq!(failed.state, PlaybackStatus::Error);
    assert_eq!(failed.error_code, Some(diagnostic.code));
    assert_eq!(failed.error_retryable, Some(true));
    assert_eq!(failed.track.as_ref().unwrap().id, "1");
    assert_eq!(failed.queue_len, 2);
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.as_str() == "/api/song/enhance/player/url/v1")
            .count(),
        1,
        "the failed selection does not retry or skip on its own"
    );
    api.unavailable_url_id.store(0, Ordering::SeqCst);
    let restored = client
        .player_action(
            "1",
            PlayerAction::RefreshFor {
                play_id: failed.play_id.unwrap(),
                expected_queue_revision: failed.queue_revision.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(restored.state, PlaybackStatus::Playing);
    assert_eq!(restored.track.as_ref().unwrap().id, "1");
    assert_eq!(restored.entry_id, failed.entry_id);
    assert_eq!(restored.queue_revision, failed.queue_revision);
    assert!(restored.error.is_none());
}

#[tokio::test]
async fn displayed_removal_resumes_only_when_it_removes_the_paused_current_song() {
    use cumulus_core::playback::controller::PlaybackStatus;
    let (_dir, _api, _session, client, _) = fixture();
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into(), "3".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    let remove = |revision: String, id: String| PlayerAction::QueueFor {
        expected_queue_revision: revision,
        expected_room_id: None,
        edit: QueueEdit::Remove {
            entry_ids: vec![id],
        },
    };
    let edited = client
        .player_action(
            "1",
            remove(queue.revision, queue.tracks[2].entry_id.clone()),
        )
        .await
        .unwrap();
    assert_eq!(edited.state, PlaybackStatus::Paused);
    assert_eq!(edited.play_id, paused.play_id);
    client
        .player_action("1", remove(edited.queue_revision, paused.entry_id.unwrap()))
        .await
        .unwrap();
    let next = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let state = client.player_snapshot("1").await.unwrap();
            if state.state == PlaybackStatus::Playing {
                break state;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(next.track.unwrap().id, "2");
    assert_eq!(next.queue_len, 1);
    assert_ne!(next.play_id, paused.play_id);
}

#[tokio::test]
async fn authorized_media_and_output_failures_keep_distinct_action_and_snapshot_diagnostics() {
    use cumulus_core::{client::ClientError, playback::controller::PlaybackStatus};
    let device = Arc::new(support::Device::default());
    let (_dir, api, _session, client, _) = fixture_with_device(device.clone());
    for (code, public) in [(-15, "audio_open_failed"), (-14, "audio_output_failed")] {
        device.load_failure_code.store(code, Ordering::SeqCst);
        let error = client.play_selection("1", songs("1")).await.err().unwrap();
        let diagnostic = ClientError::from(error);
        assert_eq!(diagnostic.code, public);
        assert!(diagnostic.retryable);
        let state = client.player_snapshot("1").await.unwrap();
        assert_eq!(state.state, PlaybackStatus::Error);
        assert_eq!(state.error, Some(diagnostic.message));
        assert_eq!(state.error_code, Some(public));
        assert_eq!(state.error_retryable, Some(true));
        assert!(
            !serde_json::to_string(&state)
                .unwrap()
                .contains("secret.invalid")
        );
    }
    assert!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call.ends_with("/player/url/v1"))
    );
    device.load_failure_code.store(0, Ordering::SeqCst);
    let playing = client.play_selection("1", songs("2")).await.unwrap();
    assert_eq!(playing.state, PlaybackStatus::Playing);
    assert!(playing.error_code.is_none());
    assert_eq!(playing.error_retryable, None);
}

#[tokio::test]
async fn quality_menu_target_and_preference_revision_are_checked_before_requesting_a_url() {
    let (_dir, api, _session, client, _) = fixture();
    let first = client.play_selection("1", songs("1")).await.unwrap();
    let old_play = first.play_id.unwrap();
    let initial = first.quality_preferences.revision.unwrap();
    let current = client.play_selection("1", songs("2")).await.unwrap();
    let before = api.calls.lock().unwrap().len();
    let old = client
        .player_action(
            "1",
            PlayerAction::QualityFor {
                play_id: old_play,
                expected_revision: initial.clone(),
                level: Quality::HiRes,
            },
        )
        .await;
    assert!(matches!(old, Err(Error::StaleResponse)));
    assert_eq!(api.calls.lock().unwrap().len(), before);
    let changed = client
        .player_action(
            "1",
            PlayerAction::QualityFor {
                play_id: current.play_id.unwrap(),
                expected_revision: initial.clone(),
                level: Quality::HiRes,
            },
        )
        .await
        .unwrap();
    assert_eq!(changed.requested_quality.as_deref(), Some("hires"));
    assert_eq!(changed.actual_quality.as_deref(), Some("standard"));
    assert_eq!(changed.quality_preferences.quality, Quality::HiRes);
    let before = api.calls.lock().unwrap().len();
    assert!(matches!(
        client
            .player_action(
                "1",
                PlayerAction::QualityFor {
                    play_id: changed.play_id.unwrap(),
                    expected_revision: initial,
                    level: Quality::ExHigh,
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), before);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn rejecting_an_old_quality_menu_does_not_cancel_a_newer_pending_song_selection() {
    let (_dir, api, _session, client, _) = fixture();
    let first = client.play_selection("1", songs("1")).await.unwrap();
    client.play_selection("1", songs("2")).await.unwrap();
    api.hold_id.store(3, Ordering::SeqCst);
    let next = client.clone();
    let pending = tokio::spawn(async move { next.play_selection("1", songs("3")).await });
    api.started.notified().await;
    assert!(matches!(
        client
            .player_action(
                "1",
                PlayerAction::QualityFor {
                    play_id: first.play_id.unwrap(),
                    expected_revision: first.quality_preferences.revision.unwrap(),
                    level: Quality::HiRes,
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    api.release.notify_waiters();
    assert_eq!(pending.await.unwrap().unwrap().track.unwrap().id, "3");
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn local_startup_preferences_need_no_engine_or_http_and_reject_old_account_writes() {
    use cumulus_core::client::dto::StartupPreferencesCommand as Change;
    use cumulus_core::playback::preferences::StartupSettings;
    let (_dir, api, session, client, count) = fixture();
    let initial = client.startup_preferences("1").unwrap();
    assert_eq!(initial.value.settings, StartupSettings::default());
    let saved = client
        .startup_preferences_command(
            "1",
            Change::Change {
                expected_revision: initial.value.revision.unwrap(),
                settings: StartupSettings {
                    auto_play: true,
                    remember_position: false,
                },
            },
        )
        .unwrap();
    assert_eq!(saved.value.revision.as_deref(), Some("1"));
    assert!(
        client
            .startup_preferences_command(
                "1",
                Change::Change {
                    expected_revision: "0".into(),
                    settings: StartupSettings::default()
                }
            )
            .is_err()
    );
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.startup_preferences_command("1", Change::Repair),
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        client.startup_preferences("2").unwrap().value.settings,
        saved.value.settings
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn provisional_player_preserves_untouched_autoplay_only_for_first_authoritative_restore() {
    use cumulus_core::playback::{
        PlaybackQueue, controller::PlaybackStatus, preferences::StartupSettings,
    };
    for (interactive, guest_stopped, expected) in [
        (false, false, PlaybackStatus::Playing),
        (false, true, PlaybackStatus::Paused),
        (true, false, PlaybackStatus::Paused),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let api = Arc::new(Backend::default());
        let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        let client = MusicClient::with_session(session.clone(), registry)
            .with_audio_factory(|| Ok(support::audio_engine(Arc::new(support::Device::default()))));
        let mut queue = PlaybackQueue::new(Some(store), "9").unwrap();
        queue
            .replace(vec![json!({"id":1,"dt":120000})], 0, json!({"kind":"test"}))
            .unwrap();
        client
            .startup_preferences_command(
                "0",
                StartupPreferencesCommand::Change {
                    expected_revision: "0".into(),
                    settings: StartupSettings {
                        auto_play: true,
                        remember_position: false,
                    },
                },
            )
            .unwrap();
        let provisional = client.subscribe_playback("0").await.unwrap();
        assert_eq!(provisional.current().unwrap().state, PlaybackStatus::Idle);
        if guest_stopped {
            client.player_action("0", PlayerAction::Stop).await.unwrap();
        }
        if interactive {
            session
                .adopt(api, json!({"userId":9}), &session.token().unwrap())
                .unwrap();
        } else {
            client.restore_account().await.unwrap();
        }
        let restored = client
            .player_action("1", PlayerAction::Startup)
            .await
            .unwrap();
        assert_eq!(restored.state, expected);
        assert_eq!(restored.position, Some(0.0));
        assert_eq!(
            client
                .player_action("1", PlayerAction::Startup)
                .await
                .unwrap()
                .play_id,
            restored.play_id
        );
    }
}

#[tokio::test]
async fn public_startup_claims_audio_once_and_damaged_preferences_allow_manual_play_and_explicit_repair()
 {
    use cumulus_core::client::dto::StartupPreferencesCommand as Change;
    use cumulus_core::{
        playback::{PlaybackQueue, preferences::StartupSettings},
        storage::StateStore,
    };
    let (dir, _api, session, client, _count) = fixture();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let mut queue = PlaybackQueue::new(Some(store.clone()), "9").unwrap();
    queue
        .replace(vec![json!({"id":1,"dt":120000})], 0, json!({"kind":"test"}))
        .unwrap();
    store.put("9", "playback", "position", &json!({"entry_id":queue.selected().unwrap().entry_id,"seconds":17.0,"state":"paused","timeline_version":1})).unwrap();
    client
        .startup_preferences_command(
            "1",
            Change::Change {
                expected_revision: "0".into(),
                settings: StartupSettings {
                    auto_play: true,
                    remember_position: true,
                },
            },
        )
        .unwrap();
    let playing = client
        .player_action("1", PlayerAction::Startup)
        .await
        .unwrap();
    assert_eq!(
        playing.state,
        cumulus_core::playback::controller::PlaybackStatus::Playing
    );
    assert_eq!(playing.position, Some(17.0));
    assert_eq!(
        client
            .player_action("1", PlayerAction::Startup)
            .await
            .unwrap()
            .play_id,
        playing.play_id
    );
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    assert_eq!(
        client
            .player_action("1", PlayerAction::Startup)
            .await
            .unwrap()
            .state,
        cumulus_core::playback::controller::PlaybackStatus::Idle
    );
    store
        .put(
            "local-device",
            "playback-startup-preferences-v1",
            "current",
            &json!({"damaged":true}),
        )
        .unwrap();
    let reopened = MusicClient::with_session(
        session,
        ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap(),
    )
    .with_audio_factory(|| Ok(support::audio_engine(Arc::new(support::Device::default()))));
    let damaged = reopened.startup_preferences("1").unwrap().value;
    assert!(damaged.damaged && damaged.revision.is_none());
    assert!(matches!(
        reopened.player_action("1", PlayerAction::Startup).await,
        Err(Error::State("playback settings damaged"))
    ));
    let repaired = reopened
        .startup_preferences_command("1", Change::Repair)
        .unwrap()
        .value;
    assert!(!repaired.damaged);
    assert_eq!(repaired.settings, StartupSettings::default());
    reopened
        .startup_preferences_command(
            "1",
            Change::Change {
                expected_revision: repaired.revision.unwrap(),
                settings: StartupSettings {
                    auto_play: true,
                    remember_position: true,
                },
            },
        )
        .unwrap();
    // Changing preferences after a failed initial attempt still takes effect on
    // the next process launch, not a page reload in this process.
    assert_eq!(
        reopened
            .player_action("1", PlayerAction::Startup)
            .await
            .unwrap()
            .state,
        cumulus_core::playback::controller::PlaybackStatus::Idle
    );
    assert_eq!(
        reopened
            .player_action("1", PlayerAction::Restore)
            .await
            .unwrap()
            .state,
        cumulus_core::playback::controller::PlaybackStatus::Paused
    );
}

#[tokio::test]
async fn album_selection_uses_complete_native_order_and_rejects_changed_revision() {
    let (_dir, _api, session, client, _count) = fixture();
    let page = client
        .ranking_page("1", RankingQuery::Album { id: "7".into() }, true)
        .await
        .unwrap();
    let RankingPageView::Album {
        revision, tracks, ..
    } = page.value.page
    else {
        panic!()
    };
    assert_eq!(tracks.len(), 501);
    let state = client
        .play_selection(
            "1",
            PlaySelection::Album {
                id: "7".into(),
                start_id: Some("501".into()),
                revision,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.queue_len, 501);
    assert!(state.history_warning.is_none());
    let history = cumulus_core::history::HistoryService::new(session.clone()).unwrap();
    let token = session.token().unwrap();
    assert_eq!(
        history
            .snapshot_for(&token, cumulus_core::history::HistoryKind::Song)
            .unwrap()
            .local[0]
            .id,
        "501"
    );
    assert_eq!(
        history
            .snapshot_for(&token, cumulus_core::history::HistoryKind::Album)
            .unwrap()
            .local[0]
            .id,
        "7"
    );
    assert_eq!(state.track.unwrap().id, "501");
    assert!(matches!(
        client
            .play_selection(
                "1",
                PlaySelection::Album {
                    id: "7".into(),
                    start_id: None,
                    revision: "changed".into(),
                    quality: Quality::Standard
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 501);
    assert_eq!(queue.tracks.last().unwrap().track.id, "501");
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn complete_playlist_is_native_and_public_player_queue_lyrics_are_whitelisted() {
    let (dir, api, session, client, count) = fixture();
    let mut subscription = client.subscribe_playback("1").await.unwrap();
    let view = client
        .play_selection(
            "1",
            PlaySelection::Playlist {
                id: "7".into(),
                start_id: Some("151".into()),
                revision: None,
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.queue_len, 205);
    assert!(view.history_warning.is_none());
    let history = cumulus_core::history::HistoryService::new(session.clone()).unwrap();
    let playlist = history
        .snapshot_for(
            &session.token().unwrap(),
            cumulus_core::history::HistoryKind::Playlist,
        )
        .unwrap();
    assert_eq!(playlist.local[0].id, "7");
    assert_eq!(view.track.as_ref().unwrap().id, "151");
    assert_eq!(view.requested_quality, Some("lossless".into()));
    assert_eq!(view.actual_quality, Some("standard".into()));
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|s| *s == "/api/v3/song/detail")
            .count(),
        3
    );
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 205);
    assert_eq!(queue.tracks[204].track.id, "205");
    let id = view.play_id.as_ref().unwrap();
    let lyric = client.player_lyrics("1", id).await.unwrap();
    assert_eq!(lyric.lines.len(), 2);
    assert_eq!(lyric.word_lines[0].words.len(), 2);
    assert_eq!(lyric.word_translation[0].lyric.trim(), "hello");
    assert_eq!(lyric.word_romanization[0].lyric.trim(), "ni hao");
    let changed = client.lyric_offset("1", id, 1.0).await.unwrap();
    assert_eq!(changed.offset, 0.5);
    assert_eq!(changed.lines[0].time, Some(0.5));
    for value in [
        serde_json::to_value(view).unwrap(),
        serde_json::to_value(queue).unwrap(),
        serde_json::to_value(lyric).unwrap(),
    ] {
        let text = value.to_string();
        assert!(!text.contains("SECRET"));
        assert!(!text.contains("cookie"));
        assert!(!text.contains("resource_info"));
    }
    let stored = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    assert!(!stored.to_string().contains("SECRET"));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let updated = subscription.changed().await.unwrap();
    assert_eq!(
        updated.state,
        cumulus_core::playback::controller::PlaybackStatus::Paused
    );
}

#[tokio::test]
async fn stale_inputs_and_unknown_native_fields_never_initialize_audio_or_access_network() {
    let (_dir, api, session, client, count) = fixture();
    assert!(matches!(
        client.play_selection("0", songs("1")).await.err().unwrap(),
        Error::StaleResponse
    ));
    assert!(matches!(
        client
            .player_action("0", PlayerAction::Play)
            .await
            .err()
            .unwrap(),
        Error::StaleResponse
    ));
    assert!(serde_json::from_value::<PlaySelection>(json!({"kind":"songs","ids":["1"],"index":0,"quality":"standard","url":"https://private"})).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(api.calls.lock().unwrap().is_empty());
    let stream = client.subscribe_playback("1").await.unwrap();
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        stream.current().err().unwrap(),
        Error::StaleResponse
    ));
}

#[tokio::test]
async fn slow_metadata_cannot_replace_new_selection_and_old_lyrics_cannot_adjust_new_song() {
    let (_dir, api, _session, client, _) = fixture();
    api.hold_id.store(1, Ordering::SeqCst);
    let old_client = client.clone();
    let old = tokio::spawn(async move { old_client.play_selection("1", songs("1")).await });
    api.started.notified().await;
    let next = client.play_selection("1", songs("2")).await.unwrap();
    api.release.notify_waiters();
    assert!(matches!(
        old.await.unwrap().err().unwrap(),
        Error::StaleResponse
    ));
    let play_id = next.play_id.unwrap();
    client.player_lyrics("1", &play_id).await.unwrap();
    client.play_selection("1", songs("3")).await.unwrap();
    assert!(matches!(
        client.lyric_offset("1", &play_id, 1.0).await.err().unwrap(),
        Error::StaleResponse
    ));
    assert_eq!(
        client.player_queue("1").await.unwrap().tracks[0].track.id,
        "3"
    );
}

#[tokio::test]
async fn catalog_selection_keeps_loaded_artist_order_source_and_navigation_references() {
    let (dir, api, _session, client, _count) = fixture();
    let mut pages = Vec::new();
    for offset in [0, 20] {
        let query = CatalogQuery::ArtistSongs {
            id: "12".into(),
            offset,
            limit: 20,
        };
        let saved = client
            .catalog_page("1", query.clone(), true)
            .await
            .unwrap()
            .value
            .unwrap();
        pages.push(CatalogSelectionPage {
            query,
            revision: saved.revision,
        });
    }
    let state = client
        .play_selection(
            "1",
            PlaySelection::Catalog {
                pages: pages.clone(),
                index: 23,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.queue_len, 40);
    assert!(
        state.origin_name.is_none(),
        "song credits do not supply the artist page name"
    );
    let track = state.track.unwrap();
    assert_eq!(track.id, "24");
    assert_eq!(track.artist_refs[0].id.as_deref(), Some("12"));
    assert_eq!(track.album_id.as_deref(), Some("7"));
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(-1)));
    let stored = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    assert_eq!(
        stored["items"][0]["source"],
        json!({"kind":"artist","id":"12","scene":"artist","trial_mode":-1})
    );
    client
        .catalog_page("1", CatalogQuery::Artist { id: "12".into() }, true)
        .await
        .unwrap();
    // The page cache by itself does not relabel a queue already prepared.
    assert!(
        client
            .player_snapshot("1")
            .await
            .unwrap()
            .origin_name
            .is_none()
    );
    api.calls.lock().unwrap().clear();
    let named = client
        .play_selection(
            "1",
            PlaySelection::Catalog {
                pages: pages.clone(),
                index: 22,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(named.origin_name.as_deref(), Some("已核对的歌手页"));
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|path| path.starts_with("/api/artist/"))
    );
    pages[1].revision = "old".into();
    assert!(matches!(
        client
            .play_selection(
                "1",
                PlaySelection::Catalog {
                    pages: pages.clone(),
                    index: 0,
                    quality: Quality::Standard
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    pages[1].query = CatalogQuery::ArtistSongs {
        id: "12".into(),
        offset: 40,
        limit: 20,
    };
    assert!(matches!(
        client
            .play_selection(
                "1",
                PlaySelection::Catalog {
                    pages,
                    index: 0,
                    quality: Quality::Standard
                }
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 40);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn catalog_album_selection_revalidates_the_album_and_keeps_all_tracks() {
    let (_dir, api, _session, client, _count) = fixture();
    let query = CatalogQuery::Album { id: "7".into() };
    let saved = client
        .catalog_page("1", query.clone(), true)
        .await
        .unwrap()
        .value
        .unwrap();
    api.calls.lock().unwrap().clear();
    let state = client
        .play_selection(
            "1",
            PlaySelection::Catalog {
                pages: vec![CatalogSelectionPage {
                    query,
                    revision: saved.revision,
                }],
                index: 500,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.queue_len, 501);
    assert_eq!(state.origin_name.as_deref(), Some("长专辑"));
    assert_eq!(state.track.unwrap().id, "501");
    assert_eq!(api.calls.lock().unwrap()[0], "/api/album/v3/detail");
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn output_settings_are_local_lazy_and_reject_old_account_commands() {
    let (_dir, api, session, client, count) = fixture();
    let epoch = session.token().unwrap().generation.to_string();
    let initial = client.output_settings(&epoch, false).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(initial.value.supported, None);
    let inspected = client.output_settings(&epoch, true).await.unwrap();
    assert_eq!(inspected.value.supported, Some(false));
    client.output_settings(&epoch, true).await.unwrap();
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "one never-loaded inspector"
    );
    let saved = client
        .output_command(
            &epoch,
            OutputCommand::Select {
                expected_revision: "0".into(),
                id: "auto".into(),
            },
        )
        .unwrap();
    assert_eq!(saved.value.revision.as_deref(), Some("1"));
    assert!(
        client
            .output_command(
                &epoch,
                OutputCommand::Retry {
                    expected_revision: "0".into()
                }
            )
            .is_err()
    );
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.output_command(
            &epoch,
            OutputCommand::Retry {
                expected_revision: "1".into()
            }
        ),
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.output_settings(&epoch, true).await,
        Err(Error::StaleResponse)
    ));
    let next = session.token().unwrap().generation.to_string();
    assert_eq!(
        client
            .output_settings(&next, false)
            .await
            .unwrap()
            .value
            .revision
            .as_deref(),
        Some("1")
    );
    assert!(
        api.calls.lock().unwrap().is_empty(),
        "device settings have no music API"
    );
}

#[tokio::test]
async fn desktop_lyric_document_shares_detail_cache_offset_and_native_clock_without_retries() {
    use cumulus_core::preferences::desktop_lyrics::{Appearance, Auxiliary};
    let (_dir, api, session, client, count) = fixture();
    assert!(matches!(
        client.desktop_lyric_document("1", "missing").await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(api.calls.lock().unwrap().is_empty());
    let current = client.play_selection("1", songs("1")).await.unwrap();
    let id = current.play_id.unwrap();
    let mut document = client.desktop_lyric_document("1", &id).await.unwrap();
    let requests = api.calls.lock().unwrap().len();
    let style = Appearance::default();
    let initial = client.desktop_lyric_frame(&document, &style).unwrap();
    assert_eq!(initial.play_id, id);
    assert!(initial.frame.timed_words);
    assert_eq!(initial.frame.lines[0].auxiliary, "hello ");
    for _ in 0..50 {
        assert!(
            !client
                .refresh_desktop_lyric_document(&mut document)
                .await
                .unwrap()
        );
        client.desktop_lyric_frame(&document, &style).unwrap();
    }
    assert_eq!(api.calls.lock().unwrap().len(), requests);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    client.lyric_offset("1", &id, 1.0).await.unwrap();
    assert!(
        client
            .refresh_desktop_lyric_document(&mut document)
            .await
            .unwrap()
    );
    let updated = client.desktop_lyric_frame(&document, &style).unwrap();
    assert_ne!(updated.revision, initial.revision);
    assert_eq!(updated.frame.offset, 0.5);
    client
        .desktop_lyric_offset(&mut document, -1.0)
        .await
        .unwrap();
    assert_eq!(client.player_lyrics("1", &id).await.unwrap().offset, 0.0);
    let translated = client
        .desktop_lyric_frame(
            &document,
            &Appearance {
                auxiliary: Auxiliary::Roman,
                ..style.clone()
            },
        )
        .unwrap();
    assert_eq!(translated.frame.lines[0].auxiliary.trim(), "ni hao");
    assert_eq!(api.calls.lock().unwrap().len(), requests);
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = client.desktop_lyric_frame(&document, &style).unwrap();
    assert!(!paused.playing);
    client.play_selection("1", songs("2")).await.unwrap();
    assert!(client.desktop_lyric_frame(&document, &style).is_err());
    assert!(
        client
            .desktop_lyric_offset(&mut document, 1.0)
            .await
            .is_err()
    );
    session
        .adopt(api, json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(
        client
            .refresh_desktop_lyric_document(&mut document)
            .await
            .is_err()
    );
}

#[path = "client_player/next_play.rs"]
mod next_play;
#[path = "client_player/playlist_batch.rs"]
mod playlist_batch;

#[path = "client_player/queue_drop.rs"]
mod queue_drop;
#[path = "client_player/queue_playlist.rs"]
mod queue_playlist;

#[tokio::test]
async fn native_quality_details_bind_song_and_keep_size_reads_independent_of_default_visibility() {
    use cumulus_core::playback::quality::{QualityAccess, QualityAvailability, QualityReadState};
    let (_dir, api, _session, client, _) = fixture();
    let play = client.play_selection("1", songs("1")).await.unwrap();
    let play_id = play.play_id.unwrap();
    let details = client.player_quality_details("1", &play_id).await.unwrap();
    assert_eq!(details.song_id, "1");
    assert_eq!(details.play_id, play_id);
    assert_eq!(details.status.sizes, QualityReadState::Ready);
    let hires = details
        .choices
        .iter()
        .find(|c| c.quality == Quality::HiRes)
        .unwrap();
    assert_eq!(hires.availability, QualityAvailability::Available);
    assert_eq!(hires.access, QualityAccess::Allowed);
    let master = details
        .choices
        .iter()
        .find(|c| c.quality == Quality::JyMaster)
        .unwrap();
    assert_eq!(master.access, QualityAccess::TrialEligible);
    assert_eq!(
        details
            .choices
            .iter()
            .find(|c| c.quality == Quality::Lossless)
            .unwrap()
            .size_bytes,
        None
    );
    let text = serde_json::to_string(&details).unwrap();
    assert!(!text.contains("SECRET") && !text.contains("https://") && !text.contains("cookie"));
    let again = client.player_quality_details("1", &play_id).await.unwrap();
    assert_eq!(again.revision, details.revision);
    client.play_selection("1", songs("2")).await.unwrap();
    let calls = api.calls.lock().unwrap().len();
    assert!(matches!(
        client.player_quality_details("1", &play_id).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn an_explicit_quality_trial_uses_native_playback_and_preserves_pause_position_and_actual_quality()
 {
    let (_dir, api, _session, client, _) = fixture();
    let playing = client.play_selection("1", songs("1")).await.unwrap();
    let play_id = playing.play_id.unwrap();
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 10.0 })
        .await
        .unwrap();
    let details = client.player_quality_details("1", &play_id).await.unwrap();
    let result = client
        .player_quality_trial(
            "1",
            QualityTrialCommand::Start {
                play_id: play_id.clone(),
                details_revision: details.revision,
                level: Quality::JyMaster,
            },
        )
        .await
        .unwrap();
    assert!(result.trial.acknowledged && result.trial.can_use && result.playback_error.is_none());
    let player = result.playback.unwrap();
    assert_eq!(
        player.state,
        cumulus_core::playback::controller::PlaybackStatus::Paused
    );
    assert_eq!(player.position, Some(10.0));
    assert_eq!(player.requested_quality.as_deref(), Some("jymaster"));
    assert_eq!(player.actual_quality.as_deref(), Some("standard"));
    assert_eq!(player.quality_preferences.quality, Quality::JyMaster);
    assert_eq!(api.quality_writes.load(Ordering::SeqCst), 1);
    let updated = client
        .player_quality_details("1", player.play_id.as_ref().unwrap())
        .await
        .unwrap();
    assert_eq!(updated.trials[0].operation, result.trial.operation);
    assert!(!updated.trials_damaged);
}

#[tokio::test]
async fn obsolete_quality_trial_offers_are_rejected_before_activating_quota() {
    let (_dir, api, _session, client, _) = fixture();
    let playing = client.play_selection("1", songs("1")).await.unwrap();
    let play_id = playing.play_id.unwrap();
    let details = client.player_quality_details("1", &play_id).await.unwrap();
    client
        .player_action(
            "1",
            PlayerAction::Quality {
                level: Quality::HiRes,
            },
        )
        .await
        .unwrap();
    assert!(
        client
            .player_quality_trial(
                "1",
                QualityTrialCommand::Start {
                    play_id,
                    details_revision: details.revision,
                    level: Quality::JyMaster
                }
            )
            .await
            .is_err()
    );
    assert_eq!(api.quality_writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_lost_activation_is_reconciled_and_retried_as_playback_without_a_second_start() {
    use cumulus_core::playback::quality::{QualityRight, trials::QualityTrialPhase};
    let (_dir, api, _session, client, _) = fixture();
    let playing = client.play_selection("1", songs("1")).await.unwrap();
    let play_id = playing.play_id.unwrap();
    let details = client.player_quality_details("1", &play_id).await.unwrap();
    api.quality_start_mode.store(2, Ordering::SeqCst);
    let uncertain = client
        .player_quality_trial(
            "1",
            QualityTrialCommand::Start {
                play_id: play_id.clone(),
                details_revision: details.revision,
                level: Quality::JyMaster,
            },
        )
        .await
        .unwrap();
    assert_eq!(uncertain.trial.phase, QualityTrialPhase::Uncertain);
    assert!(uncertain.playback.is_none());
    let checked = client
        .player_quality_trial(
            "1",
            QualityTrialCommand::Reconcile {
                right: QualityRight::JyMaster,
                operation: uncertain.trial.operation,
            },
        )
        .await
        .unwrap();
    assert!(checked.trial.can_use && !checked.trial.acknowledged);
    let details = client.player_quality_details("1", &play_id).await.unwrap();
    let applied = client
        .player_quality_trial(
            "1",
            QualityTrialCommand::UseActive {
                play_id,
                details_revision: details.revision,
                level: Quality::JyMaster,
            },
        )
        .await
        .unwrap();
    assert!(applied.playback.is_some());
    assert_eq!(api.quality_writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn activation_completed_after_a_new_selection_cannot_change_the_new_song() {
    let (_dir, api, _session, client, _) = fixture();
    let playing = client.play_selection("1", songs("1")).await.unwrap();
    let play_id = playing.play_id.unwrap();
    let details = client.player_quality_details("1", &play_id).await.unwrap();
    api.quality_start_mode.store(1, Ordering::SeqCst);
    let pending = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .player_quality_trial(
                    "1",
                    QualityTrialCommand::Start {
                        play_id,
                        details_revision: details.revision,
                        level: Quality::JyMaster,
                    },
                )
                .await
        }
    });
    api.quality_started.notified().await;
    let current = client.play_selection("1", songs("2")).await.unwrap();
    api.quality_release.notify_one();
    let result = pending.await.unwrap().unwrap();
    assert!(result.trial.can_use && result.playback.is_none());
    assert_eq!(result.playback_error.unwrap().code, "stale");
    let after = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(after.track.unwrap().id, "2");
    assert_eq!(after.play_id, current.play_id);
    assert_eq!(after.quality_preferences.quality, Quality::Standard);
    assert_eq!(api.quality_writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_playback_after_activation_keeps_the_receipt_and_retries_only_the_authorized_url() {
    let (_dir, api, _session, client, _) = fixture();
    let playing = client.play_selection("1", songs("1")).await.unwrap();
    let play_id = playing.play_id.unwrap();
    let details = client.player_quality_details("1", &play_id).await.unwrap();
    api.quality_url_denied.store(1, Ordering::SeqCst);
    let failed = client
        .player_quality_trial(
            "1",
            QualityTrialCommand::Start {
                play_id: play_id.clone(),
                details_revision: details.revision,
                level: Quality::JyMaster,
            },
        )
        .await
        .unwrap();
    assert!(
        failed.trial.acknowledged
            && failed.trial.can_use
            && failed.playback.is_none()
            && failed.playback_error.is_some()
    );
    api.quality_url_denied.store(0, Ordering::SeqCst);
    let details = client.player_quality_details("1", &play_id).await.unwrap();
    let applied = client
        .player_quality_trial(
            "1",
            QualityTrialCommand::UseActive {
                play_id,
                details_revision: details.revision,
                level: Quality::JyMaster,
            },
        )
        .await
        .unwrap();
    assert!(applied.playback.is_some());
    assert_eq!(api.quality_writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn checked_mode_rejects_replaced_queues_and_the_wrong_player_family() {
    use cumulus_core::playback::PlaybackMode;
    let (_dir, api, _session, client, _) = fixture();
    let old = client.play_selection("1", songs("1")).await.unwrap();
    let current = client.play_selection("1", songs("2")).await.unwrap();
    let calls = api.calls.lock().unwrap().len();
    for (revision, fm) in [
        (old.queue_revision, false),
        (current.queue_revision.clone(), true),
    ] {
        assert!(matches!(
            client
                .player_action(
                    "1",
                    PlayerAction::ModeFor {
                        mode: PlaybackMode::Random,
                        expected_queue_revision: revision,
                        fm,
                    }
                )
                .await,
            Err(Error::QueueChanged)
        ));
    }
    let changed = client
        .player_action(
            "1",
            PlayerAction::ModeFor {
                mode: PlaybackMode::SingleLoop,
                expected_queue_revision: current.queue_revision,
                fm: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(changed.mode, PlaybackMode::SingleLoop);
    assert_eq!(changed.play_id, current.play_id);
    assert_eq!(changed.position, current.position);
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn checked_mode_during_source_loading_keeps_the_pending_selection_alive() {
    use cumulus_core::playback::PlaybackMode;
    let (_dir, api, _session, client, _) = fixture();
    api.hold_id.store(1, Ordering::SeqCst);
    let next = client.clone();
    let pending = tokio::spawn(async move { next.play_selection("1", songs("1")).await });
    api.started.notified().await;
    let subscription = client.subscribe_playback("1").await.unwrap();
    let before = subscription.current().unwrap();
    client
        .player_action(
            "1",
            PlayerAction::ModeFor {
                mode: PlaybackMode::SingleLoop,
                expected_queue_revision: before.queue_revision,
                fm: false,
            },
        )
        .await
        .unwrap();
    api.release.notify_one();
    let loaded = pending.await.unwrap().unwrap();
    assert_eq!(loaded.mode, PlaybackMode::SingleLoop);
    assert_eq!(loaded.track.unwrap().id, "1");
    assert_eq!(
        loaded.state,
        cumulus_core::playback::controller::PlaybackStatus::Playing
    );
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn displayed_queue_edits_reject_replaced_lists_before_touching_transport() {
    let (_dir, api, _session, client, _) = fixture();
    let old = client.play_selection("1", songs("1")).await.unwrap();
    let current = client.play_selection("1", songs("2")).await.unwrap();
    let calls = api.calls.lock().unwrap().len();
    for edit in [
        QueueEdit::Clear,
        QueueEdit::Select {
            entry_id: current.entry_id.clone().unwrap(),
        },
        QueueEdit::Remove {
            entry_ids: vec![current.entry_id.clone().unwrap()],
        },
        QueueEdit::Reorder {
            entry_ids: vec![current.entry_id.clone().unwrap()],
        },
        QueueEdit::Move {
            entry_ids: vec![current.entry_id.clone().unwrap()],
            movement: cumulus_core::playback::QueueMovement {
                entry_ids: vec![current.entry_id.clone().unwrap()],
                offset: 0,
            },
        },
    ] {
        assert!(matches!(
            client
                .player_action(
                    "1",
                    PlayerAction::QueueFor {
                        expected_room_id: None,
                        expected_queue_revision: old.queue_revision.clone(),
                        edit,
                    }
                )
                .await,
            Err(Error::QueueChanged)
        ));
    }
    let after = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(after.play_id, current.play_id);
    assert_eq!(after.queue_revision, current.queue_revision);
    assert_eq!(after.entry_id, current.entry_id);
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn displayed_queue_move_checks_selection_and_old_offset_before_committing() {
    use cumulus_core::playback::{QueueMovement, controller::PlaybackStatus};

    let (_dir, api, _session, client, _) = fixture();
    let playing = client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into(), "3".into()],
                index: 1,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let before = client.player_queue("1").await.unwrap();
    let ids: Vec<_> = before.tracks.iter().map(|i| i.entry_id.clone()).collect();
    let reversed: Vec<_> = ids.iter().rev().cloned().collect();
    let calls = api.calls.lock().unwrap().len();
    let action = |entry_ids, moved, offset| PlayerAction::QueueFor {
        expected_room_id: None,
        expected_queue_revision: before.revision.clone(),
        edit: QueueEdit::Move {
            entry_ids,
            movement: QueueMovement {
                entry_ids: moved,
                offset,
            },
        },
    };
    for invalid in [
        action(reversed, vec![ids[0].clone()], 2),
        action(ids.clone(), vec![ids[0].clone(), ids[0].clone()], 0),
        action(ids.clone(), vec!["not-a-visible-entry".into()], 0),
    ] {
        assert!(matches!(
            client.player_action("1", invalid).await,
            Err(Error::InvalidInput(_))
        ));
    }
    let unchanged = client
        .player_action("1", action(ids.clone(), vec![ids[1].clone()], 1))
        .await
        .unwrap();
    assert_eq!(unchanged.queue_revision, before.revision);
    let after = client
        .player_action(
            "1",
            action(
                vec![ids[1].clone(), ids[2].clone(), ids[0].clone()],
                vec![ids[0].clone()],
                3,
            ),
        )
        .await
        .unwrap();
    assert_ne!(after.queue_revision, before.revision);
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(after.play_id, playing.play_id);
    assert_eq!(after.entry_id, playing.entry_id);
    assert_eq!(after.position, paused.position);
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    let reordered: Vec<_> = client
        .player_queue("1")
        .await
        .unwrap()
        .tracks
        .into_iter()
        .map(|row| row.entry_id)
        .collect();
    assert_eq!(reordered, [ids[1].clone(), ids[2].clone(), ids[0].clone()]);

    api.hold_id.store(99, Ordering::SeqCst);
    let next = client.clone();
    let pending = tokio::spawn(async move { next.play_selection("1", songs("99")).await });
    api.started.notified().await;
    let unchanged = client
        .player_action(
            "1",
            PlayerAction::QueueFor {
                expected_room_id: None,
                expected_queue_revision: after.queue_revision.clone(),
                edit: QueueEdit::Move {
                    entry_ids: reordered,
                    movement: QueueMovement {
                        entry_ids: vec![ids[1].clone()],
                        offset: 0,
                    },
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(unchanged.queue_revision, after.queue_revision);
    api.release.notify_one();
    assert_eq!(pending.await.unwrap().unwrap().track.unwrap().id, "99");
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn displayed_queue_edits_keep_audible_identity_and_clear_only_the_confirmed_list() {
    let (_dir, _api, _session, client, _) = fixture();
    let playing = client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into(), "3".into()],
                index: 1,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let before = client.player_queue("1").await.unwrap();
    let ids: Vec<_> = before.tracks.iter().map(|i| i.entry_id.clone()).collect();
    let reordered = client
        .player_action(
            "1",
            PlayerAction::QueueFor {
                expected_room_id: None,
                expected_queue_revision: before.revision,
                edit: QueueEdit::Reorder {
                    entry_ids: ids.iter().rev().cloned().collect(),
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(reordered.play_id, playing.play_id);
    assert_eq!(reordered.entry_id, playing.entry_id);
    assert_eq!(
        reordered.state,
        cumulus_core::playback::controller::PlaybackStatus::Paused
    );
    let removed = client
        .player_action(
            "1",
            PlayerAction::QueueFor {
                expected_room_id: None,
                expected_queue_revision: reordered.queue_revision,
                edit: QueueEdit::Remove {
                    entry_ids: vec![ids[0].clone()],
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(removed.queue_len, 2);
    assert_eq!(removed.play_id, playing.play_id);
    let selected = client
        .player_action(
            "1",
            PlayerAction::QueueFor {
                expected_room_id: None,
                expected_queue_revision: removed.queue_revision,
                edit: QueueEdit::Select {
                    entry_id: ids[2].clone(),
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(selected.entry_id.as_deref(), Some(ids[2].as_str()));
    let cleared = client
        .player_action(
            "1",
            PlayerAction::QueueFor {
                expected_room_id: None,
                expected_queue_revision: selected.queue_revision,
                edit: QueueEdit::Clear,
            },
        )
        .await
        .unwrap();
    assert_eq!(cleared.queue_len, 0);
    assert!(cleared.play_id.is_none() && cleared.track.is_none());
    assert!(client.player_queue("1").await.unwrap().tracks.is_empty());
}

#[tokio::test]
async fn stale_queue_confirmation_does_not_cancel_the_new_selection_in_flight() {
    let (_dir, api, _session, client, _) = fixture();
    let old = client.play_selection("1", songs("2")).await.unwrap();
    api.hold_url_id.store(1, Ordering::SeqCst);
    let next = client.clone();
    let pending = tokio::spawn(async move { next.play_selection("1", songs("1")).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(4),
        api.url_started.notified(),
    )
    .await
    .unwrap();
    let loading = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_ne!(loading.queue_revision, old.queue_revision);
    assert!(matches!(
        client
            .player_action(
                "1",
                PlayerAction::QueueFor {
                    expected_room_id: None,
                    expected_queue_revision: old.queue_revision,
                    edit: QueueEdit::Clear,
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    api.url_release.notify_one();
    let loaded = pending.await.unwrap().unwrap();
    assert_eq!(loaded.track.unwrap().id, "1");
    assert_eq!(
        loaded.state,
        cumulus_core::playback::controller::PlaybackStatus::Playing
    );
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn hot_search_selection_keeps_queue_order_and_requests_search_trial() {
    let (_dir, api, _session, client, _) = fixture();
    client
        .play_selection(
            "1",
            PlaySelection::SearchChart {
                ids: vec!["8".into(), "3".into()],
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|track| track.track.id.as_str())
            .collect::<Vec<_>>(),
        vec!["8", "3"]
    );
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(42)));
}

#[tokio::test]
async fn single_search_result_keeps_queue_and_uses_search_trial() {
    use cumulus_core::playback::controller::PlaybackStatus;

    let (_dir, api, _session, client, _) = fixture();
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["1", "2", "3"].map(str::to_owned).to_vec(),
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let baseline = client.player_queue("1").await.unwrap();
    let entry_ids: Vec<_> = baseline
        .tracks
        .iter()
        .map(|row| row.entry_id.clone())
        .collect();
    let select = |id: &str, toggle_current| PlaySelection::SearchResultTrack {
        id: id.into(),
        quality: Quality::Standard,
        toggle_current,
    };

    let playing = client.play_selection("1", select("4", true)).await.unwrap();
    assert_eq!(playing.track.unwrap().id, "4");
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(42)));
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "4", "2", "3"]
    );
    assert_eq!(queue.current.as_ref(), Some(&queue.tracks[1].entry_id));
    let detail_calls = || {
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.as_str() == "/api/v3/song/detail")
            .count()
    };
    let calls = detail_calls();
    let paused = client.play_selection("1", select("4", true)).await.unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    assert_eq!(paused.play_id, playing.play_id);
    assert_eq!(paused.queue_revision, playing.queue_revision);
    assert_eq!(detail_calls(), calls);

    let reused = client
        .play_selection("1", select("3", false))
        .await
        .unwrap();
    assert_eq!(reused.track.unwrap().id, "3");
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(42)));
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "4", "3", "2"]
    );
    assert_eq!(queue.current.as_ref(), Some(&entry_ids[2]));
    assert_eq!(queue.tracks[0].entry_id, entry_ids[0]);
    assert_eq!(queue.tracks[3].entry_id, entry_ids[1]);
}

#[tokio::test]
async fn search_vinyl_origin_depends_on_the_played_entry_not_the_view_or_pause_gesture() {
    use cumulus_core::playback::controller::PlaybackStatus;

    let (_dir, api, _session, client, _) = fixture();
    *api.playlist_tracks.lock().unwrap() = Some(vec![json!({"id":1,"v":0})]);
    let started = client
        .play_selection(
            "1",
            PlaySelection::Playlist {
                id: "7".into(),
                start_id: Some("1".into()),
                revision: None,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let playlist = json!({"kind":"playlist","id":"7"});
    let public = serde_json::to_value(&started).unwrap();
    assert_eq!(public["origin"], playlist);
    assert!(public.get("searchOrigin").is_none());
    let calls = api.calls.lock().unwrap().clone();
    let queue = serde_json::to_value(client.player_queue("1").await.unwrap()).unwrap();
    let row = |id: &str| PlaySelection::SearchResultTrack {
        id: id.into(),
        quality: Quality::Standard,
        toggle_current: true,
    };
    for expected in [PlaybackStatus::Paused, PlaybackStatus::Playing] {
        let view = client.play_selection("1", row("1")).await.unwrap();
        assert_eq!(view.state, expected);
        assert_eq!(view.play_id, started.play_id);
        assert_eq!(view.entry_id, started.entry_id);
        assert_eq!(view.queue_revision, started.queue_revision);
        let public = serde_json::to_value(&view).unwrap();
        assert_eq!(public["origin"], playlist);
        assert!(public.get("searchOrigin").is_none());
    }
    assert_eq!(*api.calls.lock().unwrap(), calls);
    assert_eq!(
        serde_json::to_value(client.player_queue("1").await.unwrap()).unwrap(),
        queue
    );

    let played = client.play_selection("1", row("2")).await.unwrap();
    assert_eq!(played.state, PlaybackStatus::Playing);
    assert_eq!(played.track.as_ref().unwrap().id, "2");
    assert_ne!(played.play_id, started.play_id);
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(42)));
    let paused = client.play_selection("1", row("2")).await.unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    assert_eq!(paused.play_id, played.play_id);
    assert_eq!(paused.entry_id, played.entry_id);
    assert_eq!(paused.queue_revision, played.queue_revision);
    for view in [played, paused, client.player_snapshot("1").await.unwrap()] {
        let public = serde_json::to_value(&view).unwrap();
        assert_eq!(public["searchOrigin"], true);
        assert!(public["origin"].is_null());
        for key in ["source", "scene", "href", "keyword", "url", "authorization"] {
            assert!(public.get(key).is_none(), "public player leaked {key}");
        }
    }
}

#[tokio::test]
async fn search_result_actions_keep_selected_index_and_use_the_recovered_trial_context() {
    let (dir, api, _session, client, _) = fixture();
    let store = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let contexts = [
        (
            SearchResultPlayback::SongTabAll,
            42,
            json!({"kind":"songs","scene":"search","pldScene":"searchSongTab","trial_mode":42}),
        ),
        (
            SearchResultPlayback::ComprehensiveAll,
            -1,
            json!({"kind":"songs","scene":"search"}),
        ),
    ];
    for (i, (context, trial, source)) in contexts.into_iter().enumerate() {
        let ids = vec![(i * 2 + 1).to_string(), (i * 2 + 2).to_string()];
        let state = client
            .play_selection(
                "1",
                PlaySelection::SearchResults {
                    ids: ids.clone(),
                    index: 1,
                    quality: Quality::Standard,
                    context,
                },
            )
            .await
            .unwrap();
        assert!(state.search_origin);
        assert_eq!(state.track.unwrap().id, ids[1]);
        assert_eq!(state.queue_len, 2);
        assert_eq!(*api.trial.lock().unwrap(), Some(json!(trial)));
        let stored = store.get("9", "playback", "queue").unwrap().unwrap();
        assert_eq!(stored["items"][0]["source"], source);
        assert_eq!(stored["items"][1]["source"], source);
    }
}

#[tokio::test]
async fn loaded_trial_reads_and_caches_standard_assignment_without_reusing_another_accounts_flag() {
    use cumulus_core::playback::quality::QualityAccess;

    let (_dir, api, session, client, _) = fixture();
    let client = Arc::try_unwrap(client)
        .unwrap_or_else(|_| panic!("fixture must own its client"))
        .with_official_quality_experiments();
    let assignment_calls = || {
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.as_str() == "/api/rtrs/abt/front/expinfo/list")
            .count()
    };
    let standard = |details: &QualityDetailsView| {
        details
            .choices
            .iter()
            .find(|c| c.quality == Quality::Standard)
            .unwrap()
            .access
    };
    let ordinary = client.play_selection("1", songs("1")).await.unwrap();
    assert_eq!(
        standard(
            &client
                .player_quality_details("1", ordinary.play_id.as_deref().unwrap())
                .await
                .unwrap()
        ),
        QualityAccess::Allowed
    );
    assert_eq!(assignment_calls(), 0);
    api.quality_trial_source.store(true, Ordering::SeqCst);
    api.quality_standard_assignment.store(1, Ordering::SeqCst);
    let trial = client.play_selection("1", songs("2")).await.unwrap();
    let play_id = trial.play_id.unwrap();
    for _ in 0..2 {
        assert_eq!(
            standard(&client.player_quality_details("1", &play_id).await.unwrap()),
            QualityAccess::RequiresPurchase
        );
    }
    assert_eq!(assignment_calls(), 1);
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.quality_standard_assignment.store(0, Ordering::SeqCst);
    assert!(matches!(
        client.player_quality_details("1", &play_id).await,
        Err(Error::StaleResponse)
    ));
    let new = client.play_selection("2", songs("2")).await.unwrap();
    assert_eq!(
        standard(
            &client
                .player_quality_details("2", new.play_id.as_deref().unwrap())
                .await
                .unwrap()
        ),
        QualityAccess::Allowed
    );
    assert_eq!(assignment_calls(), 2);
}

#[tokio::test]
async fn daily_recommendations_keep_distinct_snapshot_and_official_playback_source() {
    let (dir, api, _session, client, _count) = fixture();
    let style = client
        .style_recommendations("1")
        .await
        .unwrap()
        .value
        .revision
        .unwrap();
    let stale = client.daily_songs().await.unwrap().value.revision.unwrap();
    let current = client.daily_songs().await.unwrap().value.revision.unwrap();
    let select = |revision, index| PlaySelection::DailyRecommendations {
        revision,
        index,
        quality: Quality::Standard,
    };
    for wrong in [style, stale] {
        assert!(matches!(
            client.play_selection("1", select(wrong, 0)).await,
            Err(Error::QueueChanged)
        ));
    }
    let state = client
        .play_selection("1", select(current.clone(), 1))
        .await
        .unwrap();
    assert_eq!(state.track.unwrap().id, "73");
    assert_eq!(state.queue_len, 2);
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(35)));
    let stored = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    assert_eq!(stored["items"][0]["resource"]["id"], "74");
    assert_eq!(stored["items"][1]["resource"]["id"], "73");
    assert_eq!(
        stored["items"][1]["source"],
        json!({"kind":"daily_recommendations","scene":"dailyRecommend","href":"/dailyRecommend","resourceType":"dailyRecommend","revision":current,"trial_mode":35})
    );
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn style_recommendation_playback_preserves_snapshot_order_scene_and_official_trial() {
    let (dir, api, _session, client, _count) = fixture();
    let stale = client
        .style_recommendations("1")
        .await
        .unwrap()
        .value
        .revision
        .unwrap();
    let revision = client
        .style_recommendations("1")
        .await
        .unwrap()
        .value
        .revision
        .unwrap();
    assert_ne!(revision, stale);
    let select = |revision, index| PlaySelection::StyleRecommendations {
        revision,
        index,
        quality: Quality::Standard,
    };
    assert!(matches!(
        client.play_selection("1", select(stale, 0)).await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        client
            .play_selection("1", select(revision.clone(), 2))
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == "/api/v3/song/detail")
    );
    let view = client.player_snapshot("1").await.unwrap();
    let ticket = client
        .prepare_play_selection("1", &view.queue_revision, view.play_id.as_deref())
        .unwrap();
    let state = client
        .commit_prepared_selection("1", &ticket, select(revision.clone(), 1))
        .await
        .unwrap();
    assert_eq!(state.track.unwrap().id, "71");
    assert_eq!(state.queue_len, 2);
    assert_eq!(*api.trial.lock().unwrap(), Some(json!(35)));
    let stored = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    assert_eq!(stored["items"][0]["resource"]["id"], "72");
    assert_eq!(stored["items"][1]["resource"]["id"], "71");
    assert_eq!(
        stored["items"][0]["source"],
        json!({"kind":"style_recommendations","scene":"styleRecommend",
        "href":"/styleRecommend","resourceType":"dailyRecommend","revision":revision,"trial_mode":35})
    );
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn style_recommendation_append_rebinds_upcoming_tracks_and_preserves_current() {
    let device = Arc::new(support::Device::default());
    let (dir, api, _session, client, _) = fixture_with_device(device.clone());
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["1", "71", "2"].into_iter().map(str::to_owned).collect(),
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let daily_revision = client.daily_songs().await.unwrap().value.revision.unwrap();
    let style_revision = client
        .style_recommendations("1")
        .await
        .unwrap()
        .value
        .revision
        .unwrap();
    let action = |revision: &str| {
        serde_json::from_value::<PlayerAction>(json!({
            "kind": "queue_style_recommendations",
            "revision": revision,
            "quality": "standard",
            "expected_queue_revision": paused.queue_revision
        }))
        .unwrap()
    };
    let before = api.calls.lock().unwrap().len();
    assert!(matches!(
        client.player_action("1", action(&daily_revision)).await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), before);
    let after = client
        .player_action("1", action(&style_revision))
        .await
        .unwrap();
    assert_eq!(after.state, paused.state);
    assert_eq!(after.play_id, paused.play_id);
    assert_eq!(after.position, paused.position);
    assert_eq!(device.state.lock().unwrap().loads, 1);
    let stored = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    let items = stored["items"].as_array().unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item["resource"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["1", "72", "71", "2"]
    );
    for item in &items[1..3] {
        assert_eq!(item["resource"]["trial_mode"], 35);
        assert_eq!(
            item["source"],
            json!({"kind":"style_recommendations","scene":"styleRecommend",
            "href":"/styleRecommend","resourceType":"dailyRecommend",
            "revision":style_revision,"trial_mode":35})
        );
    }
}

#[tokio::test]
async fn style_recommendation_refresh_during_song_details_cannot_replace_queue() {
    let (_dir, api, _session, client, _count) = fixture();
    let revision = client
        .style_recommendations("1")
        .await
        .unwrap()
        .value
        .revision
        .unwrap();
    api.hold_id.store(72, Ordering::SeqCst);
    let playing = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .play_selection(
                    "1",
                    PlaySelection::StyleRecommendations {
                        revision,
                        index: 0,
                        quality: Quality::Standard,
                    },
                )
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), api.started.notified())
        .await
        .unwrap();
    client.style_recommendations("1").await.unwrap();
    api.release.notify_one();
    assert!(matches!(playing.await.unwrap(), Err(Error::QueueChanged)));
    assert!(client.player_queue("1").await.unwrap().tracks.is_empty());
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == "/api/song/enhance/player/url/v1")
    );
}
