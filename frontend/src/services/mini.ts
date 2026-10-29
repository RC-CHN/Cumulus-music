import type { PlaybackMode, QueueTrack } from './player';
import type { ShortcutSelection } from './shortcuts';
export interface MiniPreferences {
  revision: string | null;
  alwaysOnTop: boolean;
  damaged: boolean;
}
export type MiniPreferencesCommand =
  | { kind: 'change'; expected_revision: string; always_on_top: boolean }
  | { kind: 'repair' };
export interface MiniMedia {
  epoch: string;
  selection: ShortcutSelection;
  item: {
    id: string;
    kind: 'music' | 'video';
    title: string;
    artists: string[];
    album: string;
    artwork: string | null;
  } | null;
  state: 'stopped' | 'loading' | 'playing' | 'paused';
  playing: boolean;
  canPrevious: boolean;
  canNext: boolean;
  position: number | null;
  duration: number | null;
  seekStart: number;
  seekEnd: number | null;
  canSeek: boolean;
  volume: number;
  mode: PlaybackMode | null;
  like: { observed: boolean | null; pending: boolean } | null;
  error: string | null;
}
export interface MiniView {
  generation: string;
  ticket: string;
  media: MiniMedia;
  preferences: MiniPreferences;
}
export interface MiniQueuePage {
  epoch: string;
  current: string | null;
  tracks: QueueTrack[];
  offset: number;
  total: number;
  editable: boolean;
}
export type MiniAction =
  | { kind: 'toggle' | 'previous' | 'next' | 'stop' | 'like' | 'refresh_like' | 'clear' }
  | { kind: 'seek'; seconds: number }
  | { kind: 'volume'; value: number }
  | { kind: 'mode'; mode: PlaybackMode }
  | { kind: 'select' | 'remove'; entry_id: string };
export interface MiniClient {
  read(): Promise<MiniView>;
  ready(): Promise<void>;
  returnMain(): Promise<void>;
  action(ticket: string, action: MiniAction): Promise<void>;
  queue(ticket: string, offset: number): Promise<MiniQueuePage>;
  panel(expanded: boolean): Promise<void>;
  preferences(ticket: string, command: MiniPreferencesCommand): Promise<void>;
  drag(): Promise<void>;
  close(): Promise<void>;
}
