//! Production desktop WebView and libmpv with a closed synthetic podcast transport.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/podcast_badge_fixture.rs"]
mod badge_fixture;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[path = "support/podcast_scene_home_fixture.rs"]
mod scene_home_fixture;
#[path = "support/scene_palette_fixture.rs"]
mod scene_palette_fixture;
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
#[path = "support/voice_detail_fixture.rs"]
mod voice_detail_fixture;
#[path = "support/voice_fresh_actions_fixture.rs"]
mod voice_fresh_actions_fixture;
#[path = "support/voice_page_fixture.rs"]
mod voice_page_fixture;
#[path = "support/voice_podcast_collect_fixture.rs"]
mod voice_podcast_collect_fixture;
#[path = "support/voice_recommendation_fixture.rs"]
mod voice_recommendation_fixture;
#[path = "support/voice_song_collect_fixture.rs"]
mod voice_song_collect_fixture;
#[path = "support/voice_song_download_fixture.rs"]
mod voice_song_download_fixture;
#[path = "support/voice_song_group_liked_fixture.rs"]
mod voice_song_group_liked_fixture;
#[path = "support/voice_songs_fixture.rs"]
mod voice_songs_fixture;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::{
        api::{
            Api, ApiFuture, GuardedPlaylistAdd, GuardedPlaylistCreate, GuardedSongLike,
            PlaylistAddPermission, PodcastBadgeClear, PodcastBadgePermission,
            PodcastProgressPermission, SongCardLikedRead, SongLikePermission,
            VinylPlaybackReadPermission, VinylPodcastSubscriptionRead,
            VinylPodcastSubscriptionWrite, VinylVoiceDetailRead, VinylVoiceRecommendationsRead,
            VinylVoiceSongsRead,
        },
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

const PROGRESS: &str = "/api/dj/playrecord/upload";
const PROOF_REVISION: &str = "podcast-progress-fixture";
const RECENT: &str = "/api/social/my/recent/update/voice/list/v1";
const CURSOR: u64 = 1_700_000_000_123;
const MEDIA: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";

#[derive(Default)]
struct Library {
    base: relay_fixture::Library,
    first: AtomicUsize,
    next: AtomicUsize,
    calls: Mutex<Vec<Value>>,
    progress_enabled: bool,
    program_menu: bool,
    recent_program_menu: bool,
    recent_menu_count_held: AtomicBool,
    recent_menu_count_delivered: AtomicBool,
    recent_menu_count_released: tokio::sync::Notify,
    progress_uploads: Mutex<Vec<Value>>,
    badges: Option<badge_fixture::Fixture>,
    scene_home: Option<scene_home_fixture::SceneHome>,
    scene_baseline: settings_fixture::Library,
    palette: Option<Arc<scene_palette_fixture::PaletteMedia>>,
    voice_detail: Option<voice_detail_fixture::VoiceDetail>,
    voice_page: Option<Arc<voice_page_fixture::Fixture>>,
    voice_fresh_actions: Option<voice_fresh_actions_fixture::Fixture>,
    voice_podcast_collect: Option<voice_podcast_collect_fixture::Fixture>,
    voice_recommendations: Option<voice_recommendation_fixture::Fixture>,
    voice_songs: Option<Arc<voice_songs_fixture::Fixture>>,
    voice_song_download: Option<Arc<voice_song_download_fixture::Fixture>>,
    voice_song_collect: Option<voice_song_collect_fixture::Fixture>,
    voice_song_group_liked: Option<voice_song_group_liked_fixture::Fixture>,
}

fn radio() -> Value {
    json!({"id":8,"name":"合成播客","programCount":1,"subed":true})
}
fn voice(id: u64) -> Value {
    json!({"voiceId":id,"voiceName":format!("合成单集 {id}"),"mainSong":{"id":1000+id,"duration":30000},
        "radio":radio(),"duration":30000,"listenerCount":12000+id,"djPlayRecordVo":{"listenLocation":12000,"isListened":id==502},
        "disPlayStatus":if id==505 {"REMOVED"}else{"NORMAL"}})
}
impl Library {
    fn voice_page_authority(
        &self,
        revision: &str,
        permission: &VinylPlaybackReadPermission<'_>,
    ) -> Result<&voice_page_fixture::Fixture> {
        let fixture = self
            .voice_page
            .as_deref()
            .ok_or(Error::State("sealed VoicePage proof unavailable"))?;
        assert_eq!(
            self.verified_pc_experiment_revision()?.as_deref(),
            Some(revision)
        );
        permission()?;
        Ok(fixture)
    }

    fn decorate_scene_voice(&self, mut response: Value) -> Value {
        if let Some(palette) = &self.palette {
            response = scene_palette_fixture::artwork_response(response, palette.shared_cover());
        }
        if self.voice_detail.is_some() {
            response = voice_detail_fixture::enrich(response);
        }
        if let Some(collect) = &self.voice_podcast_collect {
            response = collect.enrich(response);
        }
        response
    }
}
impl Api for Library {
    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        if let Some(liked) = &self.voice_song_group_liked {
            return liked.client.verified_pc_experiment_revision();
        }
        if let Some(actions) = &self.voice_fresh_actions {
            return actions.client.verified_pc_experiment_revision();
        }
        Ok(self.scene_home.as_ref().map(|_| {
            if self.voice_recommendations.is_some() {
                voice_recommendation_fixture::REVISION.to_owned()
            } else if self.voice_songs.is_some() {
                voice_songs_fixture::REVISION.to_owned()
            } else {
                "scene-home-fixture-revision".to_owned()
            }
        }))
    }

    fn search_audio_toplist_assignment(&self) -> ApiFuture<'_, bool> {
        Box::pin(async { Ok(self.scene_home.is_some()) })
    }

    fn podcast_progress_revision(&self) -> Result<Option<String>> {
        Ok(self.progress_enabled.then(|| PROOF_REVISION.to_owned()))
    }

    fn upload_podcast_progress<'a>(
        &'a self,
        payload: Value,
        revision: &'a str,
        allowed: &'a PodcastProgressPermission,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            assert!(self.progress_enabled && revision == PROOF_REVISION);
            if !allowed()? {
                return Err(Error::StaleResponse);
            }
            assert!(payload.as_object().is_some_and(|body| body.len() == 3));
            assert!(matches!(
                payload["programId"].as_u64(),
                Some(501 | 503 | 504 | 506)
            ));
            assert!(payload["listenLocation"].as_f64().is_some_and(|position| {
                (0.0..=30_000.0).contains(&position) && position.fract() == 0.0
            }));
            assert!(payload["isListened"].is_boolean());
            self.progress_uploads
                .lock()
                .unwrap()
                .push(json!([PROGRESS, payload]));
            Ok(())
        })
    }

    fn podcast_badge_revision(&self) -> Result<Option<String>> {
        Ok(self.badges.as_ref().map(|_| badge_fixture::REVISION.into()))
    }

    fn clear_podcast_badge<'a>(
        &'a self,
        write: PodcastBadgeClear,
        revision: &'a str,
        permission: &'a PodcastBadgePermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            self.badges
                .as_ref()
                .expect("badge proof disabled")
                .clear(write, revision, permission, dispatched)
        })
    }

    fn request_vinyl_voice_detail<'a>(
        &'a self,
        read: VinylVoiceDetailRead,
        revision: &'a str,
        permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        if let Some(actions) = &self.voice_fresh_actions {
            return actions
                .client
                .request_vinyl_voice_detail(read, revision, permission);
        }
        Box::pin(async move {
            let voice_page = self.voice_page_authority(revision, permission)?;
            let (path, body) = read.into_request();
            let ticket = voice_page.begin(voice_page_fixture::Lane::Detail, path, &body);
            let response =
                self.decorate_scene_voice(scene_home_fixture::program_detail(ticket.program));
            let result = voice_page.detail(&ticket, response).await;
            voice_page.finish(ticket, result)
        })
    }

    fn vinyl_voice_action_revision(&self) -> Result<Option<String>> {
        self.voice_fresh_actions
            .as_ref()
            .map_or(Ok(None), |actions| {
                actions.client.vinyl_voice_action_revision()
            })
    }

    fn request_vinyl_podcast_subscription_state<'a>(
        &'a self,
        read: VinylPodcastSubscriptionRead,
        revision: &'a str,
        permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        let Some(actions) = &self.voice_fresh_actions else {
            return Box::pin(async { panic!("unsigned VoicePage attempted signed radio state") });
        };
        actions
            .client
            .request_vinyl_podcast_subscription_state(read, revision, permission)
    }

    fn request_vinyl_podcast_subscription_write<'a>(
        &'a self,
        write: VinylPodcastSubscriptionWrite,
        revision: &'a str,
        permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        let Some(actions) = &self.voice_fresh_actions else {
            return Box::pin(async { panic!("unsigned VoicePage attempted signed subscription") });
        };
        actions
            .client
            .request_vinyl_podcast_subscription_write(write, revision, permission)
    }

    fn request_vinyl_voice_recommendations<'a>(
        &'a self,
        read: VinylVoiceRecommendationsRead,
        revision: &'a str,
        permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let voice_page = self.voice_page_authority(revision, permission)?;
            let (path, body) = read.into_request();
            let ticket = voice_page.begin(voice_page_fixture::Lane::Recommendations, path, &body);
            let result = match &self.voice_recommendations {
                Some(fixture) => fixture.read(path, body).await,
                None => Ok(voice_page.recommendations()),
            };
            voice_page.finish(ticket, result)
        })
    }

    fn request_vinyl_voice_songs<'a>(
        &'a self,
        read: VinylVoiceSongsRead,
        revision: &'a str,
        permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        if let Some(liked) = &self.voice_song_group_liked {
            return liked
                .client
                .request_vinyl_voice_songs(read, revision, permission);
        }
        Box::pin(async move {
            let voice_page = self.voice_page_authority(revision, permission)?;
            let (path, body) = read.into_request();
            let ticket = voice_page.begin(voice_page_fixture::Lane::Songs, path, &body);
            let response = self
                .voice_songs
                .as_ref()
                .map_or_else(|| voice_page.songs(), |fixture| fixture.songs(path, body));
            voice_page.finish(ticket, Ok(response))
        })
    }

    fn song_like_scope_revision(&self, _signed_pc: bool) -> Result<Option<String>> {
        Ok(self
            .voice_songs
            .as_ref()
            .map(|_| voice_songs_fixture::REVISION.to_owned()))
    }

    fn request_guarded_song_like<'a>(
        &'a self,
        write: GuardedSongLike,
        permission: &'a SongLikePermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let fixture = self.voice_songs.as_ref().ok_or(Error::State(
                "included songs guarded like proof unavailable",
            ))?;
            permission()?;
            dispatched.store(true, Ordering::Release);
            let (path, body) = write.into_request();
            Ok(fixture.like(path, body))
        })
    }

    fn song_card_download_scope_revision(&self) -> Result<Option<String>> {
        if let Some(liked) = &self.voice_song_group_liked {
            return liked.client.song_card_download_scope_revision();
        }
        Ok(self
            .voice_song_download
            .as_ref()
            .map(|_| voice_songs_fixture::REVISION.to_owned()))
    }

    fn song_card_playlist_scope_revision(&self) -> Result<Option<String>> {
        self.song_card_download_scope_revision()
    }

    fn song_card_playlist_create_scope_revision(&self) -> Result<Option<String>> {
        self.song_card_download_scope_revision()
    }

    fn song_card_collection_scope_revision(&self) -> Result<Option<String>> {
        if let Some(liked) = &self.voice_song_group_liked {
            return liked.client.song_card_collection_scope_revision();
        }
        let enabled = self.voice_song_collect.is_some()
            || self
                .voice_song_download
                .as_ref()
                .is_some_and(|proof| proof.group_picker());
        Ok(enabled.then(|| voice_songs_fixture::REVISION.to_owned()))
    }

    fn song_card_collection_create_scope_revision(&self) -> Result<Option<String>> {
        let enabled = self
            .voice_song_collect
            .as_ref()
            .is_some_and(|proof| proof.create_enabled())
            || self
                .voice_song_download
                .as_ref()
                .is_some_and(|proof| proof.group_picker_create());
        Ok(enabled.then(|| voice_songs_fixture::REVISION.to_owned()))
    }

    fn song_card_system_liked_scope_revision(&self) -> Result<Option<String>> {
        self.voice_song_group_liked
            .as_ref()
            .map_or(Ok(None), |liked| {
                liked.client.song_card_system_liked_scope_revision()
            })
    }

    fn request_song_card_system_liked<'a>(
        &'a self,
        read: SongCardLikedRead,
        revision: &'a str,
        permission: &'a PlaylistAddPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        let Some(liked) = &self.voice_song_group_liked else {
            return Box::pin(async { panic!("unsigned proof attempted signed system-liked read") });
        };
        liked
            .client
            .request_song_card_system_liked(read, revision, permission)
    }

    fn request_guarded_playlist_add<'a>(
        &'a self,
        write: GuardedPlaylistAdd,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        if let Some(liked) = &self.voice_song_group_liked {
            return liked
                .client
                .request_guarded_playlist_add(write, permission, dispatched);
        }
        Box::pin(async move {
            if let Some(proof) = &self.voice_song_collect {
                let (path, body) = write.into_request()?;
                permission()?;
                dispatched.store(true, Ordering::Release);
                return proof.add(path, body);
            }
            let proof = self
                .voice_song_download
                .as_ref()
                .expect("isolated SongCard add");
            let (path, body) = write.into_request()?;
            permission()?;
            dispatched.store(true, Ordering::Release);
            proof.add(path, body)
        })
    }

    fn request_guarded_playlist_create<'a>(
        &'a self,
        write: GuardedPlaylistCreate,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(proof) = self
                .voice_song_collect
                .as_ref()
                .filter(|proof| proof.create_enabled())
            {
                let (path, body) = write.into_request();
                permission()?;
                dispatched.store(true, Ordering::Release);
                return Ok(proof.create(path, body));
            }
            let proof = self
                .voice_song_download
                .as_ref()
                .expect("isolated SongCard create");
            let (path, body) = write.into_request();
            permission()?;
            dispatched.store(true, Ordering::Release);
            Ok(proof.create(path, body))
        })
    }

    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(liked) = &self.voice_song_group_liked {
                match path {
                    voice_song_group_liked_fixture::ACCOUNT => {
                        return liked.client.request(path, body, cache).await;
                    }
                    "/api/song/like/get" => return Ok(liked.resource_membership(&body)),
                    _ => {}
                }
            }
            if let Some(actions) = &self.voice_fresh_actions {
                if path == voice_fresh_actions_fixture::ACCOUNT {
                    return actions.client.request(path, body, cache).await;
                }
                if let Some(reply) = actions.route(path, &body) {
                    return reply;
                }
            }
            if let Some(collect) = &self.voice_song_collect
                && let Some(reply) = collect.read(path, &body, cache)
            {
                return Ok(reply);
            }
            if let Some(download) = &self.voice_song_download
                && let Some(reply) = download.read(path, &body, cache)
            {
                return reply;
            }
            if let Some(voice) = &self.voice_detail
                && let Some(response) = voice.profile(path, &body)
            {
                return Ok(response);
            }
            if let Some(collect) = &self.voice_podcast_collect
                && let Some(response) = collect.response(path, &body, cache)
            {
                return Ok(response);
            }
            if let Some(shelf) = &self.voice_recommendations
                && let Some(response) = shelf.route(path, &body, cache)
            {
                return Ok(response);
            }
            if let Some(songs) = &self.voice_songs
                && let Some(response) = songs.read(path, &body, cache)
            {
                return Ok(response);
            }
            if let Some(scene) = &self.scene_home {
                return match scene.response(path, &body) {
                    Some(response) => {
                        if matches!(
                            path,
                            "/api/voice/homepage/block/content" | "/api/dj/program/detail"
                        ) {
                            response.map(|response| self.decorate_scene_voice(response))
                        } else {
                            response
                        }
                    }
                    None => {
                        scene_home_fixture::baseline(&self.scene_baseline, path, body, cache).await
                    }
                };
            }
            if let Some(fixture) = &self.badges
                && let Some(response) = fixture.read(path, &body, cache)
            {
                return response;
            }
            if path == "/api/djradio/my/radio/recommend" {
                assert!(!cache);
                assert_eq!(body, json!({}));
                return Ok(json!({"code":200,"data":[]}));
            }
            if path.contains("clear")
                || path.contains("submit")
                || matches!(path, PROGRESS | "/api/user/setting/update")
            {
                panic!("unexpected synthetic remote action: {path}");
            }
            if (self.program_menu || self.recent_program_menu)
                && path == "/api/resource/commentInfo/list"
            {
                assert!(!cache);
                assert_eq!(body["resourceType"], 1);
                let ids: Vec<String> = serde_json::from_str(body["resourceIds"].as_str().unwrap())?;
                let [id] = ids.as_slice() else {
                    panic!("one program menu count target");
                };
                let count = match id.as_str() {
                    "501" => 51,
                    "503" => 53,
                    "504" => 54,
                    _ => panic!("unexpected program count target"),
                };
                self.calls.lock().unwrap().push(json!([path, body]));
                if self.recent_program_menu
                    && id == "501"
                    && !self.recent_menu_count_held.swap(true, Ordering::SeqCst)
                {
                    self.recent_menu_count_released.notified().await;
                    self.recent_menu_count_delivered
                        .store(true, Ordering::SeqCst);
                }
                return Ok(
                    json!({"code":200,"data":[{"threadId":format!("A_DJ_1_{id}"),"commentCount":count}]}),
                );
            }
            if (self.program_menu || self.recent_program_menu)
                && matches!(
                    path,
                    "/api/v1/resource/comments/A_DJ_1_503"
                        | "/api/v1/resource/hotcomments/A_DJ_1_503"
                )
            {
                self.calls.lock().unwrap().push(json!([path, body]));
                return Ok(
                    json!({"code":200,"comments":[],"hotComments":[],"topComments":[],"more":false,"hasMore":false,"total":0}),
                );
            }
            if (self.program_menu || self.recent_program_menu)
                && path == "/api/song/enhance/privilege"
            {
                panic!("program menu unexpectedly requested song sharing privileges");
            }
            if matches!(
                path,
                RECENT
                    | "/api/dj/program/detail"
                    | "/api/dj/play/url/check"
                    | "/api/song/enhance/player/url/v1"
            ) {
                self.calls.lock().unwrap().push(json!([path, body]));
            }
            Ok(match path {
                RECENT if self.recent_program_menu => {
                    if body == json!({}) {
                        let mut next = voice(503);
                        next.as_object_mut().unwrap().remove("mainSong");
                        json!({"code":200,"data":{"data":[voice(501),next,voice(505)],"updateTime":CURSOR,"hasMore":true}})
                    } else {
                        assert_eq!(body, json!({"updateTime":CURSOR}));
                        json!({"code":200,"data":{"data":[voice(504),voice(506)],"hasMore":false}})
                    }
                }
                RECENT if self.progress_enabled => {
                    assert_eq!(body, json!({}));
                    let rows = [501, 503, 504, 506].map(voice);
                    json!({"code":200,"data":{"data":rows,"hasMore":false,"newVoiceCount":0}})
                }
                RECENT if body == json!({}) => match self.first.fetch_add(1, Ordering::SeqCst) {
                    0 => return Err(Error::Transport("synthetic first failure".into())),
                    1 => {
                        let mut rows = (501..=518).map(voice).collect::<Vec<_>>();
                        rows[2].as_object_mut().unwrap().remove("mainSong");
                        json!({"code":200,"data":{"data":rows,"updateTime":CURSOR,"hasMore":true,"newVoiceCount":7}})
                    }
                    2 => json!({"code":200,"data":{"data":[],"hasMore":false}}),
                    _ => panic!("unexpected first-page replay"),
                },
                RECENT => {
                    assert_eq!(body, json!({"updateTime":CURSOR}));
                    match self.next.fetch_add(1, Ordering::SeqCst) {
                        0 => return Err(Error::Transport("synthetic continuation failure".into())),
                        1 => {
                            json!({"code":200,"data":{"data":[voice(518),voice(519),voice(520)],"hasMore":false,"newVoiceCount":99}})
                        }
                        _ => panic!("unexpected continuation replay"),
                    }
                }
                "/api/my/podcast/page/list" => {
                    json!({"code":200,"data":{"radioIdList":[8],"myPodcastVOList":[radio()]}})
                }
                "/api/content/my/liked/voice" => {
                    json!({"code":200,"data":{"data":[],"hasMore":false}})
                }
                "/api/djradio/v3/get" if self.program_menu => {
                    let mut data = radio();
                    data["programCount"] = json!(3);
                    json!({"code":200,"data":data})
                }
                "/api/djradio/v3/get" => json!({"code":200,"data":radio()}),
                "/api/v6/dj/program/byradio" if self.program_menu => {
                    let mut ids = [501, 503, 504];
                    if body["asc"] == false {
                        ids.reverse();
                    }
                    json!({"code":200,"data":{"programs":ids.map(voice),"count":3,"more":false,"asc":body["asc"]}})
                }
                "/api/v6/dj/program/byradio" => {
                    json!({"code":200,"data":{"programs":[voice(501)],"count":1,"more":false,"asc":true}})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":0}})
                }
                "/api/dj/program/detail" => {
                    let id = body["id"].as_str().unwrap().parse::<u64>().unwrap();
                    assert!(matches!(id, 501 | 503 | 504 | 506));
                    json!({"code":200,"program":voice(id)})
                }
                "/api/dj/play/url/check" => {
                    let id = body["voiceId"].as_str().unwrap().parse::<u64>().unwrap();
                    assert!(matches!(id, 501 | 503 | 504 | 506));
                    json!({"code":200,"data":{"voiceId":id,"songId":1000+id,"canPlay":true}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(body["ids"].as_str().unwrap())?;
                    assert!(
                        matches!(ids[0].as_u64(), Some(1501 | 1503 | 1504 | 1506))
                            || matches!(ids[0].as_str(), Some("1501" | "1503" | "1504" | "1506"))
                            || self.progress_enabled
                                && ids[0]
                                    .as_u64()
                                    .or_else(|| ids[0].as_str().and_then(|id| id.parse().ok()))
                                    .is_some_and(|id| (1..=40).contains(&id))
                    );
                    json!({"code":200,"data":[{"id":ids[0],"url":MEDIA,"code":200,"md5":"fixture","level":"standard","expi":120}]})
                }
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
                "/api/link/position/show/resource" => {
                    json!({"code":200,"data":{"commonResourceList":[]}})
                }
                _ => return self.base.request(path, body, cache).await,
            })
        })
    }
}

#[cfg(target_os = "linux")]
fn locale(category: libc::c_int) -> String {
    // SAFETY: null reads the initialized locale; copy the C string immediately.
    let value = unsafe { libc::setlocale(category, std::ptr::null()) };
    assert!(!value.is_null());
    unsafe { std::ffi::CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned()
}

fn main() {
    // SAFETY: before GTK/runtime initialization and any worker thread.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .unwrap();
    let output = root.join("first");
    fs::create_dir(&output).unwrap();
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc).unwrap();
    let progress_enabled = std::env::var_os("CUMULUS_PODCAST_PROGRESS_PROOF").is_some();
    let program_menu = std::env::var("CUMULUS_PODCAST_PROGRAM_MENU_PROOF").as_deref() == Ok("1");
    let recent_program_menu =
        std::env::var("CUMULUS_PODCAST_RECENT_PROGRAM_MENU_PROOF").as_deref() == Ok("1");
    assert!(!(program_menu && recent_program_menu));
    let recommendations_enabled =
        std::env::var("CUMULUS_PODCAST_RECOMMENDATIONS_PROOF").as_deref() == Ok("1");
    let badges_enabled = recommendations_enabled
        || std::env::var("CUMULUS_PODCAST_BADGES_PROOF").as_deref() == Ok("1");
    let voice_detail_enabled = std::env::var_os("CUMULUS_VOICE_DETAIL_PROOF").is_some();
    let voice_page_proof = std::env::var("CUMULUS_VOICE_PAGE_PROOF").as_deref() == Ok("1");
    assert!(!voice_page_proof || voice_detail_enabled);
    let voice_fresh_actions_enabled =
        std::env::var("CUMULUS_VOICE_FRESH_ACTIONS_PROOF").as_deref() == Ok("1");
    assert!(!voice_fresh_actions_enabled || voice_page_proof);
    let voice_podcast_collect_enabled =
        std::env::var("CUMULUS_VOICE_PODCAST_COLLECT_PROOF").as_deref() == Ok("1");
    let voice_recommendations_enabled =
        std::env::var("CUMULUS_VOICE_RECOMMENDATIONS_PROOF").as_deref() == Ok("1");
    let voice_songs_case = std::env::var("CUMULUS_VOICE_SONGS_PROOF").ok();
    assert!(
        voice_songs_case
            .as_deref()
            .is_none_or(|case| matches!(case, "double" | "cover" | "dense"))
    );
    let voice_song_download_case = std::env::var("CUMULUS_VOICE_SONG_DOWNLOAD_PROOF").ok();
    let voice_song_collect_enabled =
        std::env::var("CUMULUS_VOICE_SONG_COLLECT_PROOF").as_deref() == Ok("1");
    let voice_song_group_liked_enabled =
        std::env::var("CUMULUS_VOICE_SONG_GROUP_LIKED_PROOF").as_deref() == Ok("1");
    let voice_song_collect_creation = std::env::var("CUMULUS_VOICE_SONG_COLLECT_CREATE_PROOF").ok();
    assert!(
        voice_song_collect_creation.is_none()
            || voice_song_collect_enabled && !voice_song_group_liked_enabled
    );
    assert!(
        !voice_song_group_liked_enabled
            || (voice_song_collect_enabled && !voice_fresh_actions_enabled)
    );
    assert!(!(voice_songs_case.is_some() && voice_song_download_case.is_some()));
    assert!(
        !voice_song_collect_enabled
            || (voice_songs_case.is_none() && voice_song_download_case.is_none())
    );
    let voice_song_download = voice_song_download_case
        .as_deref()
        .map(|case| Arc::new(voice_song_download_fixture::Fixture::new(case)));
    let voice_songs_enabled =
        voice_songs_case.is_some() || voice_song_download.is_some() || voice_song_collect_enabled;
    let palette_case = std::env::var("CUMULUS_SCENE_FM_PALETTE_PROOF")
        .ok()
        .or_else(|| voice_detail_enabled.then(|| "header".to_owned()));
    let palette = palette_case
        .clone()
        .map(|case| Arc::new(scene_palette_fixture::PaletteMedia::new(case)));
    let palette_enabled = palette.is_some();
    let scene_home = std::env::var("CUMULUS_PODCAST_SCENE_HOME_PROOF")
        .ok()
        .or_else(|| palette_enabled.then(|| "automatic".to_owned()))
        .map(scene_home_fixture::SceneHome::new);
    let scene_enabled = scene_home.is_some();
    assert!(!(scene_enabled && progress_enabled));
    assert!(
        !voice_podcast_collect_enabled
            || scene_enabled
                && !palette_enabled
                && !voice_detail_enabled
                && !badges_enabled
                && !progress_enabled
    );
    assert!(!badges_enabled || (!scene_enabled && !progress_enabled && !voice_detail_enabled));
    assert!(
        !voice_recommendations_enabled
            || scene_enabled
                && !palette_enabled
                && !voice_detail_enabled
                && !voice_podcast_collect_enabled
                && !badges_enabled
                && !progress_enabled
    );
    assert!(
        !voice_songs_enabled
            || scene_enabled
                && !palette_enabled
                && !voice_detail_enabled
                && !voice_podcast_collect_enabled
                && !voice_recommendations_enabled
                && !badges_enabled
                && !progress_enabled
    );
    let voice_page = scene_enabled.then(|| {
        Arc::new(voice_page_fixture::Fixture::new(
            voice_page_proof,
            voice_fresh_actions_enabled,
        ))
    });
    let voice_fresh_actions = voice_fresh_actions_enabled.then(|| {
        voice_fresh_actions_fixture::Fixture::open(
            registry.store(),
            voice_page
                .as_ref()
                .expect("isolated VoicePage mode")
                .clone(),
        )
        .unwrap()
    });
    let voice_songs = voice_songs_case
        .as_deref()
        .or_else(|| {
            voice_song_download_case.as_deref().map(|case| {
                if case == "group_purchased_playlist" {
                    "group_purchased_playlist_download"
                } else if case == "group_purchased" {
                    "group_purchased_download"
                } else if case.starts_with("group_playlist") {
                    "group_playlist_download"
                } else if case.starts_with("group_") {
                    "group_download"
                } else if case.starts_with("purchased_") {
                    "purchased_download"
                } else {
                    "download"
                }
            })
        })
        .or(voice_song_collect_enabled.then_some("collect"))
        .map(|case| Arc::new(voice_songs_fixture::Fixture::new(case)));
    let signed_group_download = voice_song_download
        .as_ref()
        .filter(|fixture| fixture.uses_signed_liked())
        .cloned();
    let voice_song_group_liked =
        (voice_song_group_liked_enabled || signed_group_download.is_some()).then(|| {
            voice_song_group_liked_fixture::Fixture::open(
                registry.store(),
                voice_songs
                    .as_ref()
                    .expect("isolated group liked mode")
                    .clone(),
                signed_group_download,
            )
            .unwrap()
        });
    let api = Arc::new(Library {
        progress_enabled,
        program_menu,
        recent_program_menu,
        badges: badges_enabled.then(|| badge_fixture::Fixture::new(recommendations_enabled)),
        scene_home,
        palette: palette.clone(),
        voice_detail: voice_detail_enabled.then(voice_detail_fixture::VoiceDetail::default),
        voice_page,
        voice_fresh_actions,
        voice_podcast_collect: voice_podcast_collect_enabled
            .then(voice_podcast_collect_fixture::Fixture::default),
        voice_recommendations: voice_recommendations_enabled
            .then(voice_recommendation_fixture::Fixture::default),
        voice_songs,
        voice_song_group_liked,
        voice_song_download: voice_song_download.clone(),
        voice_song_collect: voice_song_collect_enabled.then(|| {
            voice_song_collect_fixture::Fixture::new(voice_song_collect_creation.as_deref())
        }),
        ..Default::default()
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let wave = relay_fixture::wave(&root).unwrap();
    let synthetic_download_root = root.join("download-output");
    let play_completed =
        std::env::var("CUMULUS_VOICE_SONG_DOWNLOAD_LOCAL_PLAY_PROOF").as_deref() == Ok("1");
    assert!(!play_completed || voice_song_download_case.as_deref() == Some("liked"));
    let allowed_download = play_completed.then(|| synthetic_download_root.clone());
    let mut client = MusicClient::with_session(session, registry).with_audio_factory(move || {
        let audio = relay_fixture::NativeAudio::open(wave.clone())?;
        Ok(Box::new(match &allowed_download {
            Some(directory) => audio.allow_download_directory(directory.clone()),
            None => audio,
        }))
    });
    if let Some(palette) = palette {
        client = client.with_scene_fm_palette_backend(palette);
    }
    if let Some(backend) = voice_song_download {
        client = client.with_download_backend(backend);
    }
    if voice_song_download_case.is_some() {
        fs::create_dir(&synthetic_download_root).unwrap();
    }
    let mut context = assets::context();
    let mut config = context.config().app.windows[0].clone();
    config.width = 960.0;
    config.height = 640.0;
    context.config_mut().app.windows[0].create = false;
    let source = std::env::var_os("CUMULUS_UX_SCRIPT").map_or_else(
        || {
            if recent_program_menu {
                include_str!("podcast_recent_program_menu_ui_probe.js").to_owned()
            } else if program_menu {
                include_str!("podcast_program_menu_ui_probe.js").to_owned()
            } else if badges_enabled {
                include_str!("podcast_badges_ui_probe.js").to_owned()
            } else if voice_song_collect_enabled {
                include_str!("voice_song_collect_ui_probe.js").to_owned()
            } else if voice_song_download_case
                .as_deref()
                .is_some_and(|case| case.starts_with("group_"))
            {
                include_str!("voice_song_group_download_ui_probe.js").to_owned()
            } else if voice_song_download_case.is_some() {
                include_str!("voice_song_download_ui_probe.js").to_owned()
            } else if voice_songs_enabled {
                include_str!("voice_songs_ui_probe.js").to_owned()
            } else if voice_recommendations_enabled {
                include_str!("voice_recommendation_ui_probe.js").to_owned()
            } else if voice_podcast_collect_enabled {
                include_str!("voice_podcast_collect_ui_probe.js").to_owned()
            } else if voice_page_proof {
                include_str!("voice_page_ui_probe.js").to_owned()
            } else if voice_detail_enabled {
                include_str!("voice_detail_ui_probe.js").to_owned()
            } else if palette_enabled {
                if palette_case
                    .as_deref()
                    .is_some_and(|case| case.starts_with("pending_"))
                {
                    include_str!("scene_palette_pending_ui_probe.js").to_owned()
                } else {
                    include_str!("scene_palette_ui_probe.js").to_owned()
                }
            } else if scene_enabled {
                include_str!("podcast_scene_home_ui_probe.js").to_owned()
            } else if progress_enabled {
                include_str!("podcast_progress_ui_probe.js").to_owned()
            } else {
                include_str!("podcast_recent_ui_probe.js").to_owned()
            }
        },
        |path| fs::read_to_string(path).expect("explicit synthetic script"),
    );
    let source = if recommendations_enabled {
        format!("window.__podcastRecommendationsProof = true;\n{source}")
    } else {
        source
    };
    let source = if voice_fresh_actions_enabled {
        format!("window.__voiceFreshActionsProof = true;\n{source}")
    } else if let Some(mode) = &voice_song_collect_creation {
        format!(
            "window.__voiceSongCollectCreateProof = {};\n{source}",
            json!(mode)
        )
    } else if voice_song_group_liked_enabled {
        format!("window.__voiceSongGroupLikedProof = true;\n{source}")
    } else {
        source
    };
    let source = if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
        capture_ack::script(&source)
    } else {
        source
    };
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            if progress_enabled || scene_enabled || badges_enabled || program_menu || recent_program_menu {
                fs::write(output.join("locale.json"), serde_json::to_vec(&json!({
                    "numeric":locale(libc::LC_NUMERIC),
                    "messages":locale(libc::LC_MESSAGES),
                    "characters":locale(libc::LC_CTYPE),
                }))?)?;
            }
            app.manage(client);
            app.add_capability(r#"{"identifier":"podcast-recent-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("ux-proof", move |event| {
                let mut value: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = value["stage"].as_str().unwrap_or("").to_owned();
                if recent_program_menu && stage == "recent_program_menu_poll" {
                    let status = json!({
                        "held":api.recent_menu_count_held.load(Ordering::SeqCst),
                        "delivered":api.recent_menu_count_delivered.load(Ordering::SeqCst),
                    });
                    handle.get_webview_window("main").unwrap().eval(format!(
                        "window.__recentProgramMenuStatus = {status};"
                    )).unwrap();
                    return;
                }
                if recent_program_menu && stage == "recent_program_menu_release" {
                    api.recent_menu_count_released.notify_one();
                    return;
                }
                if stage == "palette_poll" {
                    if let (Some(palette), Some(nonce)) = (&api.palette, value["nonce"].as_u64()) {
                        handle.get_webview_window("main").unwrap().eval(format!(
                            "window.__paletteStatus = [{nonce}, {}];", palette.snapshot()
                        )).unwrap();
                    }
                    return;
                }
                if stage == "palette_release" {
                    api.palette.as_ref().expect("isolated palette fixture").release();
                    return;
                }
                if let Some(page) = &api.voice_page {
                    if stage == "voice_page_poll" {
                        if let Some(nonce) = value["nonce"].as_u64() {
                            handle.get_webview_window("main").unwrap().eval(format!(
                                "window.__voicePageNative = [{nonce}, {}];", page.snapshot()
                            )).unwrap();
                        }
                        return;
                    }
                    if stage == "voice_page_release_failure" {
                        page.release_failure();
                        return;
                    }
                }
                if let Some(shelf) = &api.voice_recommendations {
                    if stage == "voice_recommendation_poll" {
                        if let Some(nonce) = value["nonce"].as_u64() {
                            handle.get_webview_window("main").unwrap().eval(format!(
                                "window.__voiceRecommendationNative = [{nonce}, {}];", shelf.snapshot()
                            )).unwrap();
                        }
                        return;
                    }
                    if stage == "voice_recommendation_release" {
                        shelf.release();
                        return;
                    }
                }
                if let Some(songs) = &api.voice_songs
                    && stage == "voice_songs_poll"
                {
                    if let Some(nonce) = value["nonce"].as_u64() {
                        handle.get_webview_window("main").unwrap().eval(format!(
                            "window.__voiceSongsNative = [{nonce}, {}];", songs.snapshot()
                        )).unwrap();
                    }
                    return;
                }
                if let Some(download) = &api.voice_song_download {
                    if stage == "voice_song_download_poll" {
                        if let Some(nonce) = value["nonce"].as_u64() {
                            handle.get_webview_window("main").unwrap().eval(format!(
                                "window.__voiceSongDownloadNative = [{nonce}, {}];", download.snapshot()
                            )).unwrap();
                        }
                        return;
                    }
                    if stage == "voice_song_download_reveal" {
                        download.reveal_recovery();
                        handle.get_webview_window("main").unwrap().eval(
                            "window.__voiceSongDownloadRevealed = true;"
                        ).unwrap();
                        return;
                    }
                }
                if progress_enabled && stage == "progress_poll" {
                    if let Some(nonce) = value["nonce"].as_u64() {
                        let count = api.progress_uploads.lock().unwrap().len();
                        handle.get_webview_window("main").unwrap().eval(format!(
                            "window.__podcastProgressCount = [{nonce}, {count}];",
                        )).unwrap();
                    }
                    return;
                }
                if !(["complete", "failed"].contains(&stage.as_str())
                    || stage.starts_with("ux_") && stage.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')) {
                    return;
                }
                if let Some(badges) = &api.badges {
                    value["podcastBadgeTrace"] = json!(badges.trace());
                }
                if let Some(voice)=&api.voice_detail {
                    value["voiceDetailCalls"]=json!(voice.calls());
                }
                if let Some(page) = &api.voice_page {
                    value["voicePageReads"] = page.snapshot();
                }
                if let Some(actions) = &api.voice_fresh_actions {
                    value["voiceFreshActionTrace"] = actions.report();
                }
                if let Some(palette) = &api.palette {
                    value["paletteRequests"] = palette.snapshot();
                }
                if let Some(collect) = &api.voice_podcast_collect {
                    value["voicePodcastCollectTrace"] = json!(collect.trace());
                }
                if let Some(shelf) = &api.voice_recommendations {
                    value["voiceRecommendationsTrace"] = shelf.snapshot();
                }
                if let Some(collect) = &api.voice_song_collect {
                    value["voiceSongCollectTrace"] = collect.snapshot();
                }
                if let Some(liked) = &api.voice_song_group_liked {
                    value["voiceSongGroupLikedTrace"] = liked.report();
                }
                if let Some(download) = &api.voice_song_download {
                    value["voiceSongDownloadTrace"] = download.snapshot();
                }
                if let Some(songs) = &api.voice_songs {
                    value["voiceSongsTrace"] = songs.snapshot();
                }
                if let Some(scene) = &api.scene_home {
                    value["calls"] = json!(scene.calls());
                } else if stage == "complete" || program_menu || recent_program_menu {
                    value["calls"] = json!(api.calls.lock().unwrap().clone());
                }
                if progress_enabled {
                    value["progressUploads"] = json!(api.progress_uploads.lock().unwrap().clone());
                }
                fs::write(output.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                let handle = handle.clone();
                let output = output.clone();
                tauri::async_runtime::spawn(async move {
                    if stage == "complete" || stage == "failed" {
                        handle.exit(if stage == "complete" {0} else {1});
                    } else if capture_ack::acknowledge(&handle, &output, &stage, true).await
                        && (progress_enabled || scene_enabled || badges_enabled || program_menu || recent_program_menu)
                    {
                        handle.get_webview_window("main").unwrap().eval(format!(
                            "window.__podcastProgressStage = {};",
                            json!(stage)
                        )).unwrap();
                    }
                });
            });
            tauri::WebviewWindowBuilder::from_config(app, &config)?.data_directory(root.join("webview")).build()?;
            Ok(())
        })
        .on_page_load(move |view, payload| {
            if payload.event() == PageLoadEvent::Finished && view.label() == "main" {
                if palette_enabled {
                    view.eval(format!("window.__scenePaletteCase = {};", json!(palette_case))).unwrap();
                } else if scene_enabled {
                    let case = std::env::var("CUMULUS_PODCAST_SCENE_HOME_PROOF").unwrap();
                    view.eval(format!("window.__podcastSceneHomeCase = {};", json!(case))).unwrap();
                }
                if let Some(case) = &voice_songs_case {
                    view.eval(format!("window.__voiceSongsCase = {};", json!(case))).unwrap();
                }
                if let Some(case) = &voice_song_download_case {
                    view.eval(format!(
                        "window.__voiceSongDownloadCase = {}; window.__voiceSongDownloadRoot = {}; window.__voiceSongDownloadLocalPlay = {play_completed};",
                        json!(case), json!(synthetic_download_root)
                    )).unwrap();
                }
                view.eval(&source).unwrap();
            }
        })
        .run(context).expect("podcast recent proof");
}
