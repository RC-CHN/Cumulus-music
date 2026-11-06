//! Production UI/IPC inspection with isolated synthetic account and preferences.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/card_source_fixture.rs"]
mod card_source_fixture;
#[path = "support/daily_history_fixture.rs"]
mod daily_history_fixture;
#[path = "support/download_page_fixture.rs"]
mod download_page_fixture;
#[path = "support/effect_directory_fixture.rs"]
mod effect_directory_fixture;
#[path = "support/effect_peq_fixture.rs"]
mod effect_peq_fixture;
#[path = "support/external_import_fixture.rs"]
mod external_import_fixture;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../../core/tests/support/fm.rs"]
mod fm_domain;
#[path = "support/fm_feedback_fixture.rs"]
mod fm_feedback_fixture;
#[path = "support/intelligence_start_fixture.rs"]
mod intelligence_start_fixture;
#[path = "support/intelligence_ui_fixture.rs"]
mod intelligence_ui_fixture;
#[path = "support/latest_releases_fixture.rs"]
mod latest_releases_fixture;
#[path = "support/library_fixture.rs"]
mod library_fixture;
#[path = "support/like_recovery_fixture.rs"]
mod like_recovery_fixture;
#[path = "support/media_shortcuts.rs"]
mod media_shortcuts;
#[path = "support/member_lifecycle_fixture.rs"]
mod member_lifecycle_fixture;
#[path = "support/member_sign_fixture.rs"]
mod member_sign_fixture;
#[path = "support/member_welfare_fixture.rs"]
mod member_welfare_fixture;
#[path = "support/message_fixture.rs"]
mod message_fixture;
#[path = "support/mode_fixture.rs"]
mod mode_fixture;
#[path = "support/playback_power_guide_fixture.rs"]
mod playback_power_guide_fixture;
#[path = "support/playback_quality_fixture.rs"]
mod playback_quality_fixture;
#[path = "support/playlist_collect_fixture.rs"]
mod playlist_collect_fixture;
#[path = "support/playlist_cover_layout_fixture.rs"]
mod playlist_cover_layout_fixture;
#[path = "support/playlist_cover_transfer_fixture.rs"]
mod playlist_cover_transfer_fixture;
#[path = "support/playlist_editor_fixture.rs"]
mod playlist_editor_fixture;
#[path = "support/playlist_essential_fixture.rs"]
mod playlist_essential_fixture;
#[path = "support/playlist_layout_fixture.rs"]
mod playlist_layout_fixture;
#[path = "support/playlist_square_fixture.rs"]
mod playlist_square_fixture;
#[path = "support/queue_collection_fixture.rs"]
mod queue_collection_fixture;
#[path = "support/queue_more_fixture.rs"]
mod queue_more_fixture;
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[path = "support/remote_skin_fixture.rs"]
mod remote_skin_fixture;
#[path = "support/search_artist_top50_fixture.rs"]
mod search_artist_top50_fixture;
#[path = "support/search_download_ui_fixture.rs"]
mod search_download_ui_fixture;
#[path = "support/search_fixture.rs"]
mod search_fixture;
#[path = "support/search_mv_fixture.rs"]
mod search_mv_fixture;
#[path = "support/search_privilege_fixture.rs"]
mod search_privilege_fixture;
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
#[path = "support/style_home_fixture.rs"]
mod style_home_fixture;
#[path = "support/style_recommendations_fixture.rs"]
mod style_recommendations_fixture;
#[path = "support/system_master_volume_fixture.rs"]
mod system_master_volume_fixture;
#[path = "support/tone_geometry.rs"]
mod tone_geometry;
#[path = "support/vertical_zone_fixture.rs"]
mod vertical_zone_fixture;
#[path = "support/vinyl_comments_fixture.rs"]
mod vinyl_comments_fixture;
#[path = "support/vinyl_similar_fixture.rs"]
mod vinyl_similar_fixture;
#[path = "support/voice_download_fixture.rs"]
mod voice_download_fixture;
#[path = "support/volume_fixture.rs"]
mod volume_fixture;
#[path = "support/zone_fm_preview_fixture.rs"]
mod zone_fm_preview_fixture;
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

#[path = "support/search_signed_fixture.rs"]
mod search_signed_fixture;

#[derive(Default)]
struct Library {
    signed_search: Option<Arc<search_signed_fixture::SignedSearch>>,
}
#[cfg(target_os = "linux")]
fn startup_locale(category: libc::c_int) -> String {
    // SAFETY: read-only startup query after GTK initialization. Copy the
    // process-owned locale value before querying another category.
    unsafe {
        let value = libc::setlocale(category, std::ptr::null());
        assert!(!value.is_null());
        std::ffi::CStr::from_ptr(value)
            .to_string_lossy()
            .into_owned()
    }
}

fn trial_seek_proof() -> bool {
    std::env::var_os("CUMULUS_TRIAL_SEEK_PROOF").is_some()
}
fn song(id: u64) -> Value {
    if id == 1 && trial_seek_proof() {
        return json!({"id":1,"name":"合成试听歌曲","ar":[{"name":"Cumulus"}],"al":{"name":"界面体验验证"},"dt":100000,"fee":1});
    }
    if id == 1 && std::env::var_os("CUMULUS_PLAYER_LAYOUT_PROOF").is_some() {
        return json!({"id":id,"name":"沿着海岸走到日落之后 · 用于窄窗口排版检查的长歌名（现场特别收录版本）".repeat(2),"ar":[{"name":"SyntheticArtistWithoutWordBreaks".repeat(4)}],"al":{"name":"合成专辑 · 一个很长很长的副标题与发行版本说明".repeat(4)},"dt":6_000_000,"fee":1});
    }
    if id == 2 && std::env::var_os("CUMULUS_PLAYER_LAYOUT_PROOF").is_some() {
        return json!({"id":id,"name":"合成歌曲 2 · A quiet afternoon","ar":[{"id":12,"name":"测试歌手"},{"id":13,"name":"另一位歌手"}],"al":{"id":7,"name":"测试专辑"},"dt":235000,"fee":2});
    }
    json!({"id":id,"name":format!("合成歌曲 {id} · A quiet afternoon"),"ar":[{"name":"Cumulus"}],"al":{"name":"界面体验验证"},"dt":if id==1 {6_000_000} else {235_000},"fee":id%3})
}
fn subscribers_proof() -> bool {
    std::env::var_os("CUMULUS_PLAYLIST_SUBSCRIBERS_UI_PROOF").is_some()
}

static SUBSCRIBER_OFFSETS: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());

fn playlist_comments_proof() -> bool {
    std::env::var_os("CUMULUS_PLAYLIST_COMMENT_TAB_PROOF").is_some()
}

static PLAYLIST_COMMENT_THREADS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static PLAYLIST_COMMENT_DETAILS: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());

fn playlist_tag_browse_proof() -> bool {
    std::env::var_os("CUMULUS_PLAYLIST_TAG_BROWSE_PROOF").is_some()
}
static PLAYLIST_TAG_CALLS: std::sync::Mutex<Vec<Value>> = std::sync::Mutex::new(Vec::new());
static PLAYLIST_CATALOGUE_CALLS: std::sync::Mutex<Vec<Value>> = std::sync::Mutex::new(Vec::new());

#[path = "support/profile_play_record_fixture.rs"]
mod profile_play_record_fixture;
#[path = "support/profile_relations_fixture.rs"]
mod profile_relations_fixture;

fn playlist_creator_proof() -> bool {
    std::env::var_os("CUMULUS_PLAYLIST_CREATOR_PROOF").is_some()
}
static PLAYLIST_CREATOR_CALLS: std::sync::Mutex<Vec<Value>> = std::sync::Mutex::new(Vec::new());
static SEARCH_PROFILE_CALLS: std::sync::Mutex<Vec<Value>> = std::sync::Mutex::new(Vec::new());

fn playlist(id: u64) -> Value {
    intelligence_start_fixture::playlist(playlist_editor_fixture::decorate(
        playlist_layout_fixture::decorate(
            json!({"id":id,"userId":if id<20 {2} else {3},"name":if id==1 {"我喜欢的音乐".into()} else if id==2 {"午后散步 · 一份用来检查较长名称与编辑入口的歌单".into()} else {format!("合成歌单 {id}")},"creator":{"userId":if id<20 {2} else {3},"nickname":"Cumulus"},"specialType":if id==1 {5} else {0},"privacy":10,"trackCount":40,"subscribedCount":if subscribers_proof() {Some(25)} else {None},"description":"沿着熟悉的小路，留一点时间给音乐。这是一段较长的合成简介，用来检查窄窗口里的文字、封面和操作入口。","tags":["轻音乐"],"trackIds":(1..=40).map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>()}),
            id,
        ),
        id,
    ))
}
impl Api for Library {
    fn pc_presentation_credential_revision(&self) -> cumulus_core::Result<Option<String>> {
        self.signed_search.as_ref().map_or_else(
            || self.verified_pc_experiment_revision(),
            |signed| signed.client.pc_presentation_credential_revision(),
        )
    }
    fn pc_presentation_credential_changes(
        &self,
    ) -> cumulus_core::Result<Option<tokio::sync::watch::Receiver<String>>> {
        self.signed_search.as_ref().map_or(Ok(None), |signed| {
            signed.client.pc_presentation_credential_changes()
        })
    }
    fn pc_presentation_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, cumulus_core::protocol::pc_experiments::PcExperimentGroups> {
        if let Some(signed) = self.signed_search.as_ref() {
            signed.client.pc_presentation_groups(revision)
        } else {
            self.verified_pc_experiment_groups(revision)
        }
    }
    fn verified_pc_experiment_revision(&self) -> cumulus_core::Result<Option<String>> {
        let existing = vinyl_comments_fixture::group()
            .or_else(vinyl_similar_fixture::revision)
            .or_else(search_artist_top50_fixture::revision)
            .or_else(|| vertical_zone_fixture::enabled().then(|| "synthetic-vertical-zone".into()));
        if existing.is_some() {
            return Ok(existing);
        }
        self.signed_search.as_ref().map_or(Ok(None), |signed| {
            signed.client.verified_pc_experiment_revision()
        })
    }

    fn verified_pc_experiment_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, cumulus_core::protocol::pc_experiments::PcExperimentGroups> {
        Box::pin(async move {
            let legacy = if vinyl_comments_fixture::group().is_some() {
                Some(vinyl_comments_fixture::assignment(revision))
            } else if vinyl_similar_fixture::enabled() {
                Some(vinyl_similar_fixture::groups(revision))
            } else if search_artist_top50_fixture::enabled() {
                Some(search_artist_top50_fixture::groups(revision))
            } else if vertical_zone_fixture::enabled() {
                Some(vertical_zone_fixture::groups(revision))
            } else {
                None
            };
            if let Some(groups) = legacy {
                return search_signed_fixture::legacy_layout(groups?);
            }
            let signed = self
                .signed_search
                .as_ref()
                .ok_or(cumulus_core::Error::State(
                    "signed search fixture unavailable",
                ))?;
            signed.client.verified_pc_experiment_groups(revision).await
        })
    }

    fn request_vinyl_similar_songs<'a>(
        &'a self,
        read: cumulus_core::protocol::api::VinylSimilarSongsRead,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (path, args) = read.into_request();
            vinyl_similar_fixture::songs(path, args, revision).await
        })
    }

    fn request_vinyl_related_playlists<'a>(
        &'a self,
        read: cumulus_core::protocol::api::VinylRelatedPlaylistsRead,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (path, args) = read.into_request();
            vinyl_similar_fixture::playlists(path, args, revision)
        })
    }

    fn request_vinyl_wiki<'a>(
        &'a self,
        read: cumulus_core::protocol::api::VinylWikiRead,
        revision: &'a str,
        permission: &'a cumulus_core::protocol::api::VinylWikiReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.verified_pc_experiment_revision()?.as_deref() != Some(revision) {
                return Err(cumulus_core::Error::CredentialChanged);
            }
            permission()?;
            let (path, args) = read.into_request();
            vinyl_similar_fixture::wiki(path, args, revision).await
        })
    }

    fn request_vinyl_comment_page<'a>(
        &'a self,
        read: cumulus_core::protocol::api::VinylCommentRead,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.verified_pc_experiment_revision()?.as_deref() != Some(revision) {
                return Err(cumulus_core::Error::CredentialChanged);
            }
            let (path, args) = read.into_request();
            if vinyl_comments_fixture::reports() {
                return vinyl_comments_fixture::bound_report(&path, &args, "bound_read");
            }
            if vinyl_comments_fixture::likes() {
                return vinyl_comments_fixture::bound_like_read(&path, &args);
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
            use cumulus_core::Error;
            if !(vinyl_comments_fixture::composer()
                || vinyl_comments_fixture::likes()
                || vinyl_comments_fixture::reports())
                || self.verified_pc_experiment_revision()?.as_deref() != Some(revision)
            {
                return Err(Error::RequestNotSent(Box::new(Error::CredentialChanged)));
            }
            permission().map_err(|error| Error::RequestNotSent(Box::new(error)))?;
            let (path, args) = write.into_request();
            if vinyl_comments_fixture::reports() {
                return vinyl_comments_fixture::bound_report(path, &args, "bound_write");
            }
            if vinyl_comments_fixture::likes() {
                return vinyl_comments_fixture::bound_like_write(path, &args);
            }
            self.request(path, args, false).await
        })
    }

    fn search_audio_toplist_assignment(&self) -> ApiFuture<'_, bool> {
        Box::pin(async { search_fixture::audio_toplist_assignment() })
    }

    fn search_default_context_revision(&self) -> cumulus_core::Result<Option<String>> {
        Ok(search_fixture::enabled().then(|| "synthetic-search-default-context".into()))
    }

    fn request_search_with_default_context<'a>(
        &'a self,
        path: &'a str,
        args: Value,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.search_default_context_revision()?.as_deref() != Some(revision) {
                return Err(cumulus_core::Error::CredentialChanged);
            }
            self.request(path, args, false).await
        })
    }

    fn observed_server_time_ms(&self) -> Option<u64> {
        member_sign_fixture::observed_server_time_ms()
    }

    fn playback_power_experiment(
        &self,
    ) -> ApiFuture<'_, Option<cumulus_core::protocol::pc_experiments::PcExperimentGroups>> {
        Box::pin(async { playback_power_guide_fixture::assignment() })
    }

    fn queue_more_experiment_groups(
        &self,
    ) -> ApiFuture<'_, cumulus_core::protocol::pc_experiments::PcExperimentGroups> {
        Box::pin(async move {
            if queue_more_fixture::enabled() {
                queue_more_fixture::groups()
            } else {
                Ok(
                    cumulus_core::protocol::pc_experiments::PcExperimentGroups::from_startup_cache(
                        &Value::Null,
                    ),
                )
            }
        })
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, cached: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            // Record account/get before it is delegated to the signed search
            // fixture. The lifecycle fixture deliberately lets that read pass through.
            if let Some(response) = member_lifecycle_fixture::response(path, &args, cached).await {
                return response;
            }
            if path == "/api/w/nuser/account/get"
                && let Some(signed) = &self.signed_search
            {
                return signed.client.request(path, args, cached).await;
            }
            if let Some(response) = vinyl_similar_fixture::ordinary_request(path, &args) {
                return response;
            }
            if let Some(response) = vinyl_comments_fixture::response(path, &args) {
                return response;
            }
            if let Some(response) = zone_fm_preview_fixture::response(path, &args, cached).await {
                return response;
            }
            if let Some(response) = vertical_zone_fixture::response(path, &args) {
                return response;
            }
            if vertical_zone_fixture::enabled()
                && matches!(
                    path,
                    "/api/album/v3/detail"
                        | "/api/album/v4/detail"
                        | "/api/album/privilege"
                        | "/api/album/detail/dynamic"
                        | "/api/song/enhance/privilege"
                )
            {
                return fixture::Library::default().request(path, args, false).await;
            }
            if let Some(reply) = playback_power_guide_fixture::home_response(path, &args) {
                return Ok(reply);
            }
            if let Some(reply) = queue_more_fixture::response(path, &args, cached).await {
                return reply;
            }
            if let Some(reply) = remote_skin_fixture::response(path, &args, cached).await {
                return reply;
            }
            if let Some(reply) = member_welfare_fixture::response(path, &args, cached).await {
                return reply;
            }
            if playlist_tag_browse_proof()
                && let Some(response) = playlist_square_fixture::response(path, &args)
            {
                return response;
            }
            if playlist_tag_browse_proof()
                && let Some(response) = playlist_essential_fixture::response(path, &args)
            {
                return response;
            }
            if let Some(response) = message_fixture::response(path, &args).await {
                return response;
            }
            if let Some(response) = member_sign_fixture::response(path, &args).await {
                return response;
            }
            if let Some(response) = search_mv_fixture::response(path, &args) {
                return response;
            }
            if let Some(response) = search_download_ui_fixture::response(path, &args, cached) {
                return response;
            }
            if let Some(response) = search_fixture::mv_preview_privileges(path, &args, cached) {
                return response;
            }
            if let Some(response) = search_privilege_fixture::response(path, &args, cached) {
                return response;
            }
            if let Some(response) = daily_history_fixture::response(path, &args).await {
                return response;
            }
            if let Some(response) = style_home_fixture::response(path, &args) {
                return response;
            }
            if let Some(response) = style_recommendations_fixture::response(path, &args) {
                return response;
            }
            if let Some(response) = tone_geometry::response(path, &args) {
                return response;
            }
            if let Some(response) = effect_peq_fixture::response(path, &args) {
                return response;
            }
            if let Some(response) = effect_directory_fixture::response(path, &args) {
                return response;
            }
            if let Some(response) = fm_feedback_fixture::response(path, &args).await {
                return response;
            }
            intelligence_start_fixture::before(path).await;
            if let Some(response) = intelligence_start_fixture::response(path, &args).await {
                return response;
            }
            playback_quality_fixture::before(path).await;
            if let Some(response) = mode_fixture::response(path, &args).await {
                return response;
            }
            if let Some(result) = playback_quality_fixture::metadata(path, &args) {
                return result;
            }
            if let Some(result) = voice_download_fixture::response(path, &args) {
                return result;
            }
            if let Some(result) = download_page_fixture::response(path, &args) {
                return result;
            }
            if let Some(result) = library_fixture::response(path, &args).await {
                return result;
            }
            if let Some(result) = playlist_collect_fixture::response(path, &args).await {
                return result;
            }
            if let Some(result) = queue_collection_fixture::response(path, &args) {
                return result;
            }
            profile_play_record_fixture::observe_playback(path, &args);
            if let Some(result) = profile_play_record_fixture::response(path, &args, cached) {
                profile_play_record_fixture::hold_response(path, &args).await;
                return result;
            }
            if let Some(result) = profile_relations_fixture::response(path, &args) {
                return result;
            }
            if let Some(result) = latest_releases_fixture::response(path, &args) {
                return result;
            }
            if latest_releases_fixture::enabled()
                && matches!(
                    path,
                    "/api/album/v3/detail"
                        | "/api/album/v4/detail"
                        | "/api/album/privilege"
                        | "/api/album/detail/dynamic"
                        | "/api/song/enhance/privilege"
                )
            {
                return fixture::Library::default().request(path, args, false).await;
            }
            if let Some(result) = search_artist_top50_fixture::response(path, &args, cached) {
                return result;
            }
            if let Some(result) = search_fixture::response(path, &args) {
                return result;
            }
            if download_page_fixture::enabled()
                && matches!(
                    path,
                    "/api/artist/v3/detail"
                        | "/api/artist/detail/dynamic"
                        | "/api/song/enhance/privilege"
                )
            {
                return fixture::Library::default().request(path, args, false).await;
            }
            if search_fixture::enabled()
                && matches!(
                    path,
                    "/api/artist/v3/detail"
                        | "/api/artist/detail/dynamic"
                        | "/api/album/v3/detail"
                        | "/api/album/privilege"
                        | "/api/album/detail/dynamic"
                        | "/api/song/enhance/privilege"
                )
            {
                if search_fixture::card_failed() {
                    return Err(cumulus_core::Error::Transport("合成卡片读取失败".into()));
                }
                return fixture::Library::default().request(path, args, false).await;
            }
            if std::env::var_os("CUMULUS_PLAYER_LAYOUT_PROOF").is_some()
                && matches!(
                    path,
                    "/api/artist/v3/detail"
                        | "/api/artist/detail/dynamic"
                        | "/api/album/v3/detail"
                        | "/api/album/privilege"
                        | "/api/album/detail/dynamic"
                        | "/api/song/enhance/privilege"
                )
            {
                return fixture::Library::default().request(path, args, false).await;
            }
            Ok(match path {
                "/api/usertool/sound/queryAllSound"
                    if std::env::var_os("CUMULUS_PLAYER_LAYOUT_PROOF").is_some()
                        && args == json!({"exposeType":5}) =>
                {
                    json!({"code":200,"data":[]})
                }
                "/api/rtrs/abt/front/expinfo/list"
                    if external_import_fixture::sidebar_mode().is_some() =>
                {
                    external_import_fixture::sidebar_assignment(&args, cached)
                }
                "/api/rtrs/abt/front/expinfo/list" => json!({"code":200,"data":[]}),
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
                "/api/w/nuser/account/get" => {
                    if external_import_fixture::sidebar_mode().as_deref() == Some("guest") {
                        return Err(cumulus_core::Error::LoginDeferred);
                    }
                    json!({"code":200,"profile":{"userId":2,"nickname":"界面体验验证"}})
                }
                "/api/w/v1/user/detail/501" if search_fixture::enabled() => {
                    SEARCH_PROFILE_CALLS.lock().unwrap().push(args);
                    json!({"code":200,"profile":{"userId":501,"nickname":"合成用户主页 501","signature":"合成公开主页签名"}})
                }
                "/api/user/playlist" if search_fixture::enabled() && args["uid"] == "501" => {
                    SEARCH_PROFILE_CALLS.lock().unwrap().push(args);
                    json!({"code":200,"more":false,"playlist":[{"id":30,"userId":501,"name":"合成公开歌单","privacy":0}]})
                }
                "/api/w/v1/user/detail/3" if playlist_creator_proof() => {
                    PLAYLIST_CREATOR_CALLS.lock().unwrap().push(args);
                    json!({"code":200,"profile":{"userId":3,"nickname":"合成作者 3","signature":"合成签名","follows":21,"followeds":1}})
                }
                "/api/user/playlist" if playlist_creator_proof() && args["uid"] == "3" => {
                    PLAYLIST_CREATOR_CALLS.lock().unwrap().push(args);
                    let mut created = playlist(20);
                    created["privacy"] = json!(0);
                    let mut collected = playlist(2);
                    collected["privacy"] = json!(0);
                    json!({"code":200,"more":false,"playlist":[created,playlist(21),collected]})
                }
                "/api/user/playlist/v2" => {
                    let offset = args["offset"].as_u64().unwrap_or(0) * 10;
                    let rows = (1..=24)
                        .skip(offset as usize)
                        .take(10)
                        .map(playlist)
                        .collect::<Vec<_>>();
                    json!({"code":200,"more":offset+10<24,"playlist":rows})
                }
                "/api/user/playlist" => {
                    // One collected list exercises a menu whose reorder items
                    // are both disabled, without changing production state.
                    let last = if playlist_layout_fixture::enabled() {
                        20
                    } else {
                        24
                    };
                    json!({"code":200,"more":false,"playlist":(1..=last).map(playlist).collect::<Vec<_>>()})
                }
                "/api/song/like/get" => json!({"code":200,"ids":[3,7,10]}),
                "/api/v3/discovery/recommend/songs" => {
                    json!({"code":200,"data":{"dailySongs":(1..=40).map(song).collect::<Vec<_>>()}})
                }
                "/api/v6/playlist/detail" => {
                    let id = args["id"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .or_else(|| args["id"].as_u64())
                        .unwrap_or(2);
                    if playlist_comments_proof() {
                        PLAYLIST_COMMENT_DETAILS.lock().unwrap().push(id);
                    }
                    playlist_editor_fixture::wait_while("hold_read").await;
                    if playlist_editor_fixture::fails_read(id) {
                        return Err(cumulus_core::Error::Response(
                            "synthetic playlist editor read failure",
                        ));
                    }
                    if playlist_layout_fixture::first_read_failure(id) {
                        playlist_layout_fixture::wait_for_release().await;
                        json!({"code":500})
                    } else {
                        let mut value = playlist(id);
                        profile_play_record_fixture::decorate_playlist(id, &mut value);
                        if playlist_creator_proof() && matches!(id, 2 | 20) {
                            value["playCount"] = json!(12600);
                            if id == 20 {
                                value["privacy"] = json!(0);
                            }
                        }
                        json!({"code":200,"playlist":value})
                    }
                }
                "/api/v1/resource/comments/A_PL_0_2" | "/api/v1/resource/comments/A_PL_0_3"
                    if playlist_comments_proof() =>
                {
                    let thread = path.rsplit('/').next().unwrap();
                    PLAYLIST_COMMENT_THREADS.lock().unwrap().push(thread.into());
                    let second = thread == "A_PL_0_3";
                    let content = if second {
                        "第二张合成歌单的评论"
                    } else {
                        "第一张合成歌单的评论"
                    };
                    json!({"code":200,"comments":[{"commentId":if second {102}else{101},
                        "content":content,"time":1700000000000_u64,"liked":false,"likedCount":0,
                        "user":{"userId":3,"nickname":"合成听友"},"beReplied":[]}],
                        "hotComments":[],"topComments":[],"more":false,"total":if second {8}else{12}})
                }
                "/api/playlist/category/list" if playlist_tag_browse_proof() => {
                    let long = args["cat"] == "轻音乐";
                    let promo = long.then(|| {
                        json!({
                            "name":"合成精品入口","cat":"不能作为路由的服务值","action":"unsafe"
                        })
                    });
                    let mut ids = (100..124).collect::<Vec<_>>();
                    if long {
                        ids.extend([3, 4]);
                        ids.extend(124..338);
                    }
                    PLAYLIST_TAG_CALLS.lock().unwrap().push(args);
                    json!({"code":200,"playlists":(100..124).map(|id| json!({"id":id,"name":format!("分类合成歌单 {id}")})).collect::<Vec<_>>(),
                        "playlistIds":ids,"highQuality":promo})
                }
                "/api/playlist/catalogue" if playlist_tag_browse_proof() => {
                    let mut calls = PLAYLIST_CATALOGUE_CALLS.lock().unwrap();
                    calls.push(args);
                    if calls.len() == 1 {
                        return Err(cumulus_core::Error::Transport("合成标签读取中断".into()));
                    }
                    json!({"code":200,"categories":{"1":"风格","0":"语种"},
                        "sub":[{"name":"摇滚","displayName":"摇滚精选","category":1},
                            {"name":"华语","category":0},{"name":"官方","category":0},
                            {"name":"推荐","category":1},{"name":"民谣","category":1}]})
                }
                "/api/playlist/list/get" if playlist_tag_browse_proof() => {
                    let ids: Vec<u32> =
                        serde_json::from_str(args["ids"].as_str().unwrap()).unwrap();
                    assert_eq!(ids.len(), 24);
                    let mut calls = PLAYLIST_TAG_CALLS.lock().unwrap();
                    let first = ids.first() == Some(&3) && !calls.iter().any(|call| call == &args);
                    calls.push(args);
                    if first {
                        return Err(cumulus_core::Error::Transport("合成分类续页中断".into()));
                    }
                    json!({"code":200,"playlists":ids.into_iter().rev().map(|id| json!({"id":id,"name":format!("分类续页 {id}")})).collect::<Vec<_>>()})
                }
                "/api/playlist/subscribers" if subscribers_proof() => {
                    assert_eq!(args["id"], "2");
                    assert_eq!(args["limit"], 24);
                    let offset = args["offset"].as_u64().unwrap();
                    let mut calls = SUBSCRIBER_OFFSETS.lock().unwrap();
                    calls.push(offset);
                    match offset {
                        0 => json!({"code":200,"more":true,"total":25,"subscribers":
                            (1..=24).map(|index| json!({"nickname":format!("合成收藏者 {index}")})).collect::<Vec<_>>()}),
                        24 if calls.iter().filter(|n| **n == 24).count() == 1 => {
                            json!({"code":500})
                        }
                        24 => {
                            json!({"code":200,"more":false,"total":25,"subscribers":[{"userId":3,"nickname":"合成收藏者 25","signature":"合成签名"}]})
                        }
                        _ => panic!("unexpected synthetic subscriber offset"),
                    }
                }
                "/api/v3/song/detail" => {
                    let rows: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    let mut response = json!({"code":200,"songs":rows.iter().map(|row|row["id"].as_str().and_then(|s|s.parse().ok()).or_else(||row["id"].as_u64()).unwrap()).filter(|id| (!search_fixture::missing_song() || *id != 2) && !vertical_zone_fixture::batch_omit_song(*id, &args)).map(song).collect::<Vec<_>>()});
                    if intelligence_start_fixture::enabled() {
                        response["privileges"]=json!(rows.iter().map(|row|json!({"id":row["id"],"pl":320000,"maxbr":999000,"flag":0})).collect::<Vec<_>>());
                    }
                    response
                }
                "/api/batch" if playlist_editor_fixture::enabled() => {
                    playlist_editor_fixture::wait_while("hold_write").await;
                    return playlist_editor_fixture::batch(args);
                }
                "/api/nos/token/alloc" if playlist_cover_layout_fixture::enabled() => {
                    return playlist_cover_layout_fixture::allocation().await;
                }
                "/api/playlist/catalogue" => {
                    if playlist_editor_fixture::enabled() {
                        return playlist_editor_fixture::catalogue().await;
                    }
                    json!({"code":200,"categories":{"0":"风格"},"sub":[{"name":"轻音乐","category":0},{"name":"摇滚","category":0}]})
                }
                "/api/cloudsearch/pc" => {
                    json!({"code":200,"result":{"songs":(1..=12).map(song).collect::<Vec<_>>(),"songCount":12}})
                }
                "/api/song/enhance/player/url/v1" => {
                    if let Some(expected) = search_fixture::result_play_trial()
                        && args["trialMode"] != expected
                    {
                        return Err(cumulus_core::Error::InvalidInput(
                            "wrong synthetic search playback trial",
                        ));
                    }
                    if let Some(value) = playback_quality_fixture::response(&args) {
                        return value;
                    }
                    let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().unwrap())?;
                    card_source_fixture::record_authorization(&ids);
                    let mut row = json!({"id":ids[0],"url":"https://m1.music.126.net/cumulus-relay-fixture.wav","code":200,"md5":"fixture","level":"standard","expi":120});
                    if trial_seek_proof() && (ids[0] == 1 || ids[0] == "1") {
                        row["freeTrialInfo"] = json!({"start":20,"end":40});
                    }
                    json!({"code":200,"data":[row]})
                }
                "/api/song/lyric/v1" => {
                    let long = std::env::var_os("CUMULUS_PLAYER_LAYOUT_PROOF").is_some();
                    if long && (args["id"] == 3 || args["id"] == "3") {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                    let mut response = json!({"code":200,"lrc":{"lyric":(0..30).map(|i|format!("[00:{i:02}.00]合成歌词 {i} {}\n", if long && i % 3 == 0 {"SyntheticLyricsWithoutWordBreaks".repeat(4)} else {String::new()})).collect::<String>()}});
                    if long && (args["id"] == 1 || args["id"] == "1") {
                        for (key, text) in [("tlyric", "合成翻译"), ("romalrc", "synthetic roman")]
                        {
                            let lyric = (0..30)
                                .map(|i| format!("[00:{i:02}.00]{text} {i}\n"))
                                .collect::<String>();
                            response[key] = json!({"version":1,"lyric":lyric});
                        }
                    }
                    response
                }
                "/api/resource/commentInfo/list"
                    if std::env::var_os("CUMULUS_PLAYER_LAYOUT_PROOF").is_some() =>
                {
                    let (id, comment_count) =
                        match (args["resourceType"].as_u64(), args["resourceIds"].as_str()) {
                            (Some(4), Some("[\"1\"]")) => (1, 1234),
                            (Some(4), Some("[\"2\"]")) => (2, 25),
                            _ => {
                                return Err(cumulus_core::Error::InvalidInput(
                                    "wrong player comment fixture resource",
                                ));
                            }
                        };
                    json!({"code":200,"data":[{"threadId":format!("R_SO_4_{id}"),"commentCount":comment_count}]})
                }
                "/api/song/chorus" if std::env::var_os("CUMULUS_PLAYER_LAYOUT_PROOF").is_some() => {
                    match args["ids"].as_str() {
                        Some("[1]" | "[\"1\"]") => {
                            json!({"code":200,"data":[{"id":1,"startTime":4000}]})
                        }
                        Some("[2]" | "[\"2\"]") => json!({"code":200,"data":[]}),
                        _ => {
                            return Err(cumulus_core::Error::InvalidInput(
                                "wrong chorus fixture ids",
                            ));
                        }
                    }
                }
                _ => {
                    return Err(cumulus_core::Error::Response(
                        "unsupported UX fixture endpoint",
                    ));
                }
            })
        })
    }
}
fn main() {
    // SAFETY: first operation before GTK/runtime or worker creation.
    unsafe { cumulus_desktop::prepare_graphics() };
    queue_more_fixture::phase_milestone("graphics_prepared");
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof root"));
    let motion_proof = std::env::var_os("CUMULUS_MOTION_PROOF").is_some();
    let playback_cache_proof = std::env::var_os("CUMULUS_PLAYBACK_CACHE_PROOF").is_some();
    assert!(!playback_cache_proof || motion_proof);
    assert!(!vertical_zone_fixture::enabled() || motion_proof);
    let search_download_row_proof = search_download_ui_fixture::enabled();
    assert!(
        !search_download_row_proof
            || (motion_proof && !search_fixture::enabled() && !download_page_fixture::enabled())
    );
    let queue_ui_proof = std::env::var_os("CUMULUS_QUEUE_UI_PROOF").is_some();
    let search_footer_ui_proof = std::env::var_os("CUMULUS_SEARCH_FOOTER_UI_PROOF").is_some();
    assert!(!search_footer_ui_proof || (motion_proof && search_fixture::hot_playback_enabled()));
    assert!(!search_signed_fixture::presentation_enabled() || search_footer_ui_proof);
    assert!(!vinyl_similar_fixture::enabled() || (queue_ui_proof && motion_proof));
    let queue_click_proof = std::env::var_os("CUMULUS_QUEUE_CLICK_PROOF").is_some();
    assert!(!queue_click_proof || (queue_ui_proof && motion_proof));
    assert!(
        !queue_more_fixture::enabled() || (queue_ui_proof && motion_proof),
        "queue recommendations require the isolated native player and queue capture"
    );
    assert!(!message_fixture::enabled() || motion_proof);
    assert!(!playback_power_guide_fixture::enabled() || motion_proof);
    assert!(!queue_more_fixture::tip_enabled() || queue_more_fixture::enabled());
    let peq_proof = effect_peq_fixture::enabled();
    let ordinary_source_bypass_proof = effect_peq_fixture::source_bypass();
    assert!(!ordinary_source_bypass_proof || peq_proof);
    let device_peq_proof = std::env::var_os("CUMULUS_EFFECT_DEVICE_NATIVE_PROOF").is_some();
    assert!(!device_peq_proof || (!peq_proof && effect_directory_fixture::enabled()));
    let tone_geometry_proof = tone_geometry::enabled();
    assert!(!tone_geometry_proof || (motion_proof && !peq_proof && !device_peq_proof));
    let device_source_bypass_proof = effect_directory_fixture::source_bypass_enabled();
    assert!(!device_source_bypass_proof || device_peq_proof);
    assert!(
        !(peq_proof || device_peq_proof) || motion_proof,
        "native PEQ proof needs the closed audio fixture"
    );
    let recovery_proof = std::env::var_os("CUMULUS_LIKE_RECOVERY_PROOF").is_some();
    let media_proof = std::env::var_os("CUMULUS_SYSTEM_MEDIA_PROOF").is_some();
    let shortcut_proof = std::env::var_os("CUMULUS_SHORTCUT_UI_PROOF").is_some();
    let card_source_proof = card_source_fixture::enabled();
    let playback_wait_proof = std::env::var_os("CUMULUS_PLAYER_WAIT_PROOF").is_some();
    assert!(
        !playback_wait_proof
            || (motion_proof && std::env::var_os("CUMULUS_PLAYER_LAYOUT_PROOF").is_some()),
        "playback wait proof requires isolated layout/audio fixtures"
    );
    let native_stop_proof = card_source_fixture::native_stop_enabled();
    assert!(
        !native_stop_proof || motion_proof,
        "native stop requires closed audio"
    );
    let search_voice_pending_proof = card_source_fixture::search_voice_pending_enabled();
    assert!(!search_voice_pending_proof || (motion_proof && search_fixture::enabled()));
    assert!(
        !search_privilege_fixture::enabled() || (motion_proof && search_fixture::v3_enabled()),
        "membership refresh proof requires the sealed native V3 search fixture"
    );
    let audio_status_proof =
        card_source_proof || playback_wait_proof || native_stop_proof || search_voice_pending_proof;
    assert!(
        !card_source_proof
            || (motion_proof && library_fixture::enabled() && search_fixture::enabled()),
        "card source proof requires isolated library/search/audio fixtures"
    );
    assert!(
        !member_lifecycle_fixture::enabled() || search_privilege_fixture::enabled(),
        "membership lifecycle proof requires the sealed search fixture"
    );
    let output = root.join(
        if motion_proof && std::env::var_os("CUMULUS_MOTION_NATIVE_REDUCED").is_some() {
            "native-reduced"
        } else if std::env::var_os("CUMULUS_PROOF_REOPEN").is_some() {
            "reopen"
        } else {
            "first"
        },
    );
    fs::create_dir_all(&root).unwrap();
    fs::create_dir(&output).expect("new evidence directory");
    let mut context = assets::context();
    queue_more_fixture::phase_milestone("context_loaded");
    if device_source_bypass_proof || ordinary_source_bypass_proof {
        effect_directory_fixture::restrict_source_bypass_artwork(&mut context);
    }
    if zone_fm_preview_fixture::enabled()
        || vertical_zone_fixture::playlist_v6()
        || vertical_zone_fixture::playlist_batch()
        || vertical_zone_fixture::playlist_queue_menu()
        || search_privilege_fixture::enabled()
        || search_download_row_proof
        || vinyl_similar_fixture::enabled()
    {
        zone_fm_preview_fixture::restrict_artwork(&mut context);
    }
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        queue_more_fixture::phase_milestone("setup_enter");
        #[cfg(target_os = "linux")]
        if tone_geometry_proof || search_download_row_proof || ordinary_source_bypass_proof || search_fixture::hot_playback_enabled() || search_artist_top50_fixture::enabled() || style_home_fixture::enabled() || queue_more_fixture::enabled() || system_master_volume_fixture::running() || vertical_zone_fixture::enabled() || vinyl_comments_fixture::group().is_some() || vinyl_similar_fixture::enabled() {
            let numeric=startup_locale(libc::LC_NUMERIC);
            assert_eq!(numeric,"C");
            fs::write(output.join("locale.json"),serde_json::to_vec(&json!({
                "numeric":numeric,"messages":startup_locale(libc::LC_MESSAGES),
                "characters":startup_locale(libc::LC_CTYPE)
            }))?)?;
        }

        let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
        queue_more_fixture::phase_milestone("registry_opened");
        let proof_store=registry.store();
        search_fixture::initialize(&proof_store)?;
        let older_verified=vinyl_comments_fixture::group().is_some() || vinyl_similar_fixture::enabled() || search_artist_top50_fixture::enabled() || vertical_zone_fixture::enabled();
        let signed_search=(search_fixture::enabled() && !older_verified).then(||search_signed_fixture::SignedSearch::open(proof_store.clone())).transpose()?;
        let library=Arc::new(Library { signed_search:signed_search.clone() });
        let recovery = recovery_proof.then(||Arc::new(like_recovery_fixture::Library::new(library.clone())));
        let api:Arc<dyn Api>=recovery.as_ref().map(|f|f.clone() as Arc<dyn Api>).unwrap_or_else(||library.clone());
        let transfer=playlist_cover_transfer_fixture::enabled().then(||Arc::new(playlist_cover_transfer_fixture::Fixture::new(api.clone(),proof_store.clone(),root.clone())));
        let api=transfer.as_ref().map(|f|f.clone() as Arc<dyn Api>).unwrap_or(api);
        let session=Arc::new(AccountSession::new(api,Some(registry.store())));
        let download_page=download_page_fixture::enabled().then(||Arc::new(download_page_fixture::Fixture::new(session.clone(),proof_store.clone(),root.clone())));
        let fixture_session=session.clone();
        let mut client=MusicClient::with_session(session,registry);
        if playback_power_guide_fixture::enabled() {
            playback_power_guide_fixture::initialize(&proof_store)?;
            if playback_power_guide_fixture::power_host_available() {
                client=client.with_playback_power_sink(Arc::new(playback_power_guide_fixture::Sink));
            }
        }
        if queue_more_fixture::enabled() { client=client.with_queue_recommendations(); }
        let external_import = external_import_fixture::enabled()
            .then(|| Arc::new(external_import_fixture::Wire::new(proof_store.clone())));
        if let Some(wire) = external_import.as_ref() {
            client = client.with_login_providers(Some(wire.clone()), None)
                .with_external_import_backend(wire.clone());
        }
        if external_import_fixture::sidebar_mode().is_some() {
            client=client.with_official_external_import_sidebar()
                .with_login_providers(Some(Arc::new(external_import_fixture::OfflineLogin)),None);
        }
        if remote_skin_fixture::enabled() {
            client = client.with_remote_skin_images(Arc::new(remote_skin_fixture::Media))
                .with_login_providers(Some(Arc::new(external_import_fixture::OfflineLogin)),None);
        }
        if member_welfare_fixture::enabled() {
            client=client.with_official_member_welfare()
                .with_login_providers(Some(Arc::new(external_import_fixture::OfflineLogin)),None);
        }
        if effect_directory_fixture::enabled() {
            client = client.with_effect_material_backend(Arc::new(effect_peq_fixture::Material));
        }
        if daily_history_fixture::enabled() { client=client.with_official_daily_experiment(); }
        if search_fixture::v3_enabled() || search_fixture::carousel_enabled() || vertical_zone_fixture::hotspot() { client=client.with_official_search_experiment(); }
        if search_mv_fixture::fallback_enabled() || search_privilege_fixture::enabled() {
            client=client.with_native_search_preview_config(search_fixture::fallback_config(proof_store.clone())?);
        }
        if search_mv_fixture::enabled() {
            let path=search_mv_fixture::create(&root)?;
            client=client.with_video_factory(Arc::new(move || Ok(Box::new(search_mv_fixture::NativeVideo::open(path.clone())?))));
        }
        if download_page.is_some() { client=client.with_download_backend(Arc::new(download_page_fixture::NoNetwork))
            .with_video_factory(Arc::new(|| Ok(Box::new(cumulus_core::playback::engine::MpvVideoEngine::open(cumulus_core::playback::engine::MpvConfig {null_output:true,..Default::default()},640,360)?)))); }
        if voice_download_fixture::enabled() {
            client=client.with_download_backend(Arc::new(voice_download_fixture::Media));
        }
        if search_download_row_proof {
            client=client.with_download_backend(Arc::new(search_download_ui_fixture::Media));
        }
        if let Some(fixture)=transfer.as_ref() { client=client.with_playlist_cover_backend(fixture.clone()); }
        else if playlist_cover_layout_fixture::enabled() {
            client=client.with_playlist_cover_backend(Arc::new(playlist_cover_layout_fixture::NoNetwork));
        }
        let startup_delay: u64 = std::env::var("CUMULUS_PLAYER_START_DELAY_MS").ok()
            .map(|value| value.parse().expect("integer fixture startup delay")).unwrap_or(0);
        assert!(startup_delay <= 5000, "bounded synthetic startup delay");
        let factory_elapsed = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let factory_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let card_audio = Arc::new(card_source_fixture::Control::default());
        if tone_geometry_proof {
            let path = PathBuf::from(std::env::var_os("CUMULUS_AUDIO_FILTER")
                .expect("trusted instrumented fixture EQ adapter")).canonicalize()?;
            let paths = tone_geometry::prepare(&root, &path)?;
            client = client.with_audio_factory(move || Ok(Box::new(tone_geometry::Audio::open(paths.clone(), path.clone())?)));
        } else if peq_proof || device_peq_proof {
            let path = PathBuf::from(std::env::var_os("CUMULUS_AUDIO_FILTER")
                .expect("trusted synthetic PEQ adapter"));
            let wave = relay_fixture::wave(&root)?;
            let directory_graphic_eq_unavailable = device_peq_proof && !device_source_bypass_proof;
            client = client
                .with_effect_material_backend(Arc::new(effect_peq_fixture::Material))
                .with_audio_factory(move || Ok(Box::new(effect_peq_fixture::Audio::open(
                    wave.clone(), path.clone(), directory_graphic_eq_unavailable,
                )?)));
        } else if motion_proof {
            let elapsed = factory_elapsed.clone();
            let count=factory_count.clone();
            let card_control=card_audio.clone();
            let wave = root.join("relay-synthetic.wav");
            let wave = if wave.exists() { wave } else if queue_more_fixture::tip_enabled() { relay_fixture::wave_seconds(&root,45)? } else { relay_fixture::wave(&root)? };
            let download_directory = if search_download_row_proof { Some(root.join("search-download-media")) }
                else { download_page_fixture::enabled().then(||root.join("media")) };
            client = client.with_audio_factory(move || {
                let start = std::time::Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(startup_delay));
                queue_more_fixture::phase_milestone("audio_factory_enter");
                let mut audio = relay_fixture::NativeAudio::open(wave.clone())?;
                queue_more_fixture::phase_milestone("audio_factory_returned");
                if let Some(directory)=download_directory.as_ref() { audio=audio.allow_download_directory(directory.clone()); }
                elapsed.store(start.elapsed().as_millis() as u64, std::sync::atomic::Ordering::Release);
                count.fetch_add(1,std::sync::atomic::Ordering::AcqRel);
                let audio:Box<dyn cumulus_core::playback::engine::AudioEngine>=if search_download_row_proof {Box::new(search_download_ui_fixture::Audio::new(audio))}else if audio_status_proof || queue_more_fixture::enabled() {Box::new(card_source_fixture::Audio::new(audio,card_control.clone()))}else{Box::new(audio)};
                Ok(audio)
            });
        }
        if playback_cache_proof {
            client = cumulus_desktop::configure_streaming_cache(client);
            assert_eq!(client.streaming_media_cache_status()?.is_none(),
                std::env::var_os("CUMULUS_PLAYBACK_CACHE_BLOCKED").is_some());
        }
        app.manage(cumulus_desktop::effect_activation::Host::new(&app.path().resource_dir()?));
        system_master_volume_fixture::install(app);
        app.manage(if motion_proof { client } else { cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true) });
        queue_more_fixture::phase_milestone("client_managed");
        if search_signed_fixture::presentation_enabled() {
            cumulus_desktop::pc_presentation::start(app.handle())?;
        }
        if member_lifecycle_fixture::enabled() || download_page_fixture::quality_enabled() {
            let client = app.state::<MusicClient>();
            let owner = tauri::async_runtime::block_on(client.start_membership_lifecycle())?;
            app.manage(std::sync::Mutex::new(owner));
        }
        if message_fixture::enabled() {
            cumulus_desktop::start_message_count_polling(app.handle())?;
        }
        app.add_capability(r#"{"identifier":"ux-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        let cover_root=root.clone();
        app.listen("ux-proof",move |event| {
            let mut value:Value=serde_json::from_str(event.payload()).unwrap();
            if playback_power_guide_fixture::enabled() {
                value["playbackPowerGuideFixture"]=playback_power_guide_fixture::report(&proof_store).unwrap();
            }
            if system_master_volume_fixture::enabled() {
                if value["stage"] == "system-master-volume-fixture" {
                    let mode = value["mode"].as_str().unwrap();
                    system_master_volume_fixture::set(mode);
                    handle.get_webview_window("main").unwrap().eval(format!(
                        "window.__systemMasterVolumeFixtureAck={};", json!(mode)
                    )).unwrap();
                    return;
                }
                value["systemMasterVolumeReads"] = json!(system_master_volume_fixture::reads());
            }
            if let Some(wire) = external_import.as_ref() { value["externalImport"] = wire.report(); }
            if external_import_fixture::sidebar_mode().is_some() { value["externalImportSidebarReads"] = json!(external_import_fixture::sidebar_reads()); }
            if remote_skin_fixture::enabled() {
                value["remoteSkinFixture"] = remote_skin_fixture::report();
            }
            if member_welfare_fixture::enabled() {
                value["memberWelfareFixture"] = member_welfare_fixture::report();
                if value["stage"]=="member-welfare-fixture" {
                    use tauri::Emitter;
                    let id=value["id"].as_u64().unwrap();
                    let action=value["action"].as_str().unwrap();
                    member_welfare_fixture::action(action);
                    let switch=action=="switch";
                    let handle=handle.clone(); let session=fixture_session.clone();
                    tauri::async_runtime::spawn(async move {
                        if switch {
                            session.refresh().await.unwrap();
                            handle.emit_to("main","cumulus:account-changed",handle.state::<MusicClient>().account().unwrap()).unwrap();
                        }
                        let report=member_welfare_fixture::report();
                        handle.get_webview_window("main").unwrap().eval(format!("window.__welfareAck[{id}]={report};")).unwrap();
                    });
                    return;
                }
            }
            if queue_more_fixture::tip_enabled() {
                if value["stage"]=="queue-more-tip-milestone" {
                    queue_more_fixture::renderer_milestone(value["step"].as_str().unwrap_or_default());
                    return;
                }
                let main=handle.get_webview_window("main").unwrap();
                if value["stage"]=="queue-more-tip-focus" {
                    let sequence=value["sequence"].as_u64().unwrap();
                    queue_more_fixture::request_tip_focus(&handle, value["focus"]==true, sequence);
                    return;
                }
                let owner=fixture_session.token().unwrap().uid.unwrap_or_default();
                value["syntheticQueueTip"]=json!({
                    "nativeFocused":cumulus_desktop::queue_tip_window_foreground(&main).unwrap(),"nativeVisible":main.is_visible().unwrap(),
                    "seen":proof_store.get(&owner,"queue-more-v1","tip-seen").unwrap()==Some(json!(true))
                });
            }
            if queue_more_fixture::enabled() {
                value["syntheticQueueMore"]=queue_more_fixture::report();
                if value["stage"]=="queue-more-fixture" {
                    let sequence=value["sequence"].as_u64().unwrap();
                    let mode=value["mode"].as_str().unwrap();
                    let report=queue_more_fixture::control(mode).unwrap();
                    handle.get_webview_window("main").unwrap().eval(format!("window.__queueMoreFixtureAck[{sequence}]={report};")).unwrap();
                    return;
                }
            }
            if intelligence_start_fixture::enabled() {value["syntheticIntelligenceStart"]=intelligence_start_fixture::report();}
            if effect_directory_fixture::enabled() {
                value["effectDirectoryCalls"]=json!(effect_directory_fixture::calls());
                value["effectDirectoryMaterialGets"]=effect_peq_fixture::report()["materialGets"].clone();
            }
            if peq_proof || device_peq_proof {value["nativePeq"]=effect_peq_fixture::report();}
            if device_source_bypass_proof {
                value["headphoneSourceCalls"]=json!(effect_directory_fixture::source_calls());
            }
            if latest_releases_fixture::enabled() {value["latestReleasesCalls"]=latest_releases_fixture::report();}
            if daily_history_fixture::enabled() {value["dailyHistoryCalls"]=json!(daily_history_fixture::calls());}
            if member_sign_fixture::enabled() {
                value["memberSignCalls"]=json!(member_sign_fixture::calls());
                value["memberOfferReads"]=json!(member_sign_fixture::offer_reads());
                value["memberSignSongLikeWrites"]=json!(member_sign_fixture::sign_song_like_writes());
            }
            if style_home_fixture::enabled() {value["styleHomeCalls"]=json!(style_home_fixture::calls());}
            if vinyl_comments_fixture::group().is_some() {value["vinylCommentsCalls"]=json!(vinyl_comments_fixture::report());}
            if vinyl_similar_fixture::enabled() {value["vinylSimilarFixture"]=vinyl_similar_fixture::report();}
            if vertical_zone_fixture::enabled() {value["verticalZoneCalls"]=json!(vertical_zone_fixture::calls());}
            if zone_fm_preview_fixture::enabled() {value["zoneFmPreview"]=zone_fm_preview_fixture::report();}
            if style_recommendations_fixture::enabled() {value["styleRecommendationCalls"]=json!(style_recommendations_fixture::calls());}
            if search_fixture::enabled() {
                value["syntheticSearchCalls"] = json!(search_fixture::calls());
                if let Some(calls) = search_fixture::suggestion_match_calls() { value["suggestionMatchCalls"] = json!(calls); }
                if search_artist_top50_fixture::enabled() { value["artistTop50Calls"] = json!(search_artist_top50_fixture::report()); }
                if let Some(calls) = search_fixture::audio_toplist_calls() { value["audioToplistCalls"] = json!(calls); }
                if search_fixture::v3_enabled() { value["syntheticV3Calls"] = json!(search_fixture::v3_calls()); }
                if search_signed_fixture::presentation_enabled() { value["presentationQueryCalls"] = json!(search_fixture::presentation_query_calls()); }
                if search_fixture::hot_playback_enabled() { value["hotPlaybackCalls"] = json!(search_fixture::hot_playback_calls()); }
                if search_fixture::carousel_enabled() { value["searchCarouselCalls"] = json!(search_fixture::carousel_calls()); }
                value["searchProfileCalls"] = json!(*SEARCH_PROFILE_CALLS.lock().unwrap());
                value["audioRankDetails"] = json!(search_fixture::audio_rank_details());
                if let Some(calls) = search_fixture::voice_queue_calls() { value["voiceQueueCalls"] = json!(calls); }
                if let Some(calls) = search_fixture::voice_next_calls() { value["voiceNextCalls"] = json!(calls); }
                if let Some(calls) = search_fixture::podcast_follow_calls() { value["podcastFollowCalls"] = json!(calls); }
                if let Some(calls) = search_fixture::artist_gallery_calls() { value["artistGalleryCalls"] = json!(calls); }
            }
            if search_mv_fixture::enabled() { value["searchMvCalls"]=json!(search_mv_fixture::calls()); }
            if search_mv_fixture::enabled() && !search_mv_fixture::fallback_enabled() {
                value["searchMvPrivilegeCalls"]=json!(search_fixture::mv_preview_privilege_calls());
            }
            if search_privilege_fixture::enabled() { value["searchPrivilegeFixture"]=search_privilege_fixture::report(); }
            if member_lifecycle_fixture::enabled() { value["membershipLifecycleFixture"]=member_lifecycle_fixture::report(); }
            if subscribers_proof() { value["subscriberOffsets"]=json!(*SUBSCRIBER_OFFSETS.lock().unwrap()); }
            if playlist_tag_browse_proof() {
                value["playlistTagCalls"] = json!(*PLAYLIST_TAG_CALLS.lock().unwrap());
                value["playlistCatalogueCalls"] = json!(*PLAYLIST_CATALOGUE_CALLS.lock().unwrap());
                value["playlistSquare"] = playlist_square_fixture::report();
                value["playlistTopTagCalls"] = json!(playlist_square_fixture::top_calls());
                value["playlistEssential"] = playlist_essential_fixture::report();
            }
            if profile_play_record_fixture::enabled() {
                if profile_play_record_fixture::removal_enabled() {
                    if value["stage"] == "profile-rank-release-delete" {
                        profile_play_record_fixture::release_remove();
                        return;
                    }
                    if value["stage"] == "profile-rank-displace-row" {
                        profile_play_record_fixture::remove_all_row_externally();
                        handle.get_webview_window("main").unwrap().eval("window.__profileRankDisplaced=true;").unwrap();
                        return;
                    }
                }
                if value["stage"] == "profile-rank-revoke" {
                    profile_play_record_fixture::revoke_guest();
                    handle.get_webview_window("main").unwrap().eval("window.__profileRankRevoked=true;").unwrap();
                    return;
                }
                value["profileRankCalls"] = json!(profile_play_record_fixture::report());
                if profile_play_record_fixture::play_card_enabled() {
                    value["profileRankPlaybackCalls"] = json!(profile_play_record_fixture::playback_calls());
                }
            }
            if profile_relations_fixture::enabled() {
                value["profileRelationCalls"] = json!(profile_relations_fixture::report());
            }
            if playlist_creator_proof() {
                value["playlistCreatorCalls"] = json!(*PLAYLIST_CREATOR_CALLS.lock().unwrap());
            }
            if playlist_comments_proof() {
                value["playlistCommentThreads"]=json!(*PLAYLIST_COMMENT_THREADS.lock().unwrap());
                value["playlistCommentDetails"]=json!(*PLAYLIST_COMMENT_DETAILS.lock().unwrap());
            }
            if voice_download_fixture::enabled() { value["voiceDownloadService"]=voice_download_fixture::report(); }
            let stage=value["stage"].as_str().unwrap_or("");
            if vinyl_similar_fixture::enabled() && stage=="vinyl-similar-fixture" {
                let id=value["id"].as_u64().unwrap();
                let action=value["action"].as_str().unwrap();
                let result=match vinyl_similar_fixture::control(action) {
                    Ok(reply)=>json!({"id":id,"reply":reply}),
                    Err(error)=>json!({"id":id,"error":error.to_string()}),
                };
                handle.get_webview_window("main").unwrap().eval(format!("window.__vinylSimilarFixtureAck={result};")).unwrap();
                return;
            }
            if member_lifecycle_fixture::enabled() && stage=="member-lifecycle-release" {
                member_lifecycle_fixture::release();
                return;
            }
            if ordinary_source_bypass_proof && stage=="ordinary-peq-source-native" {
                let sequence=value["sequence"].as_u64().unwrap();
                let stats=&value["nativePeq"];
                handle.get_webview_window("main").unwrap().eval(format!("window.__ordinaryPeqNative={{sequence:{sequence},stats:{stats}}};")).unwrap();
                return;
            }
            if device_source_bypass_proof && stage=="effect-source-native-stats" {
                let sequence=value["sequence"].as_u64().unwrap();
                let stats=&value["nativePeq"];
                handle.get_webview_window("main").unwrap().eval(format!("window.__headphoneSourceStats={{sequence:{sequence},stats:{stats}}};")).unwrap();
                return;
            }
            if voice_download_fixture::enabled() && stage=="voice-download-release" {
                voice_download_fixture::release();return;
            }
            if device_peq_proof && stage=="effect-device-comparison-pcm" {
                let stats=&value["nativePeq"];
                let ready=stats["nativeAttached"]==true && stats["attaches"]==3 &&
                    stats["processedSamples"].as_u64().is_some_and(|n|n>0) && stats["failedBlocks"]==0;
                handle.get_webview_window("main").unwrap().eval(format!("window.__headphonePcmReady={ready};")).unwrap();return;
            }
            if stage=="message-fixture" && message_fixture::enabled() {
                let report=message_fixture::control(value["mode"].as_str().unwrap_or(""));
                let id=value["id"].as_u64().expect("message probe sequence");
                handle.get_webview_window("main").unwrap().eval(format!("window.__messageFixtureAck[{id}]={report};")).unwrap();
                return;
            }
            if stage=="message-poll-hide" && message_fixture::polling() {
                let window=handle.get_webview_window("main").unwrap();
                let baseline=message_fixture::count_calls();
                window.hide().unwrap();
                cumulus_desktop::message_count_window_visibility(&handle,true);
                let handle=handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(32)).await;
                    let hidden=message_fixture::count_calls();
                    let window=handle.get_webview_window("main").unwrap();
                    assert!(!window.is_visible().unwrap());
                    window.show().unwrap();
                    cumulus_desktop::message_count_window_visibility(&handle,false);
                    window.eval(format!("window.__messagePollRevealed={{baseline:{baseline},hidden:{hidden}}};")).unwrap();
                });
                return;
            }
            if stage=="message-poll-switch" && message_fixture::polling() {
                use tauri::Emitter;
                message_fixture::control("poll-hold");
                fixture_session.adopt(Arc::new(Library::default()), json!({"userId":3,"nickname":"合成第二账号"}), &fixture_session.token().unwrap()).unwrap();
                handle.emit_to("main","cumulus:account-changed",handle.state::<MusicClient>().account().unwrap()).unwrap();
                return;
            }
            if stage=="effect-peq-fail-runtime" && effect_peq_fixture::inject_runtime_fault() {
                handle.get_webview_window("main").unwrap().eval("window.__peqRuntimeFaultArmed=true;").unwrap();return;
            }
            if stage=="member-sign-reveal" && member_sign_fixture::reveal_write() {
                handle.get_webview_window("main").unwrap().eval("window.__memberSignRevealed=true;").unwrap();return;
            }
            if audio_status_proof && stage=="card-source-fixture" {
                let id=value["id"].as_u64().expect("synthetic card control id");
                let reply=card_audio.configure(value["mode"].as_str().unwrap_or("")).unwrap_or_else(|error|json!({"error":error.to_string()}));
                handle.get_webview_window("main").unwrap().eval(format!("window.__cardSourceAck[{id}]={reply};")).unwrap();return;
            }
            if intelligence_start_fixture::enabled() && stage=="intelligence-start-fixture" {
                let kind=value["kind"].as_str().unwrap();intelligence_start_fixture::control(kind);
                handle.get_webview_window("main").unwrap().eval(format!("window.__intelligenceStartReport={};window.__intelligenceStartAck={};",intelligence_start_fixture::report(),json!(kind))).unwrap();return;
            }
            if intelligence_ui_fixture::enabled() && stage=="intelligence-fixture" {
                let handle=handle.clone();let store=proof_store.clone();let session=fixture_session.clone();let root=cover_root.clone();
                tauri::async_runtime::spawn(async move {
                    let result=intelligence_ui_fixture::control(&handle.state::<MusicClient>(),session,store,&root,value["kind"].as_str().unwrap()).await;
                    let ack=match result {Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"error":error.to_string()})};
                    handle.get_webview_window("main").unwrap().eval(format!("window.__intelligenceFixtureAck={ack};")).unwrap();
                });return;
            }
            if (search_fixture::enabled() || std::env::var_os("CUMULUS_SIDEBAR_UI_PROOF").is_some() || std::env::var_os("CUMULUS_PLAYLIST_MENU_UI_PROOF").is_some()) && ["search-pointer", "sidebar-pointer"].contains(&stage) {
                let window=handle.get_webview_window("main").unwrap();
                let position=tauri::LogicalPosition::new(value["x"].as_f64().unwrap(),value["y"].as_f64().unwrap());
                window.set_cursor_position(position).unwrap();
                window.eval("window.__searchPointerAck=true;").unwrap();return;
            }
            if (search_fixture::enabled() || std::env::var_os("CUMULUS_PLAYLIST_MENU_UI_PROOF").is_some()) && stage=="search-clipboard-check" {
                use tauri_plugin_clipboard_manager::ClipboardExt;
                let handle=handle.clone(); let expected=value["expected"].as_str().unwrap().to_owned();
                tauri::async_runtime::spawn(async move {
                    let matches = handle.clipboard().read_text().is_ok_and(|text|text==expected);
                    handle.get_webview_window("main").unwrap().eval(format!("window.__searchClipboardAck={matches};")).unwrap();
                });return;
            }
            if search_download_row_proof && stage=="search-download-fixture" {
                let handle=handle.clone();let session=fixture_session.clone();let store=proof_store.clone();
                let root=cover_root.clone();let mode=value["mode"].as_str().unwrap_or_default().to_owned();
                tauri::async_runtime::spawn(async move {
                    let result=search_download_ui_fixture::control(&handle.state::<MusicClient>(),&session,&store,&root,&mode).await;
                    let ack=match result {Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"message":error.to_string()})};
                    handle.get_webview_window("main").unwrap().eval(format!("window.__searchDownloadFixtureAck={ack};")).unwrap();
                });return;
            }
            if let Some(fixture)=download_page.as_ref() && stage=="download-page-fixture" {
                let handle=handle.clone();let fixture=fixture.clone();
                tauri::async_runtime::spawn(async move {
                    let result=fixture.control(&handle.state::<MusicClient>(),&value).await;
                    let ack=match result {Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"message":error.to_string()})};
                    handle.get_webview_window("main").unwrap().eval(format!("window.__downloadPageAck={ack};")).unwrap();
                });return;
            }
            if search_fixture::enabled() && stage=="search-fixture" {
                search_fixture::control(&value);
                handle.get_webview_window("main").unwrap().eval("window.__searchFixtureAck=true;").unwrap();return;
            }
            if playlist_tag_browse_proof() && stage=="playlist-square-disable" {
                playlist_square_fixture::disable();
                handle.get_webview_window("main").unwrap().eval("window.__squareDisabledAck=true;").unwrap();return;
            }
            if let Some(fixture)=transfer.as_ref() && stage=="cover-transfer-fixture" {
                let handle=handle.clone();let fixture=fixture.clone();
                tauri::async_runtime::spawn(async move {
                    let result=fixture.control(&handle.state::<MusicClient>(),&value).await;
                    let ack=match result {Ok(())=>json!({"ok":true,"report":fixture.report()}),Err(error)=>json!({"ok":false,"message":error.to_string()})};
                    handle.get_webview_window("main").unwrap().eval(format!("window.__coverTransferAck={ack};")).unwrap();
                });return;
            }
            if playlist_cover_layout_fixture::enabled() && stage=="cover-fixture" {
                let handle=handle.clone();let root=cover_root.clone();let store=proof_store.clone();
                tauri::async_runtime::spawn(async move {
                    let client=handle.state::<MusicClient>();
                    let result=playlist_cover_layout_fixture::control(&client,&store,&root,&value).await;
                    let ack=match result {Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"message":error.to_string()})};
                    handle.get_webview_window("main").unwrap().eval(format!("window.__coverFixtureAck={ack};")).unwrap();
                });
                return;
            }
            if playlist_editor_fixture::enabled() && stage=="editor-fixture" {
                    playlist_editor_fixture::control(&value);
                    handle.get_webview_window("main").unwrap().eval("window.__editorFixtureAck=true;").unwrap();
                    return;
            }
            if playlist_collect_fixture::enabled() && stage=="collect-fixture" {
                playlist_collect_fixture::control(&value);
                let report = playlist_collect_fixture::report();
                handle.get_webview_window("main").unwrap().eval(format!("window.__collectFixtureAck={report};")).unwrap();
                return;
            }
            if library_fixture::enabled() && stage=="library-fixture" {
                library_fixture::control(&value);
                let report = library_fixture::report();
                handle.get_webview_window("main").unwrap().eval(format!("window.__libraryFixtureAck={report};")).unwrap();
                return;
            }
            if playlist_layout_fixture::enabled() && stage=="playlist-fixture-release" {
                playlist_layout_fixture::release_failure();
                return;
            }
            if shortcut_proof && stage=="shortcut-fixture" {
                let epoch=handle.state::<MusicClient>().account().unwrap().epoch;
                media_shortcuts::command(&handle,&proof_store,&epoch,&value).unwrap();
                handle.get_webview_window("main").unwrap().eval(format!("window.__shortcutFixtureAck={};",serde_json::to_string(&value["kind"]).unwrap())).unwrap();
                return;
            }
            if playback_quality_fixture::enabled() && stage=="quality-fixture" {
                playback_quality_fixture::control(&proof_store,&value);
                let report=playback_quality_fixture::report();
                handle.get_webview_window("main").unwrap().eval(format!("window.__qualityFixtureAck={report};")).unwrap();
                return;
            }
            if mode_fixture::enabled() && stage=="mode-fixture" {
                let kind=value["kind"].as_str().unwrap();mode_fixture::control(&proof_store,kind);
                handle.get_webview_window("main").unwrap().eval(format!("window.__modeFixtureAck={};",serde_json::to_string(kind).unwrap())).unwrap();
                return;
            }
            if fm_feedback_fixture::enabled() && stage=="fm-feedback-fixture" {
                let kind=value["kind"].as_str().unwrap();let owner=fixture_session.token().unwrap().uid.unwrap();fm_feedback_fixture::control(&proof_store,&owner,kind);
                handle.get_webview_window("main").unwrap().eval(format!("window.__fmFeedbackFixtureAck={};",serde_json::to_string(kind).unwrap())).unwrap();return;
            }
            if volume_fixture::enabled() && stage=="volume-fixture" {
                let kind=value["kind"].as_str().unwrap();
                volume_fixture::apply(&proof_store,kind);
                handle.get_webview_window("main").unwrap().eval(format!("window.__volumeFixtureAck={};",serde_json::to_string(kind).unwrap())).unwrap();
                return;
            }
            if media_proof && stage=="media-fixture" {
                let kind=value["kind"].as_str().unwrap();
                match kind {
                    "reject" | "allow" => {
                        let sql=rusqlite::Connection::open(proof_store.directory().join("core.sqlite3")).unwrap();
                        sql.execute_batch(if kind=="reject" {"CREATE TRIGGER reject_media_ui BEFORE INSERT ON state WHEN NEW.kind='system-media-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic setting write failure'); END;"} else {"DROP TRIGGER reject_media_ui;"}).unwrap();
                    }
                    "damage"=>proof_store.put("local-device","system-media-preferences-v1","current",&json!({"enabled":"invalid"})).unwrap(),
                    "raw_damage"=>{
                        let sql=rusqlite::Connection::open(proof_store.directory().join("core.sqlite3")).unwrap();
                        assert_eq!(sql.execute("UPDATE state SET value=? WHERE owner='local-device' AND kind='system-media-preferences-v1' AND key='current'",["{broken"]).unwrap(),1);
                    }
                    _=>panic!("unknown isolated media fixture action"),
                }
                handle.get_webview_window("main").unwrap().eval(format!("window.__mediaFixtureAck={};",serde_json::to_string(kind).unwrap())).unwrap();
                return;
            }
            if let Some(fixture)=recovery.as_ref() {
                use std::sync::atomic::Ordering;
                if stage=="like-fixture" {
                    if let Some(healthy)=value["healthy"].as_bool(){fixture.healthy.store(healthy,Ordering::SeqCst);}
                    if let Some(remote)=value["remote"].as_bool(){fixture.remote(value["id"].as_str().unwrap(),remote);}
                    if value["failRead"]==true{fixture.fail_read.store(true,Ordering::SeqCst);}
                    return;
                }
            }
            if stage=="compact-zoom" && download_page_fixture::enabled() {
                let scale=match value["enabled"].as_bool(){Some(true)=>1.25,Some(false)=>1.0,None=>return};
                handle.get_webview_window("main").unwrap().set_zoom(scale).unwrap();return;
            }
            if stage=="focus-sample" && download_page_fixture::enabled() {
                handle.get_webview_window("main").unwrap().set_focus().unwrap();return;
            }
            if stage=="resize" {
                let window=handle.get_webview_window("main").unwrap();let width=value["width"].as_f64().unwrap();let height=value["height"].as_f64().unwrap();
                if let Some(scale)=value["scale"].as_f64() {assert!(scale.is_finite() && scale>0.0 && scale<=4.0);window.set_size(tauri::PhysicalSize::new((width*scale).round() as u32,(height*scale).round() as u32)).unwrap();}
                else{window.set_size(tauri::LogicalSize::new(width,height)).unwrap();}return;
            }
            if stage=="pc-presentation-fixture" && search_signed_fixture::presentation_enabled() {
                let signed=signed_search.as_ref().expect("sealed signed presentation client").clone();
                let nonce=value["nonce"].as_u64().unwrap();
                let mode=value["mode"].as_str().unwrap();
                let before=signed.full_reads();
                signed.set_server_mode(mode).unwrap();
                let handle=handle.clone();
                tauri::async_runtime::spawn(async move {
                    let result=handle.state::<MusicClient>().refresh_pc_presentation().await;
                    let result=result.map_err(cumulus_core::client::ClientError::from);
                    let ack=json!({"nonce":nonce,"beforeFull":before,"afterFull":signed.full_reads(),
                        "result":result});
                    handle.get_webview_window("main").unwrap().eval(format!("window.__pcPresentationFixtureAck={ack};")).unwrap();
                });
                return;
            }
            if !(["complete","failed"].contains(&stage) || stage.starts_with("ux_") && stage.chars().all(|c|c.is_ascii_alphanumeric()||c=='_')){return;}
            let stage=stage.to_owned();
            if search_download_row_proof { value["searchDownloadFixture"]=search_download_ui_fixture::report(); }
            if download_page_fixture::quality_enabled() {
                value["downloadQualityFixture"] = download_page_fixture::quality_report(&proof_store).unwrap();
            }
            if let Some(signed)=signed_search.as_ref() { value["signedSearch"] = signed.report(); }
            if message_fixture::enabled() { value["messageRequests"] = message_fixture::report(); }
            if playback_quality_fixture::enabled() { value["syntheticQuality"] = playback_quality_fixture::report(); }
            if mode_fixture::enabled() { value["syntheticMode"] = mode_fixture::report(); }
            if library_fixture::enabled() { value["syntheticLibrary"] = library_fixture::report(); }
            if playlist_collect_fixture::enabled() { value["syntheticCollection"] = playlist_collect_fixture::report(); }
            if let Some(fixture)=transfer.as_ref() {value["syntheticCoverTransfer"]=fixture.report();}
            if playlist_editor_fixture::enabled() {value["syntheticMetadataWrites"]=json!(playlist_editor_fixture::writes());}
            if let Some(fixture)=recovery.as_ref() {
                use std::sync::atomic::Ordering;
                value["syntheticLikeWrites"]=json!(fixture.writes.load(Ordering::SeqCst));
                value["syntheticLikeReads"]=json!(fixture.reads.load(Ordering::SeqCst));
            }
            if motion_proof {
                if audio_status_proof || queue_more_fixture::enabled() {value["sourceAudio"]=card_audio.report();}
                value["audioFactoryElapsedMs"] = json!(factory_elapsed.load(std::sync::atomic::Ordering::Acquire));
                value["audioFactoryDelayRequestedMs"] = json!(startup_delay);
                value["audioFactoryCount"] = json!(factory_count.load(std::sync::atomic::Ordering::Acquire));
                if media_proof {
                    value["systemMediaStatus"]=serde_json::to_value(cumulus_desktop::system_media::settings::system_media_status(handle.clone())).unwrap();
                    value["systemMediaPreferences"]=serde_json::to_value(handle.state::<MusicClient>().system_media_preferences_native().unwrap()).unwrap();
                }
                if shortcut_proof {media_shortcuts::observe(&handle,&mut value);}
                let stage = stage.to_owned(); let handle=handle.clone(); let output=output.clone();
                let rank_source_store=proof_store.clone();let rank_source_session=fixture_session.clone();
                tauri::async_runtime::spawn(async move {
                    let client=handle.state::<MusicClient>(); let epoch=client.account().unwrap().epoch;
                    if fm_feedback_fixture::enabled() {
                        match client.fm_view(&epoch,cumulus_core::fm::FmSource::Personal,false).await {
                            Ok(view)=>value["nativeFm"]=serde_json::to_value(view.value).unwrap(),
                            Err(error)=>value["nativeFmFailure"]=serde_json::to_value(cumulus_core::client::ClientError::from(error)).unwrap(),
                        }
                        value["syntheticFmFeedback"]=fm_feedback_fixture::report();
                    }
                    if queue_click_proof && value["deferQueueClickObservation"]==true && stage=="ux_06_catalog_added" {
                        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                    }
                    // Queue proof stages await this actor barrier before the
                    // script releases an intentionally held clear/edit. A watch
                    // snapshot may still describe the previous queue revision.
                    let player = if queue_ui_proof || queue_click_proof || search_footer_ui_proof || search_artist_top50_fixture::enabled() {
                        client.player_snapshot(&epoch).await
                    } else {
                        client.subscribe_playback(&epoch).await.and_then(|playback|playback.current())
                    };
                    if let Ok(snapshot)=player {
                        value["nativePlayer"]=serde_json::to_value(snapshot).unwrap();
                        if std::env::var_os("CUMULUS_PLAYLIST_QUEUE_UI_PROOF").is_some() || queue_ui_proof || search_footer_ui_proof || search_download_row_proof || download_page_fixture::enabled() || card_source_proof || profile_play_record_fixture::enabled() || search_artist_top50_fixture::enabled() {
                            value["nativeQueue"] = serde_json::to_value(client.player_queue(&epoch).await.unwrap()).unwrap();
                        }
                        if profile_play_record_fixture::play_card_enabled() {
                            let owner=rank_source_session.token().unwrap().uid.unwrap();
                            if let Some(queue)=rank_source_store.get(&owner,"playback","queue").unwrap() {
                                value["nativeRankSources"]=json!(queue["items"].as_array().unwrap().iter().map(|item| &item["source"]).collect::<Vec<_>>());
                            }
                        }
                    } else { value["nativePlayerUnavailable"]=json!(true); }
                    if tone_geometry_proof { value["toneGeometry"]=tone_geometry::report(); }
                    fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                    if queue_more_fixture::tip_enabled() { queue_more_fixture::capture_milestone(&stage); }
                    if !capture_ack::acknowledge(&handle,&output,&stage,value["captureHandshake"]==true).await {return;}
                    if tone_geometry_proof && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__toneGeometryRecorded={};",json!(stage))).unwrap();
                    }
                    if ordinary_source_bypass_proof && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__ordinaryPeqStageRecorded={};",json!(stage))).unwrap();
                    }
                    if device_source_bypass_proof && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__headphoneSourceStageRecorded={};",json!(stage))).unwrap();
                    }
                    if vertical_zone_fixture::playlist_stage_ack() && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__zonePlaylistStageRecorded={};",json!(stage))).unwrap();
                    }
                    // An emit reply does not wait for this spawned snapshot.
                    // Do not let Stop on completion overtake the JSON sample.
                    if (system_master_volume_fixture::running() || playback_power_guide_fixture::enabled())
                        && stage.starts_with("ux_")
                        && !handle.get_webview_window("main").is_some_and(|window| {
                            window.eval(format!("window.__systemProbeStageRecorded={};", json!(stage))).is_ok()
                        })
                    {
                        let failure = json!({"ok":false,"stage":stage,"error":"native system probe acknowledgement failed"});
                        let _=fs::write(output.join("failed.json"),serde_json::to_vec_pretty(&failure).unwrap());
                        handle.exit(1);
                        return;
                    }
                    if queue_ui_proof && stage.starts_with("ux_")
                        && !handle.get_webview_window("main").is_some_and(|window| {
                            window.eval(format!("window.__queueUiStageRecorded={};",json!(stage))).is_ok()
                        })
                    {
                        let failure=json!({"ok":false,"stage":stage,"error":"native queue stage acknowledgement failed"});
                        let _=fs::write(output.join("failed.json"),serde_json::to_vec_pretty(&failure).unwrap());
                        handle.exit(1);
                        return;
                    }
                    if queue_click_proof && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__queueClickRecorded={};",serde_json::json!(stage))).unwrap();
                    }
                    if (vinyl_comments_fixture::composer() || vinyl_comments_fixture::likes() || vinyl_comments_fixture::reports()) && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__vinylComposerRecorded={};",json!(stage))).unwrap();
                    }
                    if vinyl_similar_fixture::enabled() && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__vinylSimilarRecorded={};",json!(stage))).unwrap();
                    }
                    if search_artist_top50_fixture::enabled() && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__searchArtistTop50Recorded={};",serde_json::json!(stage))).unwrap();
                    }
                    if search_download_row_proof && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__searchDownloadStageRecorded={};",json!(stage))).unwrap();
                    }
                    if search_footer_ui_proof && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__searchFooterRecorded={};",json!(stage))).unwrap();
                    }
                    if (std::env::var_os("CUMULUS_QUEUE_SELECTION_UI_PROOF").is_some() || intelligence_ui_fixture::enabled() || intelligence_start_fixture::enabled() || fm_feedback_fixture::enabled() || queue_more_fixture::enabled()) && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__queueSelectionRecorded={};",serde_json::json!(stage))).unwrap();
                    }
                    if ["complete","failed"].contains(&stage.as_str()) {
                        if stage=="complete" { queue_more_fixture::phase_milestone("complete_written"); }
                        queue_more_fixture::phase_milestone("shutdown_stop_enter");
                        let _=client.player_action(&epoch,cumulus_core::client::dto::PlayerAction::Stop).await;
                        queue_more_fixture::phase_milestone("shutdown_stop_returned");
                        handle.exit(if stage=="complete" {0} else {1});
                    }
                });
                return;
            }
            fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref()==Ok("1") && stage.starts_with("ux_") {
                let handle=handle.clone();let output=output.clone();
                tauri::async_runtime::spawn(async move {capture_ack::acknowledge(&handle,&output,&stage,value["captureHandshake"]==true).await;});
                return;
            }
            if ["complete","failed"].contains(&stage.as_str()) {handle.exit(if stage=="complete" {0} else {1});}
        });
        queue_more_fixture::phase_milestone("proof_listener_installed");
        if queue_more_fixture::tip_enabled() {
            // Focus tests only need a real OS window. Build it before the only
            // WebView, so WebView2 cannot pump main-page IPC during a second
            // controller creation on the UI thread.
            queue_more_fixture::phase_milestone("focus_helper_build_enter");
            tauri::WindowBuilder::new(app,"queue-tip-focus-helper")
                .title("Synthetic native focus control").visible(false).focused(false)
                .position(1150.0,32.0).inner_size(180.0,100.0).build()?;
            queue_more_fixture::phase_milestone("focus_helper_built");
        }
        // Absolute config dataDirectory values are ignored by Tauri's safe-path
        // resolver. The builder API owns this isolated test directory explicitly.
        let mut builder=tauri::WebviewWindowBuilder::from_config(app, &window_config)?.data_directory(root.join("webview"));
        if motion_proof { builder=builder.initialization_script(include_str!("motion_environment.js")); }
        queue_more_fixture::phase_milestone("main_window_build_enter");
        builder.build()?;
        queue_more_fixture::phase_milestone("main_window_built");
        if media_proof {cumulus_desktop::system_media::install(app.handle());}
        playback_power_guide_fixture::install_media(app.handle());
        if shortcut_proof {cumulus_desktop::shortcuts::install(app.handle());}
        Ok(())
    }).on_page_load(|view, payload| {
        if payload.event()==PageLoadEvent::Finished && view.label()=="main" {
            queue_more_fixture::phase_milestone("main_page_loaded");
            let script=std::env::var_os("CUMULUS_UX_SCRIPT").map(|p|fs::read_to_string(p).unwrap());
            let default_script = if std::env::var_os("CUMULUS_LIKE_RECOVERY_PROOF").is_some() { include_str!("like_recovery_ui_probe.js") } else if std::env::var_os("CUMULUS_MOTION_PROOF").is_some() { include_str!("motion_ui_probe.js") } else { include_str!("ux_ui_probe.js") };
            let source=script.as_deref().unwrap_or(default_script);
            if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref()==Ok("1") {view.eval(capture_ack::script(source)).unwrap();}
            else {view.eval(source).unwrap();}
            queue_more_fixture::phase_milestone("script_queued");
        }
    }).run(context).expect("UX probe");
    queue_more_fixture::phase_milestone("run_returned");
}
