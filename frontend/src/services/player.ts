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
  { value: 'jyeffect', label: '高清环绕声' },
  { value: 'jymaster', label: '超清母带' },
  { value: 'sky', label: '沉浸环绕声' },
  { value: 'vivid', label: '臻品音质' },
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
  fm: boolean;
  equalizer: EqualizerView;
  epoch: string;
  revision: string;
  queueRevision: string;
  state: PlaybackStatus;
  playId: string | null;
  entryId: string | null;
  track: TrackView | null;
  position: number | null;
  timeline: Timeline | null;
  requestedQuality: Quality | null;
  actualQuality: Quality | null;
  volume: number;
  speed: number;
  mode: PlaybackMode;
  queueLen: number;
  error: string | null;
  historyWarning: string | null;
  positionReset: boolean;
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
  epoch: string;
  revision: string;
  current: string | null;
  tracks: QueueTrack[];
}
export type PlaySelection =
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
  | { kind: 'equalizer'; expected_revision: string; settings: EqualizerSettings }
  | { kind: 'retry_equalizer' | 'repair_equalizer' }
  | { kind: 'play' | 'next' | 'previous' | 'stop' | 'restore' | 'refresh' | 'clear' }
  | { kind: 'pause'; paused: boolean }
  | { kind: 'seek'; seconds: number }
  | { kind: 'volume' | 'speed'; value: number }
  | { kind: 'quality'; level: Quality }
  | { kind: 'mode'; mode: PlaybackMode }
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
export interface PlayerClient {
  watchPlayer(epoch: string, update: (event: PlayerUpdate) => void): Promise<() => void>;
  playSelection(epoch: string, selection: PlaySelection): Promise<PlayerView>;
  playerAction(epoch: string, action: PlayerAction): Promise<PlayerView>;
  playerQueue(epoch: string): Promise<QueueView>;
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
