use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::json;
use std::sync::Arc;
use tauri::test::{INVOKE_KEY, assert_ipc_response, mock_builder, mock_context, noop_assets};

struct NoNetwork;
impl Api for NoNetwork {
    fn request<'a>(
        &'a self,
        _path: &'a str,
        _payload: serde_json::Value,
        _cache: bool,
    ) -> ApiFuture<'a, serde_json::Value> {
        Box::pin(async { panic!("a snapshot or stale write must never access the network") })
    }
}

#[test]
fn foreign_and_unregistered_mini_windows_cannot_invoke_application_commands() {
    let app = cumulus_desktop::configure(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    for label in ["captcha-1", "mini-player-1"] {
        let window = tauri::WebviewWindowBuilder::new(&app, label, Default::default())
            .build()
            .unwrap();
        // A registered custom scheme is local to Tauri. Window identity must still
        // deny the app dispatcher; local-only capabilities alone are insufficient.
        for cmd in [
            "mini_enter",
            "mini_read",
            "mini_action",
            "mini_intelligence",
            "mini_mode",
            "mini_intelligence_cancel",
            "mini_preferences_command",
            "get_app_info",
            "account_snapshot",
            "login_command",
            "login_presentation",
            "select_login_country",
            "open_login_document",
            "pick_local_directory",
            "download_collect_creation",
            "reorder_playlist_mixed",
            "playlist_mixed_reorder_receipt",
            "reconcile_playlist_mixed_reorder",
            "player_action",
            "player_vinyl_voice_detail_target",
            "player_voice_songs_download_prepare",
            "player_voice_songs_download_prepare_with_destination",
            "player_voice_songs_download_destination",
            "player_voice_songs_download_cancel",
            "player_voice_songs_download_start",
            "player_voice_songs_download_confirm_quality",
            "open_voice_song_group_purchased_quality_guide",
            "change_fresh_vinyl_podcast_subscription",
            "reconcile_fresh_vinyl_podcast_subscription",
            "player_voice_songs_collect_liked_destination",
            "player_voice_songs_collect_liked",
            "solve_login_captcha",
            "catalog_page",
            "collection_page",
            "history_page",
            "history_command",
            "history_sync_preference",
            "remote_skin_catalog",
            "remote_skin_image",
            "account_settings_page",
            "account_settings_command",
            "relay_view",
            "relay_command",
            "relay_setting_command",
            "catalog_membership",
            "scene_fm_home_enter",
            "scene_fm_home_leave",
            "scene_fm_autoplay_preference",
            "set_scene_fm_autoplay_preference",
            "scene_fm_palette_read",
            "scene_fm_palette_sample",
            "scene_fm_palette_cancel",
            "pc_presentation",
            "search_presentation",
            "copy_text",
        ] {
            assert_ipc_response(
                &window,
                tauri::webview::InvokeRequest {
                    cmd: cmd.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: if cfg!(windows) {
                        "http://tauri.localhost"
                    } else {
                        "tauri://localhost"
                    }
                    .parse()
                    .unwrap(),
                    body: tauri::ipc::InvokeBody::default(),
                    headers: Default::default(),
                    invoke_key: INVOKE_KEY.into(),
                },
                Err(json!("Application commands require the main window")),
            );
        }
    }
}

#[test]
fn account_snapshot_and_stale_mutation_cross_registered_commands() {
    let directory = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
    let api: Arc<dyn Api> = Arc::new(NoNetwork);
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(
            api,
            json!({"userId":9,"nickname":"用户","cookie":"SECRET"}),
            &session.token().unwrap(),
        )
        .unwrap();
    let client = MusicClient::with_session(session, registry);
    let app = cumulus_desktop::configure(mock_builder())
        .manage(client)
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let request = |cmd: &str, body| tauri::webview::InvokeRequest {
        cmd: cmd.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: tauri::ipc::InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.into(),
    };
    let card_receipt = "0123456789abcdef0123456789abcdef";
    let selection_receipt = "abcdef0123456789abcdef0123456789";
    let local_id = "b".repeat(64);
    let revision = "a".repeat(64);
    let reorder = json!({
        "epoch":"0","id":"10","revision":revision,
        "rows":[{"kind":"online","id":"123"},{"kind":"local","id":local_id,"revision":revision}],
    });
    assert_ipc_response(
        &window,
        request("reorder_playlist_mixed", reorder.clone()),
        Err(json!({"code":"stale","message":"操作已取消或账号已切换","retryable":false})),
    );
    let mut untrusted = reorder;
    untrusted["rows"][1]["path"] = json!("forged.wav");
    let error =
        tauri::test::get_ipc_response(&window, request("reorder_playlist_mixed", untrusted))
            .expect_err("local row cannot carry a path");
    assert!(
        error
            .as_str()
            .is_some_and(|message| message.contains("invalid args `rows`"))
    );
    assert_ipc_response(
        &window,
        request(
            "playlist_mixed_reorder_receipt",
            json!({"epoch":"1","id":"10"}),
        ),
        Ok(json!({"epoch":"1","value":null})),
    );
    let collect_create = "download_collect_creation";
    let input = |command| json!({"epoch":"0","receipt":selection_receipt,"command":command});
    let stale_create = json!({"code":"stale","message":"操作已取消或账号已切换","retryable":false});
    let invalid_create =
        json!({"code":"invalid_input","message":"输入内容不符合要求","retryable":false});
    for command in [
        json!({"kind":"create","name":"合成","privatePlaylist":true}),
        json!({"kind":"status","reconcile":true}),
        json!({"kind":"add_created"}),
    ] {
        assert_ipc_response(
            &window,
            request(collect_create, input(command)),
            Err(stale_create.clone()),
        );
    }
    for command in [
        json!({"kind":"create","name":"合成","privatePlaylist":true,"songId":"301"}),
        json!({"kind":"status","reconcile":true,"operationId":selection_receipt}),
        json!({"kind":"add_created","playlistId":"81"}),
    ] {
        assert_ipc_response(
            &window,
            request(collect_create, input(command)),
            Err(invalid_create.clone()),
        );
    }
    let mut forged = input(json!({"kind":"add_created"}));
    forged["taskId"] = json!("not-a-native-task");
    assert_ipc_response(
        &window,
        request(collect_create, forged),
        Err(invalid_create),
    );
    let prepare = json!({"epoch":"0","playId":"stale-play","cardReceipt":card_receipt,
                        "anchorIndex":2,"indices":[1,3,2]});
    let prepared = json!({"epoch":"0","playId":"stale-play","cardReceipt":card_receipt,
                         "selectionReceipt":selection_receipt});
    for (command, body, overrides) in [
        (
            "player_voice_songs_download_prepare",
            prepare,
            vec![
                ("anchor", json!(1)),
                ("anchorSongId", json!("301")),
                ("anchorIndex", json!(-1)),
                ("indices", json!(["1", "3", "2"])),
                ("selectionReceipt", json!(selection_receipt)),
            ],
        ),
        (
            "player_voice_songs_download_cancel",
            prepared.clone(),
            vec![
                ("anchorIndex", json!(2)),
                ("indices", json!([1, 3, 2])),
                ("selectionReceipt", json!("301")),
            ],
        ),
        (
            "player_voice_songs_download_start",
            prepared.clone(),
            vec![
                ("anchorIndex", json!(2)),
                ("indices", json!([1, 3, 2])),
                ("selectionReceipt", json!("301")),
            ],
        ),
    ] {
        // The well-shaped body reaches the native epoch fence. The same
        // stale scope with malformed input must fail in the IPC schema first.
        assert_ipc_response(
            &window,
            request(command, body.clone()),
            Err(json!({"code":"stale","message":"操作已取消或账号已切换","retryable":false})),
        );
        let invalid =
            json!({"code":"invalid_input","message":"输入内容不符合要求","retryable":false});
        for (field, value) in overrides.into_iter().chain([
            ("options", json!({"quality":"lossless"})),
            ("quality", json!("lossless")),
            ("songIds", json!(["301", "302"])),
            ("songId", json!("301")),
            ("voiceId", json!("71")),
        ]) {
            let mut payload = body.clone();
            payload[field] = value;
            assert_ipc_response(&window, request(command, payload), Err(invalid.clone()));
        }
        let mut incomplete = body.clone();
        incomplete
            .as_object_mut()
            .unwrap()
            .remove(if command.ends_with("_prepare") {
                "anchorIndex"
            } else {
                "selectionReceipt"
            });
        for payload in [
            incomplete,
            json!({"target":body}),
            json!(["0", "stale-play", card_receipt]),
        ] {
            assert_ipc_response(&window, request(command, payload), Err(invalid.clone()));
        }
    }
    let command = "player_voice_songs_download_confirm_quality";
    let stale = json!({"code":"stale","message":"操作已取消或账号已切换","retryable":false});
    let invalid = json!({"code":"invalid_input","message":"输入内容不符合要求","retryable":false});
    for quality in [
        "vivid", "sky", "jymaster", "jyeffect", "hires", "lossless", "exhigh", "standard",
    ] {
        let mut payload = prepared.clone();
        payload["quality"] = json!(quality);
        // A valid tier reaches the native epoch fence; it is not authority
        // to download unless the current native receipt actually offered it.
        assert_ipc_response(&window, request(command, payload), Err(stale.clone()));
    }
    for value in [
        json!("dolby"),
        json!("higher"),
        json!("64aac"),
        json!("HIRES"),
        json!(320000),
        json!(null),
    ] {
        let mut payload = prepared.clone();
        payload["quality"] = value;
        assert_ipc_response(&window, request(command, payload), Err(invalid.clone()));
    }
    assert_ipc_response(
        &window,
        request(command, prepared.clone()),
        Err(invalid.clone()),
    );
    let guide = "open_voice_song_group_purchased_quality_guide";
    assert_ipc_response(
        &window,
        request(guide, prepared.clone()),
        Err(stale.clone()),
    );
    let opt_in = "player_voice_songs_download_prepare_with_destination";
    assert_ipc_response(
        &window,
        request(
            opt_in,
            json!({
                "epoch":"0","playId":"stale-play","cardReceipt":card_receipt,"anchorIndex":2,"indices":[1,3,2],
            }),
        ),
        Err(stale.clone()),
    );
    let destination = "player_voice_songs_download_destination";
    for operation in [
        json!({"kind":"page","sort":"default","page":0}),
        json!({"kind":"choose_liked","destinationReceipt":selection_receipt}),
        json!({"kind":"status","reconcile":true}),
        json!({"kind":"start","continuation":selection_receipt}),
    ] {
        assert_ipc_response(
            &window,
            request(destination, json!({"target":prepared,"command":operation})),
            Err(stale.clone()),
        );
    }
    for operation in [
        json!({"kind":"choose_liked","playlistId":"81"}),
        json!({"kind":"start","continuation":selection_receipt,"path":"/unexpected"}),
    ] {
        assert_ipc_response(
            &window,
            request(destination, json!({"target":prepared,"command":operation})),
            Err(invalid.clone()),
        );
    }
    for (key, value) in [
        ("url", json!("https://untrusted.invalid/help")),
        ("index", json!(2)),
        ("quality", json!("hires")),
        ("selectionReceipt", json!("301")),
    ] {
        let mut injected = prepared.clone();
        injected[key] = value;
        assert_ipc_response(&window, request(guide, injected), Err(invalid.clone()));
    }
    let mut confirmed = prepared;
    confirmed["quality"] = json!("hires");
    for (key, value) in [
        ("selectionReceipt", json!("301")),
        ("options", json!({"quality":"hires"})),
        ("songIds", json!(["301", "302"])),
        ("voiceId", json!("71")),
        ("indices", json!([1, 2])),
        ("playlistId", json!("81")),
        ("destinationReceipt", json!(selection_receipt)),
        ("directory", json!("/tmp/untrusted")),
        ("url", json!("https://untrusted.invalid/audio")),
    ] {
        let mut payload = confirmed.clone();
        payload[key] = value;
        assert_ipc_response(&window, request(command, payload), Err(invalid.clone()));
    }
    assert_ipc_response(
        &window,
        request(command, json!({"target":confirmed})),
        Err(invalid),
    );
    assert_ipc_response(
        &window,
        request("account_snapshot", json!({})),
        Ok(json!({
            "epoch":"1",
            "status":"authenticated",
            "nickname":"用户",
            "avatarUrl":null,
            "canSignOut":false,
            "profileShortcut":{"id":"9","notes":null,"following":null,"followers":null},
            "loginNotice":null,
        })),
    );
    assert_ipc_response(
        &window,
        request(
            "catalog_page",
            json!({"epoch":"1","query":{"kind":"artist","id":"12"},"refresh":false}),
        ),
        Ok(json!({"epoch":"1","value":null})),
    );
    assert_ipc_response(
        &window,
        request("set_liked", json!({"epoch":"0","id":"1","liked":true})),
        Err(json!({"code":"stale","message":"操作已取消或账号已切换","retryable":false})),
    );
    let stale = json!({"code":"stale","message":"操作已取消或账号已切换","retryable":false});
    for epoch in ["0", "1"] {
        let scope = json!({"epoch":epoch,"playId":"old-play","receipt":"old-voice-receipt"});
        for (command, body) in [
            (
                "player_vinyl_voice_detail_target",
                json!({"target":{
                    "epoch":epoch,"playId":"old-play","receipt":"old-voice-receipt","kind":"creator"
                }}),
            ),
            (
                "change_fresh_vinyl_podcast_subscription",
                json!({"scope":scope,"expected":false}),
            ),
            (
                "reconcile_fresh_vinyl_podcast_subscription",
                json!({"scope":scope}),
            ),
        ] {
            assert_ipc_response(&window, request(command, body), Err(stale.clone()));
        }
        let target = json!({"epoch":epoch,"playId":"old-play","cardReceipt":"old-card","selectionReceipt":"old-selection"});
        assert_ipc_response(
            &window,
            request(
                "player_voice_songs_collect_liked_destination",
                json!({"target":target}),
            ),
            Err(stale.clone()),
        );
        assert_ipc_response(
            &window,
            request(
                "player_voice_songs_collect_liked",
                json!({"target":target,"destinationReceipt":"old-row"}),
            ),
            Err(stale.clone()),
        );
    }
    for (command, body) in [
        ("relay_view", json!({"epoch":"0","refresh":true})),
        (
            "relay_setting_command",
            json!({"epoch":"0","command":{"kind":"setting_change","id":"ignored","revision":"ignored","enabled":true}}),
        ),
        (
            "relay_command",
            json!({"epoch":"0","command":{"kind":"dismiss","revision":"fixture"}}),
        ),
        ("account_settings_page", json!({"epoch":"0","refresh":true})),
        (
            "account_settings_command",
            json!({"epoch":"0","command":{"kind":"change","id":"00000000-0000-0000-0000-000000000001","expected_revision":"a".repeat(64),"change":{"key":"shareSetting","value":1}}}),
        ),
        (
            "history_page",
            json!({"epoch":"0","kind":"song","refresh":true}),
        ),
        (
            "history_command",
            json!({"epoch":"0","kind":"song","command":{"kind":"set_sync","enabled":false}}),
        ),
        (
            "history_sync_preference",
            json!({"epoch":"0","action":{"kind":"set","enabled":false,"confirmed_first_disable":true}}),
        ),
        (
            "collection_page",
            json!({"epoch":"0","query":{"kind":"saved_albums","offset":0,"limit":50},"refresh":true}),
        ),
        (
            "catalog_membership",
            json!({"epoch":"0","resource":{"kind":"artist","id":"12"},"action":{"kind":"set","desired":true}}),
        ),
        (
            "catalog_page",
            json!({"epoch":"0","query":{"kind":"artist","id":"12"},"refresh":true}),
        ),
        ("playlist_cover_tasks", json!({"epoch":"0"})),
        ("playlist_cover_image", json!({"epoch":"0","id":"old"})),
        (
            "playlist_cover_command",
            json!({"epoch":"0","id":"old","command":{"kind":"continue"}}),
        ),
        (
            "pick_playlist_cover",
            json!({"epoch":"0","playlistId":"10","revision":"old","replaceId":null}),
        ),
        (
            "member_page",
            json!({"epoch":"0","query":{"kind":"home"},"refresh":true}),
        ),
        ("member_rights", json!({"epoch":"0","refresh":true})),
        (
            "open_member_link",
            json!({"epoch":"0","revision":"stale","key":"0:1:0"}),
        ),
        ("scene_fm_view", json!({"epoch":"0","refresh":true})),
        ("pc_presentation", json!({"epoch":"0"})),
        (
            "search_presentation",
            json!({"epoch":"0","receipt":"0123456789abcdef0123456789abcdef","revision":"1"}),
        ),
        (
            "scene_fm_palette_read",
            json!({"epoch":"0","revision":"1","voiceId":"601","pixelRatio":1.0}),
        ),
        (
            "scene_fm_palette_sample",
            json!({"epoch":"0","receipt":"old","width":1,"height":1,"rgba":[0,0,0,255]}),
        ),
        (
            "scene_fm_palette_cancel",
            json!({"epoch":"0","receipt":"old"}),
        ),
        ("scene_fm_home_enter", json!({"epoch":"0"})),
        ("scene_fm_home_leave", json!({"epoch":"0","visit":"old"})),
        ("scene_fm_autoplay_preference", json!({"epoch":"0"})),
        (
            "set_scene_fm_autoplay_preference",
            json!({"epoch":"0","enabled":true}),
        ),
        (
            "scene_fm_command",
            json!({"epoch":"0","command":{"kind":"home_automatic","revision":"1","visit":"old"}}),
        ),
        (
            "fm_discovery",
            json!({"epoch":"0","query":{"kind":"home"},"refresh":true}),
        ),
        (
            "fm_view",
            json!({"epoch":"0","source":{"kind":"personal"},"refresh":false}),
        ),
        (
            "fm_command",
            json!({"epoch":"0","source":{"kind":"personal"},"command":{"kind":"trash","entry_id":"old"}}),
        ),
        ("fm_trash", json!({"epoch":"0","page":1,"cursor":""})),
        (
            "fm_reasons",
            json!({"epoch":"0","source":{"kind":"personal"},"entryId":"old"}),
        ),
        ("saved_accounts", json!({"epoch":"0"})),
        ("remove_saved_account", json!({"epoch":"0","id":"fixture"})),
        (
            "begin_login",
            json!({"epoch":"0","mode":"email","savedId":null}),
        ),
        ("logout_account", json!({"epoch":"0","remote":false})),
        ("login_snapshot", json!({"operationId":"unknown"})),
        ("cancel_login", json!({"operationId":"unknown"})),
        (
            "login_command",
            json!({"operationId":"unknown","command":{"kind":"reconcile"}}),
        ),
        ("together_view", json!({"epoch":"0","refresh":false})),
        (
            "together_statistics",
            json!({"epoch":"0","relationKey":"unknown"}),
        ),
        (
            "together_command",
            json!({"epoch":"0","command":{"kind":"create"}}),
        ),
        ("podcast_state", json!({"epoch":"0","refresh":false})),
        (
            "podcast_radio_order",
            json!({"epoch":"0","query":{"kind":"read","id":"81"}}),
        ),
        (
            "podcast_page",
            json!({"epoch":"0","query":{"scope":{"kind":"podcasts"},"keyword":"","offset":0,"limit":50,"revision":null},"refresh":false}),
        ),
        (
            "podcast_command",
            json!({"epoch":"0","command":{"kind":"change","action":"like","id":"81","desired":true}}),
        ),
        ("local_match_view", json!({"epoch":"0","ids":[]})),
        (
            "local_match_command",
            json!({"epoch":"0","command":{"kind":"control","id":"x","action":"pause"}}),
        ),
        (
            "mv_page",
            json!({"epoch":"0","query":{"keyword":"","offset":0,"limit":24,"revision":null},"refresh":false}),
        ),
        ("mv_detail", json!({"epoch":"0","id":"123"})),
        (
            "comments_resource",
            json!({"epoch":"0","resource":{"kind":"track","id":"123"},"refresh":true}),
        ),
        (
            "comments_page",
            json!({"epoch":"0","resource":{"kind":"track","id":"123"},"query":{"sort":"latest","limit":20,"offset":0,"beforeTime":0,"commentId":null},"refresh":true}),
        ),
        (
            "comments_like",
            json!({"epoch":"0","resource":{"kind":"track","id":"123"},"id":"101","desired":true}),
        ),
        (
            "comments_reconcile",
            json!({"epoch":"0","resource":{"kind":"track","id":"123"},"id":"101"}),
        ),
        (
            "comments_prepare_write",
            json!({"epoch":"0","resource":{"kind":"track","id":"123"},"command":{"kind":"publish","content":"fixture","replyTo":null}}),
        ),
        (
            "comments_write_operations",
            json!({"epoch":"0","resource":{"kind":"track","id":"123"}}),
        ),
        (
            "comments_write_command",
            json!({"epoch":"0","id":"00000000000000000000000000000000","command":{"kind":"submit"}}),
        ),
        (
            "comments_floor",
            json!({"epoch":"0","resource":{"kind":"track","id":"123"},"query":{"parentId":"101","time":-1,"limit":20},"refresh":true}),
        ),
        ("mv_collect", json!({"epoch":"0","id":"123","desired":true})),
        ("mv_reconcile", json!({"epoch":"0","id":"123"})),
        ("mv_like", json!({"epoch":"0","id":"123","desired":true})),
        ("mv_like_reconcile", json!({"epoch":"0","id":"123"})),
        (
            "mv_recommendations",
            json!({"epoch":"0","id":"123","refresh":true}),
        ),
        ("video_snapshot", json!({"epoch":"0"})),
        (
            "video_action",
            json!({"epoch":"0","action":{"kind":"play"}}),
        ),
        (
            "player_action",
            json!({"epoch":"0","action":{"kind":"equalizer","expected_revision":"0","settings":{"enabled":true,"gains":[1,0,0,0,0,0,0,0,0,0]}}}),
        ),
        (
            "player_action",
            json!({"epoch":"0","action":{"kind":"repair_equalizer"}}),
        ),
        (
            "video_frame",
            json!({"epoch":"0","playId":"old","after":"0","width":320,"height":180}),
        ),
        (
            "player_action",
            json!({"epoch":"0","action":{"kind":"play"}}),
        ),
        (
            "play_selection",
            json!({"epoch":"0","selection":{"kind":"songs","ids":["1"],"index":0,"quality":"lossless"}}),
        ),
        ("player_queue", json!({"epoch":"0"})),
        (
            "player_queue_collection",
            json!({"epoch":"0","revision":"0"}),
        ),
        ("player_lyrics", json!({"epoch":"0","playId":"old"})),
        ("playlist_editor", json!({"epoch":"0","id":"10"})),
        ("playlist_tags", json!({"epoch":"0"})),
        (
            "playlist_collect_page",
            json!({"epoch":"0","sort":"recently","page":0}),
        ),
        ("playlist_order_view", json!({"epoch":"0"})),
        ("playlist_operations", json!({"epoch":"0"})),
        (
            "playlist_mixed_reorder_receipt",
            json!({"epoch":"0","id":"10"}),
        ),
        (
            "reconcile_playlist_mixed_reorder",
            json!({"epoch":"0","id":"10","operationId":selection_receipt}),
        ),
        (
            "downloads",
            json!({"epoch":"0","query":{"tab":"active","keyword":"","sort":"completed","descending":true,"offset":0,"limit":100}}),
        ),
        (
            "download_quality_preferences",
            json!({"epoch":"0","refresh":true}),
        ),
        (
            "download_command",
            json!({"epoch":"0","command":{"kind":"all","action":"cancel"}}),
        ),
        (
            "play_downloads",
            json!({"epoch":"0","taskIds":["old"],"index":0}),
        ),
        ("pick_download_directory", json!({"epoch":"0"})),
        (
            "open_download_directory",
            json!({"epoch":"0","taskId":null}),
        ),
        (
            "mutate_playlist",
            json!({"epoch":"0","command":{"kind":"create","name":"test","private":true}}),
        ),
        (
            "reconcile_playlist",
            json!({"epoch":"0","operationId":"old"}),
        ),
    ] {
        assert_ipc_response(
            &window,
            request(command, body),
            Err(json!({"code":"stale","message":"操作已取消或账号已切换","retryable":false})),
        );
    }
}

/// Verify the actual registered command and its serialization through Tauri's
/// IPC dispatcher, rather than calling the Rust service directly.
#[test]
fn application_info_crosses_the_desktop_bridge() {
    let app = cumulus_desktop::configure(mock_builder())
        .build(mock_context(noop_assets()))
        .expect("build mock host");
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("build mock window");

    assert_ipc_response(
        &window,
        tauri::webview::InvokeRequest {
            cmd: "get_app_info".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: tauri::ipc::InvokeBody::default(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
        Ok(json!({
            "name": "Cumulus",
            "coreVersion": env!("CARGO_PKG_VERSION"),
            "protocolVersion": 1
        })),
    );
}
