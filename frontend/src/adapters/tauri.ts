import type { NativeLikeChanged } from '../services/nativeLikes';
import { invoke, Channel } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { PlayerUpdate } from '../services/player';
import type { AccountView, AppInfo, CoreClient, CoreFailure } from '../services/contracts';
import { decodeVideoFrame } from '../services/mv';
import type { WindowLifecycleView } from '../services/window';
import type { SystemMediaStatus } from '../services/systemMedia';
import type { ShortcutStatus } from '../services/shortcuts';
import type { SessionRenewalView } from '../services/renewal';

/** The only module that knows how the desktop host transports commands. */
export function createTauriClient(): CoreClient {
  return {
    messageCounts: (epoch) => invoke('message_counts', { epoch }),
    messagePage: (epoch, query) => invoke('message_page', { epoch, query }),
    messageReply: (epoch, action) => invoke('message_reply', { epoch, action }),
    pendingMessageDeletions: (epoch) => invoke('pending_message_deletions', { epoch }),
    deleteMessage: (epoch, receipt, action) => invoke('delete_message', { epoch, receipt, action }),
    artistOffer: (epoch) => invoke('artist_onboarding_offer', { epoch }),
    artistCandidates: (epoch, id, similarTo) =>
      invoke('artist_onboarding_candidates', { epoch, id, similarTo }),
    artistSave: (epoch, id, artists) => invoke('artist_onboarding_save', { epoch, id, artists }),
    artistPreview: (epoch, id) => invoke('artist_onboarding_preview', { epoch, id }),
    artistClose: (epoch, id) => invoke('artist_onboarding_close', { epoch, id }),
    artistResult: (epoch, id, action, queueRevision, quality) =>
      invoke('artist_onboarding_result', { epoch, id, action, queueRevision, quality }),
    searchResults: (epoch, query) => invoke('search_results', { epoch, query }),
    searchReasonPage: (epoch, target) => invoke('search_reason_page', { epoch, target }),
    communityComposerSearch: (epoch, kind, keyword, offset) =>
      invoke('community_composer_search', { epoch, kind, keyword, offset }),
    communityComposerDraft: (epoch, action) =>
      invoke('community_composer_draft', { epoch, action }),
    communityComposerPublish: (epoch, action) =>
      invoke('community_composer_publish', { epoch, action }),
    profilePodcasts: (epoch, uid, query) => invoke('profile_podcasts', { epoch, uid, query }),
    openProfilePodcastManagement: (epoch, uid) =>
      invoke('open_profile_podcast_management', { epoch, uid }),
    profileFollow: (epoch, uid, command) => invoke('profile_follow', { epoch, uid, command }),
    profileBlacklist: (epoch, uid, command) => invoke('profile_blacklist', { epoch, uid, command }),
    profileBlacklistList: (epoch) => invoke('profile_blacklist_list', { epoch }),
    profileReport: (epoch, uid, command) => invoke('profile_report', { epoch, uid, command }),
    profileNotes: (epoch, uid, action) => invoke('profile_notes', { epoch, uid, action }),
    communityTopic: (epoch, name) => invoke('community_topic', { epoch, name }),
    communityTopicNotice: (epoch, topic) => invoke('community_topic_notice', { epoch, topic }),
    communityTopicSlice: (epoch, topic, action) =>
      invoke('community_topic_slice', { epoch, topic, action }),
    communityFeed: (epoch, action) => invoke('community_feed', { epoch, action }),
    communityNewSongNotice: (epoch, mode) => invoke('community_new_song_notice', { epoch, mode }),
    communityDetail: (epoch, detail) => invoke('community_detail', { epoch, detail }),
    communityNoteReport: (epoch, detail, command) =>
      invoke('community_note_report', { epoch, detail, command }),
    communityNoteDelete: (epoch, detail, action) =>
      invoke('community_note_delete', { epoch, detail, action }),
    communityComments: (epoch, detail, offset) =>
      invoke('community_comments', { epoch, detail, offset }),
    coldOfflineOpen: () => invoke('cold_offline_open'),
    coldOfflineNext: () => invoke('cold_offline_next'),
    coldOfflinePlay: (id) => invoke('cold_offline_play', { id }),
    coldOfflineStop: () => invoke('cold_offline_stop'),
    coldOfflineStatus: () => invoke('cold_offline_status'),
    coldOfflineClose: () => invoke('cold_offline_close'),
    copyText: (text) => invoke('copy_text', { text }),
    searchDiscovery: (epoch, query) => invoke('search_discovery', { epoch, query }),
    searchHistory: (epoch, command) => invoke('search_history', { epoch, command }),
    effectCatalogue: (epoch, kind) => invoke('effect_catalogue', { epoch, kind }),
    effectDetail: (epoch, id) => invoke('effect_detail', { epoch, id }),
    effectPeqPending: (epoch) => invoke('effect_peq_pending', { epoch }),
    effectPeqStatus: (epoch, id) => invoke('effect_peq_status', { epoch, id }),
    effectPeqPrepare: (epoch, id) => invoke('effect_peq_prepare', { epoch, id }),
    effectPeqActivate: (epoch, id, attempt) =>
      invoke('effect_peq_activate', { epoch, id, attempt }),
    effectPeqResume: (epoch, id, release) =>
      invoke('effect_peq_activate', { epoch, id, attempt: release, resumeExisting: true }),
    effectPeqPause: (epoch, id, release) => invoke('effect_peq_pause', { epoch, id, release }),
    effectPeqStop: (epoch, id, release, retryUnknown) =>
      invoke('effect_peq_stop', { epoch, id, release, retryUnknown }),
    effectPeqReconcile: (epoch, id) => invoke('effect_peq_reconcile', { epoch, id }),
    effectDeviceBrands: (epoch) => invoke('effect_device_brands', { epoch }),
    effectDeviceModels: (epoch, brand) => invoke('effect_device_models', { epoch, brand }),
    effectDeviceMaterial: (epoch, brand, model) =>
      invoke('effect_device_material', { epoch, brand, model }),
    effectDeviceApply: (epoch, brand, model) =>
      invoke('effect_device_apply', { epoch, brand, model }),
    effectDeviceRemembered: (epoch) => invoke('effect_device_remembered', { epoch }),
    effectDeviceRestore: (epoch, revision) => invoke('effect_device_restore', { epoch, revision }),
    effectDeviceCompare: (epoch, comparison) =>
      invoke('effect_device_compare', { epoch, comparison }),
    effectDeviceClear: (epoch, brand, model, activationId) =>
      invoke('effect_device_clear', { epoch, brand, model, activationId }),
    shortcutSettings: (epoch) => invoke('shortcut_settings', { epoch }),
    shortcutSettingsCommand: (epoch, command) =>
      invoke('shortcut_settings_command', { epoch, command }),
    shortcutStatus: () => invoke('shortcut_status'),
    shortcutCapture: (input, scope) => invoke('shortcut_capture', { input, scope }),
    shortcutAttach: (epoch, ticket) => invoke('shortcut_attach', { epoch, ticket }),
    shortcutDetach: (lease) => invoke('shortcut_detach', { lease }),
    shortcutPress: (press) => invoke('shortcut_press', { press }),
    shortcutBeginRecording: (epoch) => invoke('shortcut_begin_recording', { epoch }),
    shortcutEndRecording: (id) => invoke('shortcut_end_recording', { id }),
    onShortcuts: (handler) =>
      listen<ShortcutStatus>('cumulus:shortcuts', (event) => handler(event.payload)),
    onShortcutError: (handler) =>
      listen<{ epoch: string; error: CoreFailure }>('cumulus:shortcut-error', (event) =>
        handler(event.payload),
      ),
    systemMediaPreferences: (epoch) => invoke('system_media_preferences', { epoch }),
    systemMediaPreferencesCommand: (epoch, command) =>
      invoke('system_media_preferences_command', { epoch, command }),
    systemMediaStatus: () => invoke('system_media_status'),
    onSystemMediaStatus: (handler) =>
      listen<SystemMediaStatus>('cumulus:system-media', (event) => handler(event.payload)),
    windowPreferences: (epoch) => invoke('window_preferences', { epoch }),
    windowPreferencesCommand: (epoch, command) =>
      invoke('window_preferences_command', { epoch, command }),
    windowLifecycle: () => invoke('window_lifecycle'),
    windowCloseCommand: (command) => invoke('window_close_command', { command }),
    onWindowLifecycle: (handler) =>
      listen<WindowLifecycleView>('cumulus:window-lifecycle', (event) => handler(event.payload)),
    outputSettings: (epoch, refresh) => invoke('output_settings', { epoch, refresh }),
    outputCommand: (epoch, command) => invoke('output_command', { epoch, command }),
    queueClickPreferences: (epoch) => invoke('queue_click_preferences', { epoch }),
    queueClickPreferencesCommand: (epoch, command) =>
      invoke('queue_click_preferences_command', { epoch, command }),
    startupPreferences: (epoch) => invoke('startup_preferences', { epoch }),
    startupPreferencesCommand: (epoch, command) =>
      invoke('startup_preferences_command', { epoch, command }),
    sessionRenewal: (epoch) => invoke('session_renewal', { epoch }),
    renewSession: (epoch, action) => invoke('renew_session', { epoch, action }),
    onSessionRenewal: (handler) =>
      listen<SessionRenewalView>('cumulus:session-renewal', (event) => handler(event.payload)),
    playlistCoverTasks: (epoch) => invoke('playlist_cover_tasks', { epoch }),
    playlistCoverImage: (epoch, id) => invoke('playlist_cover_image', { epoch, id }),
    pickPlaylistCover: (epoch, playlistId, revision, replaceId) =>
      invoke('pick_playlist_cover', { epoch, playlistId, revision, replaceId }),
    playlistCoverCommand: (epoch, id, command) =>
      invoke('playlist_cover_command', { epoch, id, command }),
    memberPage: (epoch, query, refresh) => invoke('member_page', { epoch, query, refresh }),
    memberRights: (epoch, refresh) => invoke('member_rights', { epoch, refresh }),
    memberWelfareAvailable: (epoch) => invoke('member_welfare_available', { epoch }),
    memberWelfareList: (epoch) => invoke('member_welfare_list', { epoch }),
    remoteSkinCatalog: (epoch) => invoke('remote_skin_catalog', { epoch }),
    remoteSkinImage: (epoch, revision, key, kind) =>
      invoke('remote_skin_image', { epoch, revision, key, kind }),
    memberPortalQr: () => invoke('member_portal_qr'),
    memberCardOfferQr: (epoch, revision) => invoke('member_card_offer_qr', { epoch, revision }),
    memberCardOffer: (epoch, refresh) => invoke('member_card_offer', { epoch, refresh }),
    openMemberCardOffer: (epoch, revision) => invoke('open_member_card_offer', { epoch, revision }),
    memberSignCard: (epoch) => invoke('member_sign_card', { epoch }),
    memberSignDetail: (epoch, signTime) => invoke('member_sign_detail', { epoch, signTime }),
    memberSignCommand: (epoch, command) => invoke('member_sign_command', { epoch, command }),
    openMemberLink: (epoch, revision, key) => invoke('open_member_link', { epoch, revision, key }),
    fmView: (epoch, source, refresh) => invoke('fm_view', { epoch, source, refresh }),
    fmDiscovery: (epoch, query, refresh) => invoke('fm_discovery', { epoch, query, refresh }),
    fmReasons: (epoch, source, entryId) => invoke('fm_reasons', { epoch, source, entryId }),
    fmCommand: (epoch, source, command) => invoke('fm_command', { epoch, source, command }),
    fmTrash: (epoch, page, cursor) => invoke('fm_trash', { epoch, page, cursor }),
    rankingPage: (epoch, query, refresh) => invoke('ranking_page', { epoch, query, refresh }),
    catalogPage: (epoch, query, refresh) => invoke('catalog_page', { epoch, query, refresh }),
    ordinaryEssential: (epoch, category) => invoke('ordinary_essential', { epoch, category }),
    ordinaryEssentialBooks: (epoch, category) =>
      invoke('ordinary_essential_books', { epoch, category }),
    discoveryHome: (epoch) => invoke('discovery_home', { epoch }),
    discoveryHomeRankings: (epoch) => invoke('discovery_home_rankings', { epoch }),
    artistGallery: (epoch, query) => invoke('artist_gallery', { epoch, query }),
    latestReleases: (epoch, query) => invoke('latest_releases', { epoch, query }),
    followedNewSongPage: (epoch, startTimestamp) =>
      invoke('followed_new_song_page', { epoch, startTimestamp }),
    followedNewSongLatest: (epoch) => invoke('followed_new_song_latest', { epoch }),
    releaseNotification: (epoch, artist, action) =>
      invoke('release_notification', { epoch, artist, action }),
    collectionPage: (epoch, query, refresh) => invoke('collection_page', { epoch, query, refresh }),
    historyPage: (epoch, kind, refresh) => invoke('history_page', { epoch, kind, refresh }),
    historySyncPreference: (epoch, action) => invoke('history_sync_preference', { epoch, action }),
    accountSettingsPage: (epoch, refresh) => invoke('account_settings_page', { epoch, refresh }),
    accountProfileDetail: (epoch) => invoke('account_profile_detail', { epoch }),
    accountAvatarTasks: (epoch) => invoke('account_avatar_tasks', { epoch }),
    accountAvatarImage: (epoch, id) => invoke('account_avatar_image', { epoch, id }),
    pickAccountAvatar: (epoch) => invoke('pick_account_avatar', { epoch }),
    accountAvatarCommand: (epoch, id, command) =>
      invoke('account_avatar_command', { epoch, id, command }),
    accountProfileRegions: (epoch) => invoke('account_profile_regions', { epoch }),
    accountProfileEditPage: (epoch, refresh) =>
      invoke('account_profile_edit_page', { epoch, refresh }),
    accountProfileEditCommand: (epoch, command) =>
      invoke('account_profile_edit_command', { epoch, command }),
    accountBindingQr: (epoch) => invoke('account_binding_qr', { epoch }),
    accountLoginDevices: (epoch) => invoke('account_login_devices', { epoch }),
    accountLoginDeviceRemoval: (epoch) => invoke('account_login_device_removal', { epoch }),
    accountLoginDeviceRemovalCommand: (epoch, command) =>
      invoke('account_login_device_removal_command', { epoch, command }),
    relayView: (epoch, refresh) => invoke('relay_view', { epoch, refresh }),
    relayPublicationView: (epoch, requested) =>
      invoke('relay_publication_view', { epoch, requested }),
    relayPublicationCommand: (epoch, command) =>
      invoke('relay_publication_command', { epoch, command }),
    relayCommand: (epoch, command) => invoke('relay_command', { epoch, command }),
    relaySettingCommand: (epoch, command) => invoke('relay_setting_command', { epoch, command }),
    accountSettingsCommand: (epoch, command) =>
      invoke('account_settings_command', { epoch, command }),
    historyCommand: (epoch, kind, command) => invoke('history_command', { epoch, kind, command }),
    catalogMembership: (epoch, resource, action) =>
      invoke('catalog_membership', { epoch, resource, action }),
    onNativeLikeChanged: (handler) =>
      listen<NativeLikeChanged>('cumulus:native-like-changed', (event) => handler(event.payload)),
    onAccountChanged: (handler) =>
      listen<AccountView>('cumulus:account-changed', (event) => handler(event.payload)),
    loginPresentation: () => invoke('login_presentation'),
    selectLoginCountry: (alpha) => invoke('select_login_country', { alpha }),
    openLoginDocument: (document) => invoke('open_login_document', { document }),
    beginLogin: (epoch, mode, savedId) =>
      invoke('begin_login', { epoch, mode, savedId: savedId ?? null }),
    loginSnapshot: (operationId) => invoke('login_snapshot', { operationId }),
    solveLoginCaptcha: (operationId) => invoke('solve_login_captcha', { operationId }),
    loginCommand: (operationId, command) => invoke('login_command', { operationId, command }),
    cancelLogin: (operationId) => invoke('cancel_login', { operationId }),
    savedAccounts: (epoch) => invoke('saved_accounts', { epoch }),
    quickLoginEligibility: (epoch) => invoke('quick_login_eligibility', { epoch }),
    beginServerQuickLogin: (epoch, savedId, consent) =>
      invoke('begin_server_quick_login', { epoch, savedId, consent }),
    serverQuickLoginCommand: (epoch, id, command) =>
      invoke('server_quick_login_command', { epoch, id, command }),
    beginActiveServerSwitch: (epoch, savedId, consent) =>
      invoke('begin_active_server_switch', { epoch, savedId, consent }),
    activeServerSwitchCommand: (epoch, id, command) =>
      invoke('active_server_switch_command', { epoch, id, command }),
    removeSavedAccount: (epoch, id) => invoke('remove_saved_account', { epoch, id }),
    logoutAccount: (epoch, remote) => invoke('logout_account', { epoch, remote }),
    onTogetherCloseError: (epoch, handler) =>
      listen<{ epoch: string; message: string }>('cumulus:together-close-error', (event) => {
        if (event.payload.epoch === epoch) handler(event.payload.message);
      }),
    togetherView: (epoch, refresh) => invoke('together_view', { epoch, refresh }),
    togetherStatistics: (epoch, relationKey) =>
      invoke('together_statistics', { epoch, relationKey }),
    togetherCommand: (epoch, command) => invoke('together_command', { epoch, command }),
    podcastPage: (epoch, query, refresh) => invoke('podcast_page', { epoch, query, refresh }),
    podcastRecent: (epoch, query) => invoke('podcast_recent', { epoch, query }),
    podcastCatalogue: (epoch, query) => invoke('podcast_catalogue', { epoch, query }),
    podcastHome: (epoch, continuation) => invoke('podcast_home', { epoch, continuation }),
    podcastRankings: (epoch, query) => invoke('podcast_rankings', { epoch, query }),
    sceneFmView: (epoch, refresh) => invoke('scene_fm_view', { epoch, refresh }),
    sceneFmCommand: (epoch, command) => invoke('scene_fm_command', { epoch, command }),
    podcastState: (epoch, refresh) => invoke('podcast_state', { epoch, refresh }),
    podcastSearchMembership: (epoch) => invoke('podcast_search_membership', { epoch }),
    podcastCommand: (epoch, command) => invoke('podcast_command', { epoch, command }),
    localPage: (query) => invoke('local_page', { query }),
    localCommand: (command) => invoke('local_command', { command }),
    pickLocalDirectory: () => invoke('pick_local_directory'),
    openLocalTrackDirectory: (id, revision) =>
      invoke('open_local_track_directory', { id, revision }),
    localMatchView: (epoch, ids) => invoke('local_match_view', { epoch, ids }),
    localMatchCommand: (epoch, command) => invoke('local_match_command', { epoch, command }),
    mvPage: (epoch, query, refresh) => invoke('mv_page', { epoch, query, refresh }),
    mvDetail: (epoch, id) => invoke('mv_detail', { epoch, id }),
    mvCollect: (epoch, id, desired) => invoke('mv_collect', { epoch, id, desired }),
    mvReconcile: (epoch, id) => invoke('mv_reconcile', { epoch, id }),
    mvLike: (epoch, id, desired) => invoke('mv_like', { epoch, id, desired }),
    mvLikeReconcile: (epoch, id) => invoke('mv_like_reconcile', { epoch, id }),
    mvRecommendations: (epoch, id, refresh) => invoke('mv_recommendations', { epoch, id, refresh }),
    videoSnapshot: (epoch) => invoke('video_snapshot', { epoch }),
    videoAction: (epoch, action) => invoke('video_action', { epoch, action }),
    async videoFrame(epoch, playId, after, width, height) {
      const bytes = await invoke<ArrayBuffer>('video_frame', {
        epoch,
        playId,
        after,
        width,
        height,
      });
      return decodeVideoFrame(bytes);
    },
    cloudPage: (epoch, query, refresh) => invoke('cloud_page', { epoch, query, refresh }),
    cloudDeletePrepare: (epoch, ids, revision) =>
      invoke('cloud_delete_prepare', { epoch, ids, revision }),
    cloudDeleteView: (epoch, id, reconcile) =>
      invoke('cloud_delete_view', { epoch, id, reconcile }),
    cloudDeleteExecute: (epoch, id, deleteLocal, retryLocal) =>
      invoke('cloud_delete_execute', { epoch, id, deleteLocal, retryLocal }),
    cloudDeleteDiscard: (epoch, id) => invoke('cloud_delete_discard', { epoch, id }),
    cloudResource: (epoch, songId, revision) =>
      invoke('cloud_resource', { epoch, songId, revision }),
    openCloudDirectory: (epoch, songId, revision) =>
      invoke('open_cloud_directory', { epoch, songId, revision }),
    commentReport: (epoch, resource, id, command) =>
      invoke('comment_report', { epoch, resource, id, command }),
    commentsPage: (epoch, resource, query, refresh) =>
      invoke('comments_page', { epoch, resource, query, refresh }),
    commentsResource: (epoch, resource, refresh) =>
      invoke('comments_resource', { epoch, resource, refresh }),
    commentsLike: (epoch, resource, id, desired) =>
      invoke('comments_like', { epoch, resource, id, desired }),
    commentsReconcile: (epoch, resource, id) =>
      invoke('comments_reconcile', { epoch, resource, id }),
    commentsPrepareWrite: (epoch, resource, command) =>
      invoke('comments_prepare_write', { epoch, resource, command }),
    commentsWriteOperations: (epoch, resource) =>
      invoke('comments_write_operations', { epoch, resource }),
    commentsWriteCommand: (epoch, id, command) =>
      invoke('comments_write_command', { epoch, id, command }),
    commentsFloor: (epoch, resource, query, refresh) =>
      invoke('comments_floor', { epoch, resource, query, refresh }),
    openCloudCapacity: (epoch) => invoke('open_cloud_capacity', { epoch }),
    cloudUploadSelection: (epoch, history) => invoke('cloud_upload_selection', { epoch, history }),
    cloudUploadRemovals: (epoch, ids) => invoke('cloud_upload_removals', { epoch, ids }),
    openCloudUploadDirectory: (epoch, id) => invoke('open_cloud_upload_directory', { epoch, id }),
    cloudCommand: (epoch, command) => invoke('cloud_command', { epoch, command }),
    pickCloudFiles: (epoch, kind) => invoke('pick_cloud_files', { epoch, kind }),
    downloads: (epoch, query) => invoke('downloads', { epoch, query }),
    downloadCommand: (epoch, command) => invoke('download_command', { epoch, command }),
    songDownloadState: (epoch, id) => invoke('song_download_state', { epoch, id }),
    voiceDownloadState: (epoch, id) => invoke('voice_download_state', { epoch, id }),
    playDownloads: (epoch, taskIds, index) => invoke('play_downloads', { epoch, taskIds, index }),
    pickDownloadDirectory: (epoch) => invoke('pick_download_directory', { epoch }),
    openDownloadDirectory: (epoch, taskId) => invoke('open_download_directory', { epoch, taskId }),
    playlistTags: (epoch) => invoke('playlist_tags', { epoch }),
    playlistEditor: (epoch, id) => invoke('playlist_editor', { epoch, id }),
    previewExternalPlaylist: (inputs) => invoke('preview_external_playlist', { inputs }),
    externalPlaylistImportSidebar: (epoch) => invoke('external_playlist_import_sidebar', { epoch }),
    externalPlaylistImport: (epoch, action) =>
      invoke('external_playlist_import', { epoch, action }),
    externalPlaylistImportAuth: (epoch) => invoke('external_playlist_import_auth', { epoch }),
    externalPlaylistImportAuthAction: (epoch, action) =>
      invoke('external_playlist_import_auth_action', { epoch, action }),
    playlistOrderView: (epoch) => invoke('playlist_order_view', { epoch }),
    mutatePlaylist: (epoch, command) => invoke('mutate_playlist', { epoch, command }),
    reconcilePlaylist: (epoch, operationId) => invoke('reconcile_playlist', { epoch, operationId }),
    playlistOperations: (epoch) => invoke('playlist_operations', { epoch }),
    async watchPlayer(epoch, update) {
      const streamId = crypto.randomUUID();
      const onUpdate = new Channel<PlayerUpdate>();
      let active = true;
      onUpdate.onmessage = (event) => {
        if (active) update(event);
      };
      await invoke('watch_player', { epoch, streamId, onUpdate });
      return () => {
        active = false;
        void invoke('unwatch_player', { streamId }).catch(() => {});
      };
    },
    playSelection: (epoch, selection) => invoke('play_selection', { epoch, selection }),
    preparePlaySelection: (epoch, expectedQueueRevision, expectedPlayId) =>
      invoke('prepare_play_selection', { epoch, expectedQueueRevision, expectedPlayId }),
    commitPreparedSelection: (epoch, ticket, selection) =>
      invoke('commit_prepared_selection', { epoch, ticket, selection }),
    playerAction: (epoch, action) => invoke('player_action', { epoch, action }),
    playerQualityDetails: (epoch, playId) => invoke('player_quality_details', { epoch, playId }),
    playerQualityTrial: (epoch, command) => invoke('player_quality_trial', { epoch, command }),
    playerIntelligence: (epoch, command) => invoke('player_intelligence', { epoch, command }),
    playerIntelligenceCancel: (epoch, operationId) =>
      invoke('player_intelligence_cancel', { epoch, operationId }),
    playerIntelligenceEligibility: (epoch, revision) =>
      invoke('player_intelligence_eligibility', { epoch, revision }),
    playerSnapshot: (epoch) => invoke('player_snapshot', { epoch }),
    playerQueue: (epoch) => invoke('player_queue', { epoch }),
    playerQueueCollection: (epoch, revision) =>
      invoke('player_queue_collection', { epoch, revision }),
    playerChorus: (epoch, playId) => invoke('player_chorus', { epoch, playId }),
    playerCommentCount: (epoch, playId) => invoke('player_comment_count', { epoch, playId }),
    playerLyrics: (epoch, playId, cachedOnly) =>
      invoke('player_lyrics', { epoch, playId, cachedOnly }),
    lyricPreferences: (epoch, playId, preferences) =>
      invoke('lyric_preferences', { epoch, playId, preferences }),
    lyricOffset: (epoch, playId, gain) => invoke('lyric_offset', { epoch, playId, gain }),
    accountSnapshot: () => invoke('account_snapshot'),
    restoreAccount: () => invoke('restore_account'),
    beginQr: () => invoke('begin_qr'),
    pollQr: (operationId) => invoke('poll_qr', { operationId }),
    qrIdentityPending: (operationId) => invoke('qr_identity_pending', { operationId }),
    reconcileQr: (operationId) => invoke('reconcile_qr', { operationId }),
    cancelQr: (operationId) => invoke('cancel_qr', { operationId }),
    playlists: () => invoke('account_playlists'),
    playlistCollectPage: (epoch, sort, page) =>
      invoke('playlist_collect_page', { epoch, sort, page }),
    profilePlayRecord: (epoch, uid, period) =>
      invoke('profile_play_record', { epoch, uid, period }),
    removeProfilePlayRecord: (epoch, period, songId, revision, action) =>
      invoke('remove_profile_play_record', { epoch, period, songId, revision, action }),
    playlistCreatorProfile: (epoch, uid) => invoke('playlist_creator_profile', { epoch, uid }),
    profileRelationAccess: (epoch, uid) => invoke('profile_relation_access', { epoch, uid }),
    profileRelationPage: (epoch, uid, kind, continuation) =>
      invoke('profile_relation_page', { epoch, uid, kind, continuation }),
    playlistTagPage: (epoch, query) => invoke('playlist_tag_page', { epoch, query }),
    playlistTagCatalogue: (epoch) => invoke('playlist_tag_catalogue', { epoch }),
    playlistTopTags: (epoch) => invoke('playlist_top_tags', { epoch }),
    playlistEssentialCategories: (epoch) => invoke('playlist_essential_categories', { epoch }),
    playlistEssentialPage: (epoch, category, continuation) =>
      invoke('playlist_essential_page', { epoch, category, continuation }),
    playlistSquareRecommendations: (epoch, continuation) =>
      invoke('playlist_square_recommendations', { epoch, continuation }),
    playlistSubscribers: (epoch, id, offset) =>
      invoke('playlist_subscribers', { epoch, id, offset }),
    playlistPage: (id, page) => invoke('playlist_page', { id, page }),
    searchSongs: (keyword, page) => invoke('search_songs', { keyword, page }),
    dailySongs: () => invoke('daily_songs'),
    styleRecommendations: (epoch) => invoke('style_recommendations', { epoch }),
    styleHomeSeeds: (epoch) => invoke('style_home_seeds', { epoch }),
    seededStyleRecommendations: (epoch, seed) =>
      invoke('seeded_style_recommendations', { epoch, seed }),
    styleRecommendationCategories: (epoch) => invoke('style_recommendation_categories', { epoch }),
    styleTagOperations: (epoch, id) => invoke('style_tag_operations', { epoch, id }),
    saveStyleTag: (epoch, id, expectedRevision, categoryId, tagId) =>
      invoke('save_style_tag', { epoch, id, expectedRevision, categoryId, tagId }),
    reconcileStyleTag: (epoch, id) => invoke('reconcile_style_tag', { epoch, id }),
    acceptCurrentStyleTag: (epoch, id, observedRevision) =>
      invoke('accept_current_style_tag', { epoch, id, observedRevision }),
    likedIds: () => invoke('liked_ids'),
    setLiked: (epoch, id, liked) => invoke('set_liked', { epoch, id, liked }),
    likeRecovery: (epoch, offset) => invoke('like_recovery', { epoch, offset }),
    recoverLike: (epoch, command) => invoke('recover_like', { epoch, command }),
    localNetworkDiagnostics: () => invoke('local_network_diagnostics'),
    checkForUpdates: () => invoke('check_for_updates'),
    openUpdateReleasePage: () => invoke('open_update_release_page'),
    verifyUpdatePackage: () => invoke('verify_update_package'),
    async getAppInfo(): Promise<AppInfo> {
      const info: unknown = await invoke('get_app_info');
      if (
        typeof info !== 'object' ||
        info === null ||
        !('name' in info) ||
        typeof info.name !== 'string' ||
        !('coreVersion' in info) ||
        typeof info.coreVersion !== 'string' ||
        !('protocolVersion' in info) ||
        info.protocolVersion !== 1
      ) {
        throw new Error('Unsupported core protocol');
      }
      return {
        name: info.name,
        coreVersion: info.coreVersion,
        protocolVersion: info.protocolVersion,
      };
    },
  };
}
