import type { TrackView, CoreFailure } from './contracts';
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
export interface PlayerView {
  origin?: PlayerOrigin | null;
  fm: boolean;
  intelligence?: 'seeded' | 'active' | 'fallback' | null;
  intelligenceOperation?: string | null;
  equalizer: EqualizerView;
  loudness: LoudnessView;
  epoch: string;
  revision: string;
  queueRevision: string;
  state: PlaybackStatus;
  playId: string | null;
  entryId: string | null;
  track: TrackView | null;
  position: number | null;
  timeline: Timeline | null;
  qualityPreferences: {
    revision: string | null;
    quality: Quality;
    damaged: boolean;
    appliesToCurrent: boolean;
  };
  canChangeQuality: boolean;
  requestedQuality: Quality | null;
  actualQuality: Quality | null;
  volume: number;
  unmuteVolume: number;
  speed: number;
  mode: PlaybackMode;
  queueLen: number;
  error: string | null;
  historyWarning: string | null;
  positionReset: boolean;
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
}
export interface EqualizerView {
  revision: string;
  settings: EqualizerSettings;
  status: 'pending' | 'unavailable' | 'configured' | 'processing' | 'failed';
  damaged: boolean;
}
export interface QueueTrack {
  entryId: string;
  track: TrackView;
}
export interface QueueView {
  fm: boolean;
  clearContext: { roomId: string | null; issue: string | null };
  epoch: string;
  revision: string;
  current: string | null;
  tracks: QueueTrack[];
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
  | { kind: 'clear' };
export type PlaySelection =
  | { kind: 'downloads'; query: import('./downloads').DownloadQuery; start_id: string | null }
  | { kind: 'playlist_batch'; id: string; ids: string[]; revision: string; quality: Quality }
  | {
      kind: 'history';
      history_kind: import('./history').HistoryKind;
      revision: string;
      ids: string[];
      index: number;
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
  | { kind: 'mv_related'; mv_id: string; song_id: string; quality: Quality }
  | {
      kind: 'cloud';
      query: import('./cloud').CloudQuery;
      start_id: string | null;
      revision: string;
      quality: Quality;
    }
  | { kind: 'songs'; ids: string[]; index: number; quality: Quality }
  | {
      kind: 'playlist';
      id: string;
      start_id: string | null;
      revision: string | null;
      quality: Quality;
    };
export type PlayerAction =
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
  | { kind: 'queue_downloads'; task_ids: string[]; expected_queue_revision: string }
  | { kind: 'queue_playlist'; id: string; quality: Quality; expected_queue_revision: string }
  | { kind: 'next_play'; id: string; quality: Quality; expected_queue_revision: string }
  | { kind: 'loudness'; expected_revision: string; enabled: boolean }
  | { kind: 'retry_loudness' | 'repair_loudness' }
  | { kind: 'repair_quality' }
  | { kind: 'equalizer'; expected_revision: string; settings: EqualizerSettings }
  | { kind: 'retry_equalizer' | 'repair_equalizer' }
  | { kind: 'play' | 'next' | 'previous' | 'stop' | 'restore' | 'startup' | 'refresh' | 'clear' }
  | { kind: 'pause'; paused: boolean }
  | { kind: 'seek'; seconds: number }
  | { kind: 'volume' | 'speed'; value: number }
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
  words: { yrc?: string; yrcRoma?: string; yrcTrans?: string };
  offset: number;
  scrollable: boolean;
  displayType: 'default' | 'pure' | 'ask';
  preferences: LyricPreferences;
  networkFailed: boolean;
  fromCache: boolean;
}
export type PlayerUpdate =
  | { kind: 'state'; value: PlayerView }
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
export interface PlayerClient {
  playerSnapshot(epoch: string): Promise<PlayerView>;
  playerIntelligence(epoch: string, command: IntelligenceCommand): Promise<PlayerView>;
  playerIntelligenceCancel(epoch: string, operationId: string): Promise<boolean>;
  playerIntelligenceEligibility(epoch: string, revision: string): Promise<IntelligenceEligibility>;
  watchPlayer(epoch: string, update: (event: PlayerUpdate) => void): Promise<() => void>;
  playSelection(epoch: string, selection: PlaySelection): Promise<PlayerView>;
  playerAction(epoch: string, action: PlayerAction): Promise<PlayerView>;
  playerQualityDetails(epoch: string, playId: string): Promise<QualityDetailsView>;
  playerQualityTrial(epoch: string, command: QualityTrialCommand): Promise<QualityTrialOutcome>;
  playerQueue(epoch: string): Promise<QueueView>;
  playerQueueCollection(epoch: string, revision: string): Promise<QueueCollectionView>;
  playerLyrics(epoch: string, playId: string): Promise<LyricsView>;
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
