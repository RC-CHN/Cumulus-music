pub mod captcha;
mod catalog;
mod clipboard;
mod cloud;
mod collections;
mod commands;
mod comments;
pub mod desktop_lyrics;
mod downloads;
mod effects;
mod fm;
mod fonts;
mod graphics;
mod history;
mod identity;
pub mod lifecycle;
mod local;
mod member;
pub mod mini;
mod mv;
mod platform;
mod player;
mod playlist_cover;
mod playlists;
mod podcast;
mod rankings;
mod relay;
pub mod renewal;
mod search;
mod settings;
pub mod shortcuts;
pub mod system_media;
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
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(watchman::collection::lifecycle())
        .plugin(shortcuts::lifecycle())
        .manage(cumulus_core::Application)
        .manage(mini::Host::default())
        .manage(desktop_lyrics::Host::default())
        .manage(fonts::Host::default())
        .manage(player::PlayerStreams::default())
        .manage(together::CloseCoordinator::default())
        .manage(renewal::RenewalPump::default())
        .manage(watchman::EventHost::default())
        .manage(watchman::collection::MeasurementBridge::default())
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                mini::dismiss(window.app_handle(), None, false);
            }
            if !desktop_lyrics::closing(window, event)
                && !mini::closing(window, event)
                && !lifecycle::closing(window, event)
            {
                together::closing(window, event);
            }
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Focused(true)) {
                renewal::wake(window.app_handle());
            }
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if window.label() == "main" {
                    desktop_lyrics::dismiss(window.app_handle(), None);
                    mini::dismiss(window.app_handle(), None, false);
                    lifecycle::stop(window.app_handle());
                    system_media::stop(window.app_handle());
                    shortcuts::stop(window.app_handle());
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
            if desktop_lyrics::owns(
                invoke.message.webview_ref().app_handle(),
                invoke.message.webview_ref().label(),
            ) {
                let handler: fn(tauri::ipc::Invoke<R>) -> bool = tauri::generate_handler![
                    desktop_lyrics::commands::desktop_lyric_read,
                    desktop_lyrics::commands::desktop_lyric_ready,
                    desktop_lyrics::commands::desktop_lyric_action,
                    shortcuts::settings::shortcut_status,
                    shortcuts::local::shortcut_attach,
                    shortcuts::local::shortcut_detach,
                    shortcuts::local::shortcut_press,
                ];
                return handler(invoke);
            }
            if mini::owns(
                invoke.message.webview_ref().app_handle(),
                invoke.message.webview_ref().label(),
            ) {
                let handler: fn(tauri::ipc::Invoke<R>) -> bool = tauri::generate_handler![
                    mini::commands::mini_read,
                    mini::commands::mini_ready,
                    mini::commands::mini_return,
                    mini::commands::mini_action,
                    mini::commands::mini_queue,
                    mini::commands::mini_panel,
                    mini::commands::mini_preferences_command,
                    shortcuts::settings::shortcut_status,
                    shortcuts::local::shortcut_attach,
                    shortcuts::local::shortcut_detach,
                    shortcuts::local::shortcut_press,
                ];
                return handler(invoke);
            }
            if invoke.message.webview_ref().label() != "main" {
                invoke
                    .resolver
                    .reject("Application commands require the main window");
                return true;
            }
            let handler: fn(tauri::ipc::Invoke<R>) -> bool = tauri::generate_handler![
                clipboard::copy_text,
                mini::commands::mini_enter,
                desktop_lyrics::settings::desktop_lyric_settings,
                desktop_lyrics::settings::desktop_lyric_change,
                desktop_lyrics::settings::desktop_lyric_window_command,
                fonts::system_font_catalog,
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
                search::search_discovery,
                search::search_results,
                search::search_history,
                commands::daily_songs,
                rankings::ranking_page,
                catalog::catalog_page,
                collections::collection_page,
                history::history_page,
                history::history_command,
                settings::account_settings_page,
                settings::startup_preferences,
                settings::window_preferences,
                system_media::settings::system_media_preferences,
                system_media::settings::system_media_preferences_command,
                system_media::settings::system_media_status,
                shortcuts::settings::shortcut_settings,
                shortcuts::settings::shortcut_settings_command,
                shortcuts::settings::shortcut_status,
                shortcuts::local::shortcut_capture,
                shortcuts::local::shortcut_attach,
                shortcuts::local::shortcut_detach,
                shortcuts::local::shortcut_press,
                shortcuts::recording::shortcut_begin_recording,
                shortcuts::recording::shortcut_end_recording,
                settings::window_preferences_command,
                lifecycle::window_lifecycle,
                lifecycle::window_close_command,
                settings::output_settings,
                settings::output_command,
                settings::startup_preferences_command,
                settings::account_settings_command,
                relay::relay_view,
                relay::relay_publication_view,
                relay::relay_publication_command,
                relay::relay_command,
                relay::relay_setting_command,
                catalog::catalog_membership,
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
                commands::like_recovery,
                commands::recover_like,
                player::watch_player,
                player::unwatch_player,
                player::player_action,
                player::quality::player_quality_details,
                player::quality::player_quality_trial,
                player::play_selection,
                player::player_queue,
                player::player_queue_collection,
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
            lifecycle::install(app.handle())?;
            system_media::install(app.handle());
            shortcuts::install(app.handle());
            desktop_lyrics::start(app.handle());
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
