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
            "mini_preferences_command",
            "get_app_info",
            "account_snapshot",
            "login_command",
            "pick_local_directory",
            "player_action",
            "solve_login_captcha",
            "catalog_page",
            "collection_page",
            "history_page",
            "history_command",
            "account_settings_page",
            "account_settings_command",
            "relay_view",
            "relay_command",
            "relay_setting_command",
            "catalog_membership",
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
    assert_ipc_response(
        &window,
        request("account_snapshot", json!({})),
        Ok(
            json!({"epoch":"1","status":"authenticated","nickname":"用户","avatarUrl":null,"canSignOut":false,"loginNotice":null}),
        ),
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
        (
            "scene_fm_command",
            json!({"epoch":"0","command":{"kind":"automatic","revision":"1"}}),
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
            "together_command",
            json!({"epoch":"0","command":{"kind":"create"}}),
        ),
        ("podcast_state", json!({"epoch":"0","refresh":false})),
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
        ("player_lyrics", json!({"epoch":"0","playId":"old"})),
        ("playlist_editor", json!({"epoch":"0","id":"10"})),
        ("playlist_tags", json!({"epoch":"0"})),
        ("playlist_order_view", json!({"epoch":"0"})),
        ("playlist_operations", json!({"epoch":"0"})),
        (
            "downloads",
            json!({"epoch":"0","query":{"tab":"active","keyword":"","sort":"completed","descending":true,"offset":0,"limit":100}}),
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
