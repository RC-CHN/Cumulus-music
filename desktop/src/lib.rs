mod account_avatar;
mod artist_onboarding;
pub mod audio_diagnostics;
pub mod daily_notification;
mod external_import;
pub use account_avatar::AccountAvatarPicker;
mod browser;
pub use browser::LoginDocumentBrowser;
/// Native host construction only; profiles never become renderer arguments.
pub use identity::profile as native_profile;
pub mod captcha;
mod catalog;
mod clipboard;
mod cloud;
mod cold_offline;
mod collections;
mod commands;
mod comments;
mod community;
pub mod desktop_lyrics;
mod downloads;
pub mod effect_activation;
mod effects;
mod file_manager;
mod fm;
mod fonts;
mod graphics;
mod history;
mod identity;
pub mod lifecycle;
mod local;
mod login_device;
mod member;
mod messages;
pub use messages::counts::{
    start_message_count_polling, window_visibility as message_count_window_visibility,
};
pub mod mini;
mod mv;
mod network_diagnostics;
pub mod pc_presentation;
mod platform;
mod playback_cache;
pub mod playback_keep_awake;
mod player;
mod playlist_cover;
mod playlists;
mod podcast;
mod profile_relations;
mod rankings;
#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    feature = "recognition-monitor-lab"
))]
#[allow(
    dead_code,
    reason = "private Linux recognition lab has no application command"
)]
mod recognition_capture;
mod relay;
mod remote_skins;
pub mod renewal;
mod search;
mod settings;
pub mod shortcuts;
mod style_tags;
pub mod system_media;
mod together;
mod updates;
pub use updates::UpdateAdapters;
pub mod watchman;
pub use effects::configure_native_audio;
#[doc(hidden)]
pub use fonts::{Family as FontCatalogProofFamily, FontCatalogProof};
pub use graphics::prepare_process as prepare_graphics;
#[doc(hidden)]
pub use network_diagnostics::DefaultRouteObserver;
#[doc(hidden)]
pub use platform::queue_tip_window_foreground;
pub use playback_cache::configure_streaming_cache;
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
        .plugin(messages::counts::lifecycle())
        .manage(cumulus_core::Application)
        .manage(updates::UpdateGate::default())
        .manage(mini::Host::default())
        .manage(desktop_lyrics::Host::default())
        .manage(fonts::Host::default())
        .manage(player::PlayerStreams::default())
        .manage(together::CloseCoordinator::default())
        .manage(renewal::RenewalPump::default())
        .manage(pc_presentation::Host::default())
        .manage(daily_notification::Host::default())
        .manage(history::sync_observer::Host::default())
        .manage(search::RightsPump::default())
        .manage(messages::counts::Host::default())
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
            if window.label() == "main"
                && matches!(
                    event,
                    tauri::WindowEvent::Focused(_) | tauri::WindowEvent::Resized(_)
                )
            {
                messages::counts::window_visibility(window.app_handle(), false);
            }
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if window.label() == "main" {
                    #[cfg(target_os = "linux")]
                    if let Some(relay) = window.try_state::<native_open::linux::Relay>() {
                        relay.stop();
                    }
                    daily_notification::stop(window.app_handle());
                    cold_offline::close(window.app_handle());
                    desktop_lyrics::dismiss(window.app_handle(), None);
                    mini::dismiss(window.app_handle(), None, false);
                    lifecycle::stop(window.app_handle());
                    system_media::stop(window.app_handle());
                    shortcuts::stop(window.app_handle());
                    renewal::stop(window.app_handle());
                    pc_presentation::stop(window.app_handle());
                    history::sync_observer::stop(window.app_handle());
                    member::stop_lifecycle(window.app_handle());
                    messages::counts::stop(window.app_handle());
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
                    mini::feedback::mini_fm_feedback_view,
                    mini::feedback::mini_fm_feedback_reasons,
                    mini::feedback::mini_fm_feedback,
                    mini::menu::mini_lyric_menu_read,
                    mini::menu::mini_lyric_menu_action,
                    mini::menu::mini_exit,
                    mini::menu::mini_lyric_report,
                    mini::intelligence::mini_intelligence,
                    mini::intelligence::mini_mode,
                    mini::intelligence::mini_intelligence_cancel,
                    mini::commands::mini_queue,
                    mini::commands::mini_panel,
                    mini::preferences::mini_preferences_command,
                    mini::preferences::mini_preferences_apply,
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
                cold_offline::cold_offline_open,
                cold_offline::cold_offline_next,
                cold_offline::cold_offline_play,
                cold_offline::cold_offline_stop,
                cold_offline::cold_offline_status,
                cold_offline::cold_offline_close,
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
                auth::login_presentation,
                auth::select_login_country,
                auth::open_login_document,
                auth::login_snapshot,
                auth::login_command,
                auth::cancel_login,
                auth::saved_accounts,
                auth::quick_login_access,
                auth::set_quick_login_offer,
                auth::quick_login_eligibility,
                auth::begin_server_quick_login,
                auth::server_quick_login_command,
                auth::begin_active_server_switch,
                auth::active_server_switch_command,
                auth::remove_saved_account,
                auth::logout_account,
                renewal::session_renewal,
                renewal::renew_session,
                commands::get_app_info,
                updates::check_for_updates,
                updates::open_update_release_page,
                updates::verify_update_package,
                network_diagnostics::local_network_diagnostics,
                effect_activation::effect_peq_pending,
                effect_activation::effect_peq_status,
                effect_activation::effect_peq_reconcile,
                effect_activation::effect_peq_prepare,
                effect_activation::effect_peq_activate,
                effect_activation::effect_peq_pause,
                effect_activation::effect_peq_stop,
                effects::effect_catalogue,
                effects::effect_detail,
                effects::effect_device_brands,
                effects::effect_device_models,
                effects::effect_device_material,
                effect_activation::effect_device_apply,
                effect_activation::effect_device_remembered,
                effect_activation::effect_device_restore,
                effect_activation::effect_device_clear,
                effect_activation::effect_device_compare,
                together::together_view,
                together::together_statistics,
                together::together_command,
                podcast::podcast_page,
                podcast::podcast_program_comment_count,
                podcast::podcast_open_program_download_directory,
                podcast::podcast_recent_program_menu,
                podcast::podcast_recent_program,
                podcast::podcast_open_recent_program_download_directory,
                podcast::podcast_subscription_recommendations,
                podcast::podcast_radio_order,
                podcast::audiobook_skip_settings,
                podcast::player_podcast_resume,
                podcast::podcast_recent,
                podcast::podcast_clear_subscribed_badge,
                podcast::profile_podcasts,
                podcast::open_profile_podcast_management,
                podcast::podcast_catalogue,
                podcast::podcast_home,
                podcast::podcast_rankings,
                podcast::scene_fm_view,
                podcast::scene_fm_command,
                podcast::scene_fm_palette_read,
                podcast::scene_fm_palette_sample,
                podcast::scene_fm_palette_cancel,
                podcast::scene_fm_home_enter,
                podcast::scene_fm_home_leave,
                podcast::scene_fm_autoplay_preference,
                podcast::set_scene_fm_autoplay_preference,
                podcast::podcast_state,
                podcast::podcast_search_membership,
                podcast::podcast_command,
                local::local_page,
                local::local_collect_prepare,
                local::local_collect_destinations,
                local::local_collect_existing,
                local::local_collect_cancel,
                local::local_command,
                local::pick_local_directory,
                local::open_local_track_directory,
                local::reveal_local_folder,
                local::local_match_view,
                local::local_match_command,
                commands::account_snapshot,
                artist_onboarding::artist_onboarding_offer,
                artist_onboarding::artist_onboarding_candidates,
                artist_onboarding::artist_onboarding_save,
                artist_onboarding::artist_onboarding_preview,
                artist_onboarding::artist_onboarding_close,
                artist_onboarding::artist_onboarding_result,
                commands::account_profile_detail,
                account_avatar::account_avatar_tasks,
                account_avatar::account_avatar_image,
                account_avatar::pick_account_avatar,
                account_avatar::account_avatar_command,
                commands::account_profile_edit_page,
                commands::account_profile_regions,
                commands::account_profile_edit_command,
                commands::account_bindings,
                commands::account_binding_qr,
                commands::account_login_devices,
                commands::account_login_device_removal,
                commands::account_login_device_removal_command,
                commands::restore_account,
                commands::begin_qr,
                commands::begin_classic_remove_guide_qr,
                commands::dismiss_classic_remove_guide,
                commands::poll_qr,
                commands::qr_identity_pending,
                commands::reconcile_qr,
                commands::cancel_qr,
                commands::account_playlists,
                commands::playlist_page,
                commands::playlist_subscribers,
                commands::playlist_tag_page,
                commands::playlist_tag_catalogue,
                commands::playlist_top_tags,
                commands::playlist_square_recommendations,
                commands::playlist_essential_categories,
                commands::playlist_essential_page,
                commands::playlist_creator_profile,
                profile_relations::profile_play_record,
                profile_relations::remove_profile_play_record,
                profile_relations::profile_follow,
                profile_relations::profile_blacklist_list,
                profile_relations::profile_blacklist,
                profile_relations::profile_report,
                profile_relations::profile_relation_access,
                profile_relations::profile_relation_page,
                commands::search_songs,
                search::search_discovery,
                search::search_results,
                pc_presentation::pc_presentation,
                pc_presentation::search_presentation,
                search::search_refresh_privileges,
                search::search_reason_page,
                search::search_history,
                commands::daily_songs,
                commands::style_recommendations,
                commands::restore_style_recommendations,
                commands::style_home_seeds,
                commands::seeded_style_recommendations,
                style_tags::style_recommendation_categories,
                style_tags::style_tag_operations,
                style_tags::save_style_tag,
                style_tags::reconcile_style_tag,
                style_tags::accept_current_style_tag,
                rankings::ranking_page,
                catalog::catalog_page,
                catalog::discovery_home,
                catalog::ordinary_essential,
                catalog::vertical_zone_menu,
                catalog::vertical_zone_page,
                catalog::vertical_zone_playlist_origin,
                catalog::vertical_zone_playlist_detail,
                catalog::vertical_zone_playlist_batch_page,
                catalog::vertical_zone_fm_preview,
                catalog::vertical_zone_fm_card,
                catalog::ordinary_essential_books,
                catalog::discovery_home_rankings,
                catalog::followed_new_song_page,
                catalog::followed_new_song_latest,
                catalog::release_notification,
                community::community_feed,
                community::community_topic,
                community::community_topic_slice,
                community::community_topic_notice,
                community::community_note_delete,
                community::community_note_report,
                community::community_composer_search,
                community::community_composer_draft,
                community::community_composer_publish,
                community::profile_notes,
                community::community_new_song_notice,
                community::community_detail,
                community::community_comments,
                catalog::artist_gallery,
                catalog::latest_releases,
                collections::collection_page,
                history::history_page,
                history::listened_books,
                history::history_command,
                history::history_sync_preference,
                settings::account_settings_page,
                settings::startup_preferences,
                settings::queue_click_preferences,
                settings::queue_click_preferences_command,
                autostart::autostart_status,
                autostart::autostart_change,
                settings::window_preferences,
                system_media::settings::system_media_preferences,
                system_media::settings::system_media_preferences_command,
                system_media::settings::system_media_status,
                system_media::settings::system_media_guide_available,
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
                playback_cache::playback_cache_state,
                playback_cache::playback_cache_set_capacity,
                playback_cache::playback_cache_clear,
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
                member::member_portal_qr,
                member::member_card_offer,
                member::member_card_offer_qr,
                member::open_member_card_offer,
                member::member_sign_card,
                member::member_welfare_available,
                member::member_welfare_list,
                remote_skins::remote_skin_catalog,
                remote_skins::remote_skin_image,
                member::member_sign_detail,
                member::member_sign_command,
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
                player::player_chorus,
                player::player_comment_count,
                player::player_vinyl_comments_available,
                player::vinyl_similar::player_vinyl_similar_available,
                player::vinyl_similar::player_vinyl_recommendation_placement,
                player::vinyl_similar::player_vinyl_wiki,
                player::vinyl_similar::player_vinyl_similar_songs,
                player::vinyl_similar::player_vinyl_related_playlists,
                player::vinyl_similar::player_vinyl_similar_play,
                player::voice_related::player_vinyl_voice_detail,
                player::voice_related::player_vinyl_voice_detail_target,
                player::voice_related::change_fresh_vinyl_podcast_subscription,
                player::voice_related::reconcile_fresh_vinyl_podcast_subscription,
                player::voice_related::player_vinyl_voice_recommendations,
                player::voice_related::player_vinyl_voice_recommendations_next,
                player::voice_related::player_vinyl_voice_recommendation_target,
                player::voice_related::player_voice_songs,
                player::voice_related::player_voice_song_play,
                player::voice_related::player_voice_songs_selection,
                player::voice_related::player_voice_song_like,
                player::voice_song_collect::player_voice_songs_collect_prepare,
                player::voice_song_collect::player_voice_songs_collect_destinations,
                player::voice_song_collect::player_voice_songs_collect_existing,
                player::voice_song_collect::player_voice_songs_collect_liked_destination,
                player::voice_song_collect::player_voice_songs_collect_liked,
                player::voice_song_collect::player_voice_songs_collect_status,
                player::voice_song_collect::player_voice_songs_collect_create_action,
                player::voice_song_download::player_voice_song_download_prepare,
                player::voice_song_download::purchased::player_voice_song_purchased_download,
                player::voice_song_download::purchased::open_voice_song_purchased_quality_guide,
                player::voice_song_download::player_voice_song_download_start,
                player::voice_song_download::player_voice_song_download_choose_playlist,
                player::voice_song_download::player_voice_song_download_picker_status,
                player::voice_song_download::player_voice_song_download_create_playlist,
                player::voice_song_download::player_voice_song_download_creation_status,
                player::voice_song_download::player_voice_song_download_add_created_playlist,
                player::voice_song_download::player_voice_song_download_start_from_playlist,
                player::voice_song_group_download::player_voice_songs_download_prepare,
                player::voice_song_group_download::destination::player_voice_songs_download_prepare_with_destination,
                player::voice_song_group_download::destination::player_voice_songs_download_destination,
                player::voice_song_group_download::player_voice_songs_download_cancel,
                player::voice_song_group_download::player_voice_songs_download_start,
                player::voice_song_group_download::player_voice_songs_download_confirm_quality,
                player::voice_song_group_download::open_voice_song_group_purchased_quality_guide,
                player::player_vinyl_comments,
                player::player_vinyl_comment_like_operations,
                player::player_vinyl_comment_like,
                player::player_vinyl_comment_report,
                player::player_vinyl_prepare_write,
                player::player_vinyl_write_operations,
                player::player_vinyl_write_command,
                player::quality::player_quality_details,
                player::quality::player_quality_trial,
                player::play_selection,
                player::prepare_play_selection,
                player::commit_prepared_selection,
                player::player_queue,
                player::queue_more_tip,
                audio_diagnostics::system_master_volume,
                audio_diagnostics::system_spatial_sound,
                audio_diagnostics::check_volume_diagnosis,
                audio_diagnostics::dismiss_volume_diagnosis,
                daily_notification::daily_notification,
                daily_notification::daily_notification_command,
                daily_notification::daily_notification_enabled,
                daily_notification::set_daily_notification_enabled,
                player::power::playback_power,
                player::power::set_playback_power,
                player::player_snapshot,
                player::intelligence::player_intelligence,
                player::intelligence::player_intelligence_cancel,
                player::intelligence::player_intelligence_eligibility,
                player::player_queue_collection,
                player::player_queue_collect_rows,
                player::player_queue_like,
                player::player_queue_catalog_menu,
                player::player_lyrics,
                player::reports::lyric_report_open,
                player::reports::lyric_report_read,
                player::reports::lyric_report_types,
                player::reports::lyric_report_submit,
                player::reports::lyric_report_abandon,
                player::lyric_preferences,
                player::lyric_offset,
                playlists::playlist_editor,
                playlists::playlist_tags,
                playlists::playlist_collect_page,
                playlists::preview_external_playlist,
                external_import::external_playlist_import,
                external_import::external_playlist_import_sidebar,
                external_import::external_playlist_import_auth,
                external_import::external_playlist_import_auth_action,
                playlists::playlist_order_view,
                playlists::mutate_playlist,
                playlists::reconcile_playlist,
                playlists::playlist_operations,
                playlist_cover::playlist_cover_tasks,
                playlist_cover::playlist_cover_image,
                playlist_cover::playlist_cover_command,
                playlist_cover::pick_playlist_cover,
                downloads::downloads,
                downloads::download_comment_count,
                downloads::download_song_share_access,
                downloads::download_collect_prepare,
                downloads::download_collect_destinations,
                downloads::download_collect_existing,
                downloads::download_collect_creation,
                downloads::download_collect_liked_destination,
                downloads::download_collect_liked,
                downloads::download_collect_cancel,
                downloads::download_clear_prepare,
                downloads::download_clear_discard,
                downloads::download_command,
                downloads::download_quality_preferences,
                downloads::song_download_state,
                downloads::voice_download_state,
                downloads::play_downloads,
                downloads::pick_download_directory,
                downloads::open_download_directory,
                cloud::cloud_page,
                cloud::cloud_resource,
                cloud::open_cloud_directory,
                cloud::cloud_command,
                cloud::cloud_delete_prepare,
                cloud::cloud_delete_view,
                cloud::cloud_delete_execute,
                cloud::cloud_delete_discard,
                cloud::cloud_upload_removals,
                cloud::cloud_upload_selection,
                cloud::open_cloud_upload_directory,
                cloud::open_cloud_capacity,
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
                messages::counts::watch_message_counts,
                messages::counts::message_count_action,
                messages::counts::message_read_all,
                messages::counts::browser::observe_message_connectivity,
                messages::message_page,
                messages::pending_message_deletions,
                messages::delete_message,
                messages::message_reply,
                comments::comments_page,
                comments::comments_resource,
                comments::comments_like,
                comments::comments_reconcile,
                comments::comments_prepare_write,
                comments::comments_write_operations,
                comments::comments_write_command,
                comments::comments_floor,
                comments::comment_report,
            ];
            handler(invoke)
        })
}

pub fn run() {
    let file_open = native_open::Pending::from_args(std::env::args_os().skip(1));
    let mut context = tauri::generate_context!();
    // An explicit handoff from the shell never depends on initializing its
    // own graphics stack. Renderer diagnostics have no native file request.
    #[cfg(target_os = "linux")]
    let pre_graphics = file_open
        .requested()
        .then(|| native_open::linux::claim_or_exit(context.config(), &file_open));
    if !graphics::bootstrap() {
        return;
    }
    // SAFETY: desktop entry point, before the GUI runtime and worker creation.
    unsafe { prepare_graphics() };
    let launch = lifecycle::LaunchKind::from_args(std::env::args_os().skip(1));
    #[cfg(target_os = "linux")]
    let native_claim = pre_graphics
        .unwrap_or_else(|| native_open::linux::claim_or_exit(context.config(), &file_open));
    launch.prepare(&mut context);
    configure(tauri::Builder::default())
        .setup(move |app| {
            let root = match std::env::var_os("CUMULUS_DATA_DIR") {
                Some(value) => std::path::PathBuf::from(value),
                None => app.path().app_data_dir()?,
            };
            #[cfg(target_os = "linux")]
            let (root, preclaimed, relay_listener) = match native_claim {
                Some(claim) if root.canonicalize()? == claim.root =>
                    (claim.root, Some(claim.lease), claim.listener),
                Some(_) => return Err(std::io::Error::other("application data directory changed").into()),
                None => (root, None, None),
            };
            #[cfg(not(target_os = "linux"))]
            let preclaimed = None;
            let guard = match preclaimed {
                Some(lease) => Ok(lease),
                None => native_open::ProcessGuard::acquire(&root, file_open.requested()),
            };
            match guard {
                Ok(guard) => app.manage(guard),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    eprintln!("Cumulus is already running for this app profile; close it before opening local files.");
                    // The Wry setup-time exit request loses its status
                    // on Linux. No account or playback state exists yet.
                    std::process::exit(2);
                }
                Err(error) => return Err(error.into()),
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
            let client = renewal::configure(client, &profile)
                .with_official_search_experiment()
                .with_official_quality_experiments()
                .with_official_daily_experiment()
                .with_official_external_import_sidebar()
                .with_official_member_welfare();
            let client = client.with_login_device_name(&login_device::name())?;
            let client = configure_native_im(client, &app.path().resource_dir()?);
            let resources = app.path().resource_dir()?;
            app.manage(effect_activation::Host::new(&resources));
            let client = configure_streaming_cache(configure_native_audio(client, &resources, false));
            #[cfg(windows)]
            let client = match playback_keep_awake::DesktopPlaybackPower::start_windows() {
                Ok(host) => {
                    let host = std::sync::Arc::new(host);
                    app.manage(host.clone());
                    client.with_playback_power_sink(host)
                }
                Err(_) => {
                    eprintln!("Cumulus playback power settings are unavailable.");
                    client
                }
            };
            tauri::async_runtime::block_on(client.start_local_library())?;
            app.manage(client);
            start_message_count_polling(app.handle())?;
            app.manage(file_open);
            lifecycle::install(app.handle())?;
            launch.finish(app.handle());
            system_media::install(app.handle());
            shortcuts::install(app.handle());
            desktop_lyrics::start(app.handle());
            renewal::start(app.handle());
            if pc_presentation::start(app.handle()).is_err() {
                eprintln!("Cumulus PC presentation refresh is unavailable.");
            }
            if renewal::start_legacy(app.handle()).is_err() {
                eprintln!("Cumulus scheduled legacy session renewal is unavailable.");
            }
            if member::start_lifecycle(app.handle()).is_err() {
                eprintln!("Cumulus membership refresh is unavailable.");
            }
            #[cfg(target_os = "linux")]
            if let Some(listener) = relay_listener
                && native_open::linux::install(app.handle(), listener).is_err()
            {
                eprintln!("Cumulus native file handoff is unavailable; cold startup continues.");
            }
            Ok(())
        })
        .on_page_load(daily_notification::page_loaded)
        .build(context)
        .expect("could not start Cumulus")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                renewal::stop(app);
                pc_presentation::stop(app);
                daily_notification::stop(app);
            }
            #[cfg(windows)]
            if matches!(event, tauri::RunEvent::Exit)
                && let Some(host) =
                    app.try_state::<std::sync::Arc<playback_keep_awake::DesktopPlaybackPower>>()
                && host.stop().is_err()
            {
                eprintln!("Cumulus could not release playback power state cleanly.");
            }
        });
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
mod autostart;
mod native_open;
