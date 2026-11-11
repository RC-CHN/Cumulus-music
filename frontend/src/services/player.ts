import type { TrackView, CoreFailure, AccountData, LikeResult } from './contracts';
import type { CloudQuery } from './cloud';
import type { CommentOperation, VinylCommentLikeAction, VinylCommentPage } from './comments';
import type { ProfileReportView, ReportCommand } from './profileReport';
import type { VinylSimilarClient } from './vinylSimilar';
import type { VoiceRecommendationsClient } from './voiceRecommendations';
import type { VoiceDetailClient } from './voicePage';
import type { VoiceSongsClient } from './voiceSongs';
import type { VoiceDetail } from './voiceDetail';
export type Quality =
  | '64aac'
  | 'standard'
  | 'higher'
  | 'exhigh'
  | 'lossless'
  | 'hires'
  | 'dolby'
  | 'jyeffect'
  | 'jymaster'
  | 'sky'
  | 'vivid';
export const qualities: { value: Quality; label: string }[] = [
  { value: 'standard', label: '标准' },
  { value: 'higher', label: '较高' },
  { value: 'exhigh', label: '极高' },
  { value: 'lossless', label: '无损' },
  { value: 'hires', label: 'Hi-Res' },
  { value: 'dolby', label: '杜比全景声' },
  { value: 'jyeffect', label: '高清臻音' },
  { value: 'jymaster', label: '超清母带' },
  { value: 'sky', label: '沉浸环绕声' },
  { value: 'vivid', label: '臻音全景声' },
  { value: '64aac', label: '节省流量' },
];
export type PlaybackMode = 'order_loop' | 'single_loop' | 'random' | 'sequential';
export type PlaybackSpeedScope = 'song' | 'voice';
export type SongSpeedSelection = { kind: 'preset' | 'custom'; tenths: number };
export interface SongSpeedView {
  enabled: boolean;
  revision: string;
  customTenths: number;
  customSelected: boolean;
  syncFailed: boolean;
}
export type PlaybackStatus =
  | 'idle'
  | 'preparing'
  | 'loading'
  | 'playing'
  | 'paused'
  | 'buffering'
  | 'ended'
  | 'error';
export interface Timeline {
  mode: 'full' | 'full_trial' | 'cue' | 'trial';
  start: number;
  end: number | null;
  length: number | null;
  total: number | null;
  media_start: number;
}
export type BoundParametricStatus =
  | 'deferred'
  | 'configured'
  | 'bypassed_for_source'
  | 'detach_required'
  | 'processor_failed';

export interface DeviceParametricView {
  brand: number;
  model: number;
  activationId: string;
  modelName?: string;
  status: BoundParametricStatus | 'paused_locally';
  comparison?: { playId: string; receipt: string };
  selectionSaveFailed?: boolean;
}

export interface PlayerView {
  voiceDetail?: VoiceDetail | null;
  resourceKind?: 'catalog' | 'cloud' | 'local' | 'downloaded' | 'voice' | null;
  audiobookSkipAvailable?: boolean;
  origin?: PlayerOrigin | null;
  originName?: string;
  vinylSimilarOrigin?: boolean;
  searchOrigin?: boolean;
  profileRankPlaylistUid?: string;
  recommendationOrigin?: { kind: 'daily' | 'style'; revision: string } | null;
  fm: boolean;
  zoneFmRuleId?: string;
  intelligence?: 'seeded' | 'active' | 'fallback' | null;
  intelligenceOperation?: string | null;
  equalizer: EqualizerView;
  boundParametric?: BoundParametricStatus;
  appliedEffectId?: number;
  parametricProcessing?: boolean;
  deviceParametric?: DeviceParametricView;
  loudness: LoudnessView;
  epoch: string;
  revision: string;
  queueRevision: string;
  queueMoreAvailable?: boolean;
  queueMoreTipEligible?: boolean;
  state: PlaybackStatus;
  pendingPaused?: boolean;
  playId: string | null;
  endedMinibarReceipt?: string;
  entryId: string | null;
  downloadTaskId?: string;
  track: TrackView | null;
  titleAlias?: string;
  position: number | null;
  bufferedThrough?: number | null;
  nativeSeeking?: boolean;
  timeline: Timeline | null;
  qualityPreferences: {
    revision: string | null;
    quality: Quality;
    damaged: boolean;
    appliesToCurrent: boolean;
  };
  canChangeQuality: boolean;
  catalogFileSource?: boolean;
  requestedQuality: Quality | null;
  actualQuality: Quality | null;
  volume: number;
  unmuteVolume: number;
  speed: number;
  speedScope?: PlaybackSpeedScope;
  songSpeed?: SongSpeedView;
  mode: PlaybackMode;
  queueLen: number;
  error: string | null;
  errorCode?: string | null;
  errorRetryable?: boolean | null;
  historyWarning: string | null;
  positionReset: boolean;
}

export interface CurrentCatalogTrack {
  epoch: string;
  playId: string;
  track: TrackView;
}

/** Current catalogue controls cannot mistake another resource namespace for a song. */
export function currentCatalogTrack(view?: PlayerView): CurrentCatalogTrack | undefined {
  const track = view?.track;
  const playId = view?.playId;
  if (
    view?.resourceKind !== 'catalog' ||
    !playId ||
    !track ||
    track.local ||
    track.missing ||
    track.voiceId
  )
    return undefined;
  return { epoch: view.epoch, playId, track };
}

export interface CurrentPlaybackDownload {
  epoch: string;
  playId: string;
  kind: 'song' | 'voice';
  id: string;
  name: string;
}

export function currentPlaybackDownload(view?: PlayerView): CurrentPlaybackDownload | undefined {
  const song = currentCatalogTrack(view);
  if (song)
    return {
      epoch: song.epoch,
      playId: song.playId,
      kind: 'song',
      id: song.track.id,
      name: song.track.name,
    };
  const track = view?.track;
  if (
    view?.resourceKind !== 'voice' ||
    !view.playId ||
    !track?.voiceId ||
    track.local ||
    track.missing
  )
    return undefined;
  return {
    epoch: view.epoch,
    playId: view.playId,
    kind: 'voice',
    id: track.voiceId,
    name: track.name,
  };
}

export interface PlayerOrigin {
  kind: 'playlist' | 'album' | 'artist' | 'podcast';
  id: string;
}
export interface LoudnessView {
  revision: string | null;
  enabled: boolean;
  status:
    | 'off'
    | 'no_track'
    | 'no_gain'
    | 'invalid_gain'
    | 'pending'
    | 'configured'
    | 'processing'
    | 'unavailable'
    | 'failed';
  gainDb: number | null;
  damaged: boolean;
}
export interface EqualizerSettings {
  enabled: boolean;
  gains: number[];
  bass_db?: number;
  treble_db?: number;
  presence?: number;
  stereoizer?: number;
  tone_enabled?: boolean;
  master_bypassed?: boolean;
}
export type EqualizerPreset =
  | { kind: 'built_in'; index: number }
  | { kind: 'manual' }
  | { kind: 'saved' };
export type EqualizerSpatialStatus =
  | 'off'
  | 'pending'
  | 'unavailable'
  | 'configured'
  | 'processing'
  | 'bypassed'
  | 'failed';
export interface EqualizerView {
  revision: string;
  settings: EqualizerSettings;
  preset?: EqualizerPreset | null;
  saved_custom?: number[] | null;
  status: 'pending' | 'unavailable' | 'configured' | 'processing' | 'failed';
  spatial_status?: EqualizerSpatialStatus;
  damaged: boolean;
  source_bypassed?: boolean;
}
export type QueueOrigin =
  | { kind: 'album' | 'artist' | 'playlist'; id: string }
  | { kind: 'local' | 'cloud' | 'downloads' };
export interface QueueTrack {
  entryId: string;
  track: TrackView;
  moreRecommendation?: boolean;
  origin?: QueueOrigin;
  sourceLabel?: string;
  likeSongId?: string;
  catalogMenu?: boolean;
  voiceMenu?: boolean;
  localMenu?: boolean;
  downloadMenu?: 'song' | 'voice';
  collectible?: boolean;
}
export interface QueueCatalogMenu {
  epoch: string;
  revision: string;
  entryId: string;
  track: TrackView;
  sourceLabel: string;
  origin: QueueOrigin | null;
  commentCount: number | null;
  downloaded: boolean;
  directoryReceipt: string | null;
}
export interface QueueVoiceMenu {
  epoch: string;
  revision: string;
  entryId: string;
  track: TrackView;
  voiceId: string;
  sourceLabel: string;
  radioId: string | null;
  creator: { id: string | null; name: string } | null;
  commentCount: number | null;
  downloaded: boolean | null;
  directoryAvailable: boolean;
}
export interface QueueLikeTarget {
  epoch: string;
  revision: string;
  entryId: string;
  songId: string;
}
export interface QueueMoreRecommendations {
  visible: boolean;
  enabled: boolean;
  canToggle: boolean;
  separatorBefore: string | null;
  prefetchOnOpen: boolean;
}
export interface QueueView {
  fm: boolean;
  clearContext: { roomId: string | null; issue: string | null };
  epoch: string;
  revision: string;
  current: string | null;
  tracks: QueueTrack[];
  moreRecommendations?: QueueMoreRecommendations | null;
}

export function queueMoreRecommendations(queue?: QueueView): QueueMoreRecommendations | undefined {
  return queue?.moreRecommendations?.visible && !queue.fm && !queue.clearContext.roomId
    ? queue.moreRecommendations
    : undefined;
}
export interface QueueCollectionView {
  epoch: string;
  revision: string;
  total: number;
  unsupported: number;
  duplicates: number;
  tracks: TrackView[];
}
export type QueueEdit =
  | { kind: 'select'; entry_id: string }
  | { kind: 'remove' | 'reorder'; entry_ids: string[] }
  | { kind: 'move'; entry_ids: string[]; movement: { entry_ids: string[]; offset: number } }
  | { kind: 'clear' };
export type LocalQueueDropSource = {
  kind: 'local';
  query: import('./local').LocalQuery;
  revision: string;
  trackId: string;
  current: () => boolean;
};
export type PlaylistQueueDropSource = {
  kind: 'playlist';
  id: string;
  revision: string;
  rows: { trackId: string; trackIndex: number }[];
  quality: Quality;
  sourceChanged: () => void;
  current: () => boolean;
};
export type MixedPlaylistQueueDropSource = {
  kind: 'mixed_playlist';
  id: string;
  revision: string;
  rows: { trackId: string; trackIndex: number; local: boolean }[];
  quality: Quality;
  sourceChanged: () => void;
  current: () => boolean;
};
export interface SearchQueueRow {
  receipt: string;
  trackId: string;
}
type SearchQueueRowWire = { receipt: string; track_id: string };
export type SearchQueueDropSource = {
  kind: 'search';
  rows: SearchQueueRow[];
  quality: Quality;
  current: () => boolean;
};
export type AlbumQueueDropGesture = {
  id: string;
  revision: string;
  rows: { trackId: string; trackIndex: number }[];
  current: () => boolean;
};
export type AlbumQueueDropSource = AlbumQueueDropGesture & {
  kind: 'album';
  quality: Quality;
};
export type QueueDropSource =
  | LocalQueueDropSource
  | PlaylistQueueDropSource
  | MixedPlaylistQueueDropSource
  | SearchQueueDropSource
  | AlbumQueueDropSource;
export type SearchResultPlayback = 'song_tab_all' | 'comprehensive_all';
type AlbumDetailSelection = {
  kind: 'album_detail';
  album: import('./albumDurationSort').AlbumPlaybackScope;
  quality: Quality;
};
export type SongRowDoubleClickSelection =
  | {
      kind: 'daily_recommendation';
      revision: string;
      index: number;
      track_id: string;
      quality: Quality;
    }
  | (AlbumDetailSelection & { row: import('./albumDurationSort').AlbumPlaybackRow })
  | ({
      kind: 'latest_release';
      quality: Quality;
    } & import('./latestReleases').LatestSongRowSelection)
  | {
      kind: 'catalog';
      pages: import('./catalog').CatalogSelectionPage[];
      index: number;
      quality: Quality;
    }
  | { kind: 'playlist'; id: string; start_id: string; revision: string | null; quality: Quality };
export type ZonePlaylistBatchSelection = {
  id: string;
  receipt: string;
  ids: string[];
  revision: string;
  quality: Quality;
};

export type SearchPodcastRowGesture = 'index' | 'double_click' | 'menu' | 'next';

export type PlaySelection =
  | (AlbumDetailSelection & { row: import('./albumDurationSort').AlbumPlaybackRow | null })
  | {
      kind: 'vertical_zone_playlist_detail';
      id: string;
      receipt: string;
      revision: string;
      start_id?: string | null;
      double_click: boolean;
      quality: Quality;
    }
  | {
      kind: 'vertical_zone_playlist';
      id: string;
      view_id: string;
      card_key: string;
      quality: Quality;
    }
  | { kind: 'song_row_double_click'; selection: SongRowDoubleClickSelection }
  | { kind: 'message_track'; id: string; quality: Quality }
  | { kind: 'search_podcast_card'; id: string; receipt: string }
  | { kind: 'search_podcast_row'; id: string; receipt: string; gesture: SearchPodcastRowGesture }
  | { kind: 'search_voice_menu'; target: import('./search').SearchVoiceMenuTarget }
  | { kind: 'podcast_browse_voice'; receipt: string; double_click?: boolean }
  | { kind: 'podcast_liked_episode'; target: import('./podcast').PodcastLikedProgramTarget }
  | {
      kind: 'podcast_liked_page_episode';
      target: import('./podcast').PodcastLikedPageTarget;
      double_click?: boolean;
      toggle_current?: boolean;
    }
  | {
      kind: 'podcast_purchased_episode';
      target: import('./podcast').PodcastPurchasedTarget;
      menu?: boolean;
    }
  | { kind: 'voice' | 'search_result_voice' | 'message_voice'; id: string }
  | { kind: 'listened_book_chapter'; revision: string; key: number }
  | {
      kind: 'history_radio_chapter';
      history_kind: 'podcast' | 'audiobook';
      revision: string;
      id: string;
    }
  | {
      kind: 'history_radio_menu';
      history_kind: import('./history').HistoryRadioKind;
      revision: string;
      id: string;
      action: import('./history').HistoryRadioMenuAction;
    }
  | {
      kind: 'podcast_subscription_recommendation';
      id: string;
      receipt: string;
      revision: string;
      action?: import('./podcast').PodcastRecommendationAction;
    }
  | { kind: 'podcast_recent'; revision: string; id: string; double_click: boolean }
  | { kind: 'downloads'; query: import('./downloads').DownloadQuery; start_id: string | null }
  | { kind: 'playlist_batch'; id: string; ids: string[]; revision: string; quality: Quality }
  | ({ kind: 'vertical_zone_playlist_batch' } & ZonePlaylistBatchSelection)
  | {
      kind: 'history';
      history_kind: import('./history').HistoryKind;
      revision: string;
      ids: string[];
      index: number;
      quality: Quality;
    }
  | {
      kind: 'history_add' | 'history_batch_play';
      history_kind: 'song' | 'voice';
      revision: string;
      ids: string[];
      quality: Quality;
    }
  | {
      kind: 'history_add_all' | 'history_play_all';
      history_kind: 'song' | 'voice';
      revision: string;
      quality: Quality;
    }
  | {
      kind: 'history_row_next';
      history_kind: import('./history').HistoryQueueKind;
      revision: string;
      clicked_id: string;
      selected_ids: string[];
      quality: Quality;
    }
  | {
      kind: 'catalog';
      pages: import('./catalog').CatalogSelectionPage[];
      index: number;
      quality: Quality;
    }
  | {
      kind: 'member';
      query: import('./member').MemberQuery;
      revision: string;
      section: string | null;
      index: number;
      quality: Quality;
    }
  | { kind: 'album'; id: string; start_id: string | null; revision: string; quality: Quality }
  | {
      kind: 'podcast';
      scope: import('./podcast').PodcastScope;
      start_id: string | null;
      revision: string;
    }
  | {
      kind: 'local';
      query: import('./local').LocalQuery;
      start_id: string | null;
      revision: string;
    }
  | ({ kind: 'local_batch' | 'local_batch_add' } & import('./local').LocalBatchPlayback)
  | { kind: 'mv_related'; mv_id: string; song_id: string; quality: Quality }
  | {
      kind: 'cloud';
      query: import('./cloud').CloudQuery;
      start_id: string | null;
      revision: string;
      quality: Quality;
    }
  | { kind: 'cloud_batch'; ids: string[]; revision: string; quality: Quality; query?: CloudQuery }
  | { kind: 'search_hot_track'; id: string; quality: Quality }
  | { kind: 'search_artist_card'; id: string; quality: Quality }
  | { kind: 'search_result_track'; id: string; quality: Quality; toggle_current: boolean }
  | {
      kind: 'search_result_menu_play';
      id: string;
      quality: Quality;
      expected_play_id: string | null;
      expected_queue_revision: string;
    }
  | {
      kind: 'search_results';
      ids: string[];
      index: number;
      quality: Quality;
      context: SearchResultPlayback;
    }
  | { kind: 'search_chart'; ids: string[]; quality: Quality }
  | { kind: 'songs'; ids: string[]; index: number; quality: Quality }
  | { kind: 'follow_latest'; revision: string; quality: Quality }
  | { kind: 'profile_rank_playlist'; uid: string | null; quality: Quality }
  | {
      kind: 'daily_recommendations' | 'style_recommendations';
      revision: string;
      index: number;
      quality: Quality;
    }
  | { kind: 'daily_recommendations_batch'; revision: string; indices: number[]; quality: Quality }
  | {
      kind: 'sorted_playlist';
      scope: import('./playlistDuration').PlaylistDurationScope;
      start: { id: string; local: boolean } | null;
      double_click?: boolean;
      quality: Quality;
    }
  | {
      kind: 'offline_playlist';
      id: string;
      revision: string;
      cached_at_ms: number;
      start_id: string | null;
      start_local?: boolean;
      double_click?: boolean;
    }
  | {
      kind: 'mixed_playlist';
      id: string;
      revision: string;
      start: { id: string; local: boolean } | null;
      double_click: boolean;
      quality: Quality;
    }
  | {
      kind: 'playlist';
      id: string;
      start_id: string | null;
      revision: string | null;
      quality: Quality;
    };
export type PlayerAction =
  | {
      kind: 'step_mini_bar';
      play_id: string;
      expected_queue_revision: string;
      direction: 'next' | 'previous';
    }
  | {
      kind: 'step_ended_mini_bar';
      receipt: string;
      entry_id: string;
      expected_queue_revision: string;
      direction: 'next' | 'previous';
    }
  | { kind: 'queue_more_opened'; expected_revision: string }
  | { kind: 'queue_more_toggle'; enabled: boolean; expected_revision: string }
  | {
      kind: 'step_loading';
      play_id: string;
      expected_queue_revision: string;
      direction: 'next' | 'previous';
    }
  | {
      kind: 'queue_for';
      expected_queue_revision: string;
      expected_room_id?: string | null;
      edit: QueueEdit;
    }
  | {
      kind: 'queue_playlist_batch';
      id: string;
      ids: string[];
      revision: string;
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_vertical_zone_playlist_all';
      id: string;
      receipt: string;
      revision: string;
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_vertical_zone_playlist_next';
      id: string;
      receipt: string;
      revision: string;
      song_id: string;
      quality: Quality;
      expected_queue_revision: string;
    }
  | ({
      kind: 'queue_vertical_zone_playlist_batch';
      expected_queue_revision: string;
    } & ZonePlaylistBatchSelection)
  | {
      kind: 'queue_cloud_batch';
      query?: CloudQuery;
      ids: string[];
      revision: string;
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_local';
      query: import('./local').LocalQuery;
      revision: string;
      track_id?: string;
      expected_queue_revision: string;
    }
  | {
      kind: 'drop_local_into_queue';
      query: import('./local').LocalQuery;
      revision: string;
      track_id: string;
      expected_queue_revision: string;
      expected_play_id: string | null;
      before_entry_id: string | null;
    }
  | {
      kind: 'drop_playlist_song_into_queue';
      id: string;
      revision: string;
      track_id: string;
      track_index: number;
      quality: Quality;
      expected_queue_revision: string;
      expected_play_id: string | null;
      before_entry_id: string | null;
    }
  | {
      kind: 'drop_playlist_songs_into_queue';
      id: string;
      revision: string;
      rows: { track_id: string; track_index: number }[];
      quality: Quality;
      expected_queue_revision: string;
      expected_play_id: string | null;
      before_entry_id: string | null;
    }
  | {
      kind: 'drop_search_songs_into_queue';
      rows: { receipt: string; track_id: string }[];
      quality: Quality;
      expected_queue_revision: string;
      expected_play_id: string | null;
      before_entry_id: string | null;
    }
  | {
      kind: 'drop_album_songs_into_queue';
      id: string;
      revision: string;
      rows: { track_id: string; track_index: number }[];
      quality: Quality;
      expected_queue_revision: string;
      expected_play_id: string | null;
      before_entry_id: string | null;
    }
  | { kind: 'queue_downloads'; task_ids: string[]; expected_queue_revision: string }
  | { kind: 'queue_playlist'; id: string; quality: Quality; expected_queue_revision: string }
  | {
      kind: 'queue_sorted_playlist';
      scope: import('./playlistDuration').PlaylistDurationScope;
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_offline_playlist';
      id: string;
      revision: string;
      cached_at_ms: number;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_mixed_playlist';
      id: string;
      revision: string;
      track: { id: string; local: boolean } | null;
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'drop_mixed_playlist_songs_into_queue';
      id: string;
      revision: string;
      rows: { track_id: string; track_index: number; local: boolean }[];
      quality: Quality;
      expected_queue_revision: string;
      expected_play_id: string | null;
      before_entry_id: string | null;
    }
  | {
      kind: 'queue_voice';
      id: string;
      program?: import('./podcast').PodcastProgramTarget;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_recent_program';
      target: import('./podcast').PodcastRecentProgramTarget;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_purchased_program';
      target: import('./podcast').PodcastPurchasedTarget;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_liked_program';
      target: import('./podcast').PodcastLikedProgramTarget;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_liked_page_program';
      target: import('./podcast').PodcastLikedPageTarget;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_style_recommendations';
      revision: string;
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_daily_recommendations';
      revision: string;
      indices?: number[];
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_artist_songs';
      pages: import('./catalog').CatalogSelectionPage[];
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_artist_songs_batch';
      pages: import('./catalog').CatalogSelectionPage[];
      ids: string[];
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_album_batch';
      id: string;
      ids: string[];
      revision: string;
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'queue_album_detail';
      album: import('./albumDurationSort').AlbumPlaybackScope;
      quality: Quality;
      expected_queue_revision: string;
    }
  | { kind: 'queue_album'; id: string; quality: Quality; expected_queue_revision: string }
  | {
      kind: 'next_play' | 'search_next_play';
      id: string;
      quality: Quality;
      expected_queue_revision: string;
    }
  | {
      kind: 'search_group_next_play' | 'search_group_play';
      anchor: SearchQueueRowWire;
      rows: SearchQueueRowWire[];
      quality: Quality;
      expected_play_id: string | null;
      expected_queue_revision: string;
    }
  | { kind: 'loudness'; expected_revision: string; enabled: boolean }
  | { kind: 'retry_loudness' | 'repair_loudness' }
  | { kind: 'repair_quality' }
  | {
      kind: 'equalizer';
      expected_revision: string;
      settings: EqualizerSettings;
      preset?: EqualizerPreset;
    }
  | { kind: 'save_custom_equalizer'; expected_revision: string }
  | { kind: 'retry_equalizer' | 'repair_equalizer' }
  | { kind: 'play' | 'next' | 'previous' | 'stop' | 'restore' | 'startup' | 'refresh' | 'clear' }
  | { kind: 'refresh_for'; play_id: string; expected_queue_revision: string }
  | { kind: 'play_missing_download_online'; play_id: string; expected_queue_revision: string }
  | { kind: 'pause'; paused: boolean }
  | { kind: 'pause_for'; play_id: string; expected_queue_revision: string; paused: boolean }
  | { kind: 'seek'; seconds: number }
  | { kind: 'seek_for'; play_id: string; expected_queue_revision: string; seconds: number }
  | { kind: 'volume' | 'speed'; value: number }
  | { kind: 'speed_for'; scope: PlaybackSpeedScope; value: number }
  | {
      kind: 'song_speed';
      expected_revision: string;
      expected_entry: string;
      selection: SongSpeedSelection;
    }
  | { kind: 'quality'; level: Quality }
  | { kind: 'quality_for'; level: Quality; play_id: string; expected_revision: string }
  | { kind: 'mode'; mode: PlaybackMode }
  | { kind: 'mode_for'; mode: PlaybackMode; expected_queue_revision: string; fm: boolean }
  | { kind: 'select'; entry_id: string }
  | { kind: 'remove' | 'reorder'; entry_ids: string[] };
export interface LyricLine {
  time?: number;
  lyric: string;
}
export interface LyricPreferences {
  translationEnable: boolean;
  romaEnable: boolean;
}
export type LyricToolbarAction = 'translation' | 'upload';
export type LyricBountyAction = 'missing_lyrics' | 'scrolling' | 'translation';
export type LyricBountyStatus = 'accepted' | 'not_sent' | 'unknown';
export interface LyricBountyOpening {
  id: string | null;
  status: LyricBountyStatus | null;
}
export interface LyricsView {
  wordTranslation: LyricLine[];
  wordRomanization: LyricLine[];
  wordLines: {
    text: string;
    time: number;
    duration: number;
    words: { time: number; duration: number; text: string }[];
  }[];
  epoch: string;
  playId: string;
  lines: LyricLine[];
  translation: LyricLine[];
  romanization: LyricLine[];
  lyricContributor?: import('./lyricContributors').LyricContributor | null;
  translationContributor?: import('./lyricContributors').LyricContributor | null;
  contribution?: 'upload' | 'scrolling' | 'translation' | 'scrolling_and_translation';
  toolbarContributions?: LyricToolbarAction[];
  words: { yrc?: string; yrcRoma?: string; yrcTrans?: string };
  offset: number;
  scrollable: boolean;
  displayType: 'default' | 'pure' | 'ask';
  preferences: LyricPreferences;
  networkFailed: boolean;
  isCloudLyric: boolean;
  fromCache: boolean;
  fromSidecar?: boolean;
  fromEmbedded?: boolean;
}
export type PlayerUpdate =
  | { kind: 'state'; value: PlayerView; nativeOpenError?: CoreFailure }
  | { kind: 'ordinary_copyright_skip'; epoch: string; id: string; remainingMs: number }
  | { kind: 'error'; error: CoreFailure };
export type QualityAvailability = 'available' | 'unavailable' | 'unknown';
export type QualityAccess =
  | 'allowed'
  | 'trial_eligible'
  | 'trial_active'
  | 'requires_vip'
  | 'requires_svip'
  | 'requires_purchase'
  | 'unavailable'
  | 'trial_unsupported'
  | 'unknown';
export type QualityTrialPhase = 'eligible' | 'active' | 'granted' | 'unavailable' | 'unknown';
export type QualityReadState = 'ready' | 'missing' | 'failed';
export interface QualityChoiceView {
  quality: Quality;
  availability: QualityAvailability;
  access: QualityAccess;
  sizeBytes: string | null;
  trialPhase: QualityTrialPhase | null;
  remainingMs: number | null;
}
export interface QualityDetailsView {
  epoch: string;
  playId: string;
  songId: string;
  revision: string;
  choices: QualityChoiceView[];
  presentation?: { kind: 'cards'; vivid: boolean };
  status: {
    privilege: QualityReadState;
    subscriber: QualityReadState;
    sizes: QualityReadState;
    useRights: Partial<Record<'vivid' | 'jymaster' | 'sky' | 'jyeffect', QualityReadState>>;
  };
  refreshAfterMs: number | null;
  trials: QualityTrialView[];
  trialsDamaged: boolean;
}
export type QualityTrialRight = 'vivid' | 'jymaster' | 'sky' | 'jyeffect';
export interface QualityTrialView {
  right: QualityTrialRight;
  operation: string;
  revision: string;
  phase: 'sending' | 'uncertain' | 'acknowledged' | 'active' | 'expired' | 'rejected';
  acknowledged: boolean;
  canUse: boolean;
  readFailed: boolean;
  remainingMs: number | null;
}
export type QualityTrialCommand =
  | { kind: 'start' | 'use_active'; play_id: string; details_revision: string; level: Quality }
  | { kind: 'reconcile'; right: QualityTrialRight; operation: string };
export interface QualityTrialOutcome {
  trial: QualityTrialView;
  playback: PlayerView | null;
  playbackError: CoreFailure | null;
}
export type IntelligenceOrigin =
  | { kind: 'current' }
  | { kind: 'likes'; playlist_id: string; revision?: string; first_id?: string };
export type IntelligenceCommand =
  | {
      kind: 'start';
      operation_id: string;
      expected_queue_revision: string;
      origin: IntelligenceOrigin;
    }
  | { kind: 'continue'; operation_id: string; expected_queue_revision: string };
export interface IntelligenceEligibility {
  epoch: string;
  revision: string;
  canEnter: boolean;
  likedPlaylistId: string | null;
}
export type QueueMoreTipAction =
  | { kind: 'begin'; play_id: string }
  | { kind: 'seen'; ticket: string };

export interface PlayerClient
  extends VinylSimilarClient,
    VoiceDetailClient,
    VoiceRecommendationsClient,
    VoiceSongsClient {
  playerSnapshot(epoch: string): Promise<PlayerView>;
  playerIntelligence(epoch: string, command: IntelligenceCommand): Promise<PlayerView>;
  playerIntelligenceCancel(epoch: string, operationId: string): Promise<boolean>;
  playerIntelligenceEligibility(epoch: string, revision: string): Promise<IntelligenceEligibility>;
  watchPlayer(epoch: string, update: (event: PlayerUpdate) => void): Promise<() => void>;
  playSelection(epoch: string, selection: PlaySelection): Promise<PlayerView>;
  preparePlaySelection(
    epoch: string,
    expectedQueueRevision: string,
    expectedPlayId: string | null,
  ): Promise<string>;
  commitPreparedSelection(
    epoch: string,
    ticket: string,
    selection: PlaySelection,
  ): Promise<PlayerView>;
  playerAction(epoch: string, action: PlayerAction): Promise<PlayerView>;
  playerQualityDetails(epoch: string, playId: string): Promise<QualityDetailsView>;
  playerQualityTrial(epoch: string, command: QualityTrialCommand): Promise<QualityTrialOutcome>;
  playerQueue(epoch: string): Promise<QueueView>;
  queueMoreTip(epoch: string, action: QueueMoreTipAction): Promise<string | null>;
  playerQueueCollection(epoch: string, revision: string): Promise<QueueCollectionView>;
  playerQueueCollectRows(
    epoch: string,
    revision: string,
    entryIds: string[],
  ): Promise<QueueCollectionView>;
  playerQueueLike(
    epoch: string,
    revision: string,
    entryId: string,
    liked: boolean,
  ): Promise<AccountData<LikeResult>>;
  playerQueueCatalogMenu(
    epoch: string,
    revision: string,
    entryId: string,
  ): Promise<QueueCatalogMenu>;
  playerQueueOpenCatalogDirectory(
    epoch: string,
    revision: string,
    entryId: string,
    receipt: string,
  ): Promise<void>;
  playerQueueVoiceMenu(epoch: string, revision: string, entryId: string): Promise<QueueVoiceMenu>;
  playerQueueOpenVoiceDirectory(epoch: string, revision: string, entryId: string): Promise<void>;
  playerQueueOpenLocalDirectory(epoch: string, revision: string, entryId: string): Promise<void>;
  playerQueueOpenDownloadDirectory(epoch: string, revision: string, entryId: string): Promise<void>;
  playerChorus(epoch: string, playId: string): Promise<number | null>;
  playerCommentCount(epoch: string, playId: string): Promise<number | null>;
  playerVinylCommentsAvailable(epoch: string): Promise<boolean>;
  playerVinylComments(
    epoch: string,
    playId: string,
    offset: number,
  ): Promise<AccountData<VinylCommentPage>>;
  playerVinylCommentLikeOperations(
    epoch: string,
    playId: string,
  ): Promise<AccountData<CommentOperation[]>>;
  playerVinylCommentLike(
    epoch: string,
    playId: string,
    id: string,
    action: VinylCommentLikeAction,
  ): Promise<AccountData<CommentOperation>>;
  playerVinylCommentReport(
    epoch: string,
    playId: string,
    id: string,
    command: ReportCommand,
  ): Promise<AccountData<ProfileReportView>>;
  playerLyrics(epoch: string, playId: string, cachedOnly?: boolean): Promise<LyricsView>;
  openLyricContribution(epoch: string, playId: string): Promise<void>;
  lyricBountyActions(epoch: string, playId: string): Promise<LyricBountyAction[]>;
  lyricBountyPrepare(
    epoch: string,
    playId: string,
    action: LyricBountyAction,
  ): Promise<LyricBountyOpening>;
  lyricBountyConfirm(id: string): Promise<LyricBountyStatus>;
  lyricBountyCancel(id: string): Promise<void>;
  openLyricToolbarContribution(
    epoch: string,
    playId: string,
    action: LyricToolbarAction,
  ): Promise<void>;
  lyricPreferences(
    epoch: string,
    playId: string,
    preferences: LyricPreferences,
  ): Promise<LyricsView>;
  lyricOffset(epoch: string, playId: string, gain: number): Promise<LyricsView>;
}
export function time(seconds: number | null | undefined) {
  const n = Math.max(0, Math.floor(seconds ?? 0));
  return `${Math.floor(n / 60)
    .toString()
    .padStart(2, '0')}:${(n % 60).toString().padStart(2, '0')}`;
}
