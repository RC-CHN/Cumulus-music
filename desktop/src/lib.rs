pub mod captcha;
mod catalog;
mod cloud;
mod commands;
mod comments;
mod downloads;
mod effects;
mod fm;
mod graphics;
mod identity;
mod local;
mod member;
mod mv;
mod platform;
mod player;
mod playlist_cover;
mod playlists;
mod podcast;
mod rankings;
pub mod renewal;
mod together;
pub mod watchman;
pub use effects::configure_native_audio;
pub use graphics::prepare_process as prepare_graphics;
use tauri::Manager;

/// Shared by the desktop runtime and IPC contract verification.
pub fn configure<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    captcha::protocol(builder)
        .plugin(graphics::plugin())
        .plugin(
            tauri::plugin::Builder::<R>::new("media-locale")
                .setup(|_, _| {
                    platform::initialize_numeric_locale()?;
                    Ok(())
                })
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(watchman::collection::lifecycle())
        .manage(cumulus_core::Application)
        .manage(player::PlayerStreams::default())
        .manage(together::CloseCoordinator::default())
        .manage(renewal::RenewalPump::default())
        .manage(watchman::EventHost::default())
        .manage(watchman::collection::MeasurementBridge::default())
        .on_window_event(|window, event| {
            together::closing(window, event);
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Focused(true)) {
                renewal::wake(window.app_handle());
            }
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if window.label() == "main" {
                    renewal::stop(window.app_handle());
                    for (label, child) in window.app_handle().webview_windows() {
                        if label.starts_with("captcha-") {
                            let _ = child.destroy();
                        }
                    }
                    if let Some(host) =
                        window.try_state::<std::sync::Arc<watchman::host::NativeWatchman>>()
                    {
                        let _ = host.close();
                    }
                    let _ = window.state::<watchman::EventHost>().stop();
                    let _ = window
                        .state::<watchman::collection::MeasurementBridge>()
                        .page_started();
                }
                player::close(window);
            }
        })
        .invoke_handler(|invoke| {
            if invoke.message.webview_ref().label() != "main" {
                invoke
                    .resolver
                    .reject("Application commands require the main window");
                return true;
            }
            let handler: fn(tauri::ipc::Invoke<R>) -> bool = tauri::generate_handler![
                captcha::solve_login_captcha,
                watchman::watchman_observation,
                watchman::collection::broker::watchman_measurement,
                auth::begin_login,
                auth::login_snapshot,
                auth::login_command,
                auth::cancel_login,
                auth::saved_accounts,
                auth::remove_saved_account,
                auth::logout_account,
                renewal::session_renewal,
                renewal::renew_session,
                commands::get_app_info,
                together::together_view,
                together::together_command,
                podcast::podcast_page,
                podcast::scene_fm_view,
                podcast::scene_fm_command,
                podcast::podcast_state,
                podcast::podcast_command,
                local::local_page,
                local::local_command,
                local::pick_local_directory,
                local::local_match_view,
                local::local_match_command,
                commands::account_snapshot,
                commands::restore_account,
                commands::begin_qr,
                commands::poll_qr,
                commands::cancel_qr,
                commands::account_playlists,
                commands::playlist_page,
                commands::search_songs,
                commands::daily_songs,
                rankings::ranking_page,
                catalog::catalog_page,
                member::member_page,
                member::member_rights,
                member::open_member_link,
                fm::fm_view,
                fm::fm_discovery,
                fm::fm_reasons,
                fm::fm_command,
                fm::fm_trash,
                commands::liked_ids,
                commands::set_liked,
                player::watch_player,
                player::unwatch_player,
                player::player_action,
                player::play_selection,
                player::player_queue,
                player::player_lyrics,
                player::lyric_preferences,
                player::lyric_offset,
                playlists::playlist_editor,
                playlists::playlist_tags,
                playlists::playlist_order_view,
                playlists::mutate_playlist,
                playlists::reconcile_playlist,
                playlists::playlist_operations,
                playlist_cover::playlist_cover_tasks,
                playlist_cover::playlist_cover_image,
                playlist_cover::playlist_cover_command,
                playlist_cover::pick_playlist_cover,
                downloads::downloads,
                downloads::download_command,
                downloads::play_downloads,
                downloads::pick_download_directory,
                downloads::open_download_directory,
                cloud::cloud_page,
                cloud::cloud_command,
                cloud::pick_cloud_files,
                mv::mv_page,
                mv::mv_detail,
                mv::mv_collect,
                mv::mv_reconcile,
                mv::mv_like,
                mv::mv_like_reconcile,
                mv::mv_recommendations,
                mv::video_snapshot,
                mv::video_action,
                mv::video_frame,
                comments::comments_page,
                comments::comments_like,
                comments::comments_reconcile,
                comments::comments_prepare_write,
                comments::comments_write_operations,
                comments::comments_write_command,
                comments::comments_floor,
            ];
            handler(invoke)
        })
}

pub fn run() {
    if !graphics::bootstrap() {
        return;
    }
    // SAFETY: desktop entry point, before the GUI runtime and worker creation.
    unsafe { prepare_graphics() };
    configure(tauri::Builder::default())
        .setup(|app| {
            let root = match std::env::var_os("CUMULUS_DATA_DIR") {
                Some(value) => std::path::PathBuf::from(value),
                None => app.path().app_data_dir()?,
            };
            let existing = std::env::var_os("CUMULUS_SESSION_DIR").map(std::path::PathBuf::from);
            let profile = identity::profile(&root)?;
            let context = watchman::host::install(app, &root, &profile)?;
            let client = cumulus_core::client::MusicClient::open_with_request_context(
                &root,
                existing.as_deref(),
                profile.clone(),
                None,
                Some(context),
            )?;
            let client = renewal::configure(client, &profile);
            let client = configure_native_im(client, &app.path().resource_dir()?);
            let client = configure_native_audio(client, &app.path().resource_dir()?, false);
            tauri::async_runtime::block_on(client.start_local_library())?;
            app.manage(client);
            renewal::start(app.handle());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("could not start Cumulus");
}

/// SDK paths are native launch/package configuration, never accepted over IPC.
pub fn configure_native_im(
    client: cumulus_core::client::MusicClient,
    resources: &std::path::Path,
) -> cumulus_core::client::MusicClient {
    let directory = std::env::var_os("CUMULUS_NIM_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let p = resources.join("native/nim");
            p.is_dir().then_some(p)
        });
    match directory {
        Some(path) => client.with_native_im(path),
        None => client,
    }
}
mod auth;
