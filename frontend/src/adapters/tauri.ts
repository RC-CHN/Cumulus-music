import { invoke, Channel } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { PlayerUpdate } from '../services/player';
import type { AccountView, AppInfo, CoreClient } from '../services/contracts';
import { decodeVideoFrame } from '../services/mv';
import type { SessionRenewalView } from '../services/renewal';

/** The only module that knows how the desktop host transports commands. */
export function createTauriClient(): CoreClient {
  return {
    outputSettings: (epoch, refresh) => invoke('output_settings', { epoch, refresh }),
    outputCommand: (epoch, command) => invoke('output_command', { epoch, command }),
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
    openMemberLink: (epoch, revision, key) => invoke('open_member_link', { epoch, revision, key }),
    fmView: (epoch, source, refresh) => invoke('fm_view', { epoch, source, refresh }),
    fmDiscovery: (epoch, query, refresh) => invoke('fm_discovery', { epoch, query, refresh }),
    fmReasons: (epoch, source, entryId) => invoke('fm_reasons', { epoch, source, entryId }),
    fmCommand: (epoch, source, command) => invoke('fm_command', { epoch, source, command }),
    fmTrash: (epoch, page, cursor) => invoke('fm_trash', { epoch, page, cursor }),
    rankingPage: (epoch, query, refresh) => invoke('ranking_page', { epoch, query, refresh }),
    catalogPage: (epoch, query, refresh) => invoke('catalog_page', { epoch, query, refresh }),
    collectionPage: (epoch, query, refresh) => invoke('collection_page', { epoch, query, refresh }),
    historyPage: (epoch, kind, refresh) => invoke('history_page', { epoch, kind, refresh }),
    accountSettingsPage: (epoch, refresh) => invoke('account_settings_page', { epoch, refresh }),
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
    onAccountChanged: (handler) =>
      listen<AccountView>('cumulus:account-changed', (event) => handler(event.payload)),
    beginLogin: (epoch, mode, savedId) =>
      invoke('begin_login', { epoch, mode, savedId: savedId ?? null }),
    loginSnapshot: (operationId) => invoke('login_snapshot', { operationId }),
    solveLoginCaptcha: (operationId) => invoke('solve_login_captcha', { operationId }),
    loginCommand: (operationId, command) => invoke('login_command', { operationId, command }),
    cancelLogin: (operationId) => invoke('cancel_login', { operationId }),
    savedAccounts: (epoch) => invoke('saved_accounts', { epoch }),
    removeSavedAccount: (epoch, id) => invoke('remove_saved_account', { epoch, id }),
    logoutAccount: (epoch, remote) => invoke('logout_account', { epoch, remote }),
    onTogetherCloseError: (epoch, handler) =>
      listen<{ epoch: string; message: string }>('cumulus:together-close-error', (event) => {
        if (event.payload.epoch === epoch) handler(event.payload.message);
      }),
    togetherView: (epoch, refresh) => invoke('together_view', { epoch, refresh }),
    togetherCommand: (epoch, command) => invoke('together_command', { epoch, command }),
    podcastPage: (epoch, query, refresh) => invoke('podcast_page', { epoch, query, refresh }),
    sceneFmView: (epoch, refresh) => invoke('scene_fm_view', { epoch, refresh }),
    sceneFmCommand: (epoch, command) => invoke('scene_fm_command', { epoch, command }),
    podcastState: (epoch, refresh) => invoke('podcast_state', { epoch, refresh }),
    podcastCommand: (epoch, command) => invoke('podcast_command', { epoch, command }),
    localPage: (query) => invoke('local_page', { query }),
    localCommand: (command) => invoke('local_command', { command }),
    pickLocalDirectory: () => invoke('pick_local_directory'),
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
    commentsPage: (epoch, resource, query, refresh) =>
      invoke('comments_page', { epoch, resource, query, refresh }),
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
    cloudCommand: (epoch, command) => invoke('cloud_command', { epoch, command }),
    pickCloudFiles: (epoch, kind) => invoke('pick_cloud_files', { epoch, kind }),
    downloads: (epoch, query) => invoke('downloads', { epoch, query }),
    downloadCommand: (epoch, command) => invoke('download_command', { epoch, command }),
    playDownloads: (epoch, taskIds, index) => invoke('play_downloads', { epoch, taskIds, index }),
    pickDownloadDirectory: (epoch) => invoke('pick_download_directory', { epoch }),
    openDownloadDirectory: (epoch, taskId) => invoke('open_download_directory', { epoch, taskId }),
    playlistTags: (epoch) => invoke('playlist_tags', { epoch }),
    playlistEditor: (epoch, id) => invoke('playlist_editor', { epoch, id }),
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
    playerAction: (epoch, action) => invoke('player_action', { epoch, action }),
    playerQueue: (epoch) => invoke('player_queue', { epoch }),
    playerLyrics: (epoch, playId) => invoke('player_lyrics', { epoch, playId }),
    lyricPreferences: (epoch, playId, preferences) =>
      invoke('lyric_preferences', { epoch, playId, preferences }),
    lyricOffset: (epoch, playId, gain) => invoke('lyric_offset', { epoch, playId, gain }),
    accountSnapshot: () => invoke('account_snapshot'),
    restoreAccount: () => invoke('restore_account'),
    beginQr: () => invoke('begin_qr'),
    pollQr: (operationId) => invoke('poll_qr', { operationId }),
    cancelQr: (operationId) => invoke('cancel_qr', { operationId }),
    playlists: () => invoke('account_playlists'),
    playlistPage: (id, page) => invoke('playlist_page', { id, page }),
    searchSongs: (keyword, page) => invoke('search_songs', { keyword, page }),
    dailySongs: () => invoke('daily_songs'),
    likedIds: () => invoke('liked_ids'),
    setLiked: (epoch, id, liked) => invoke('set_liked', { epoch, id, liked }),
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
