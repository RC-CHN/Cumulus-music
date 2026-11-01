import type { PlaybackMode, QueueTrack } from './player';
import type { ShortcutSelection } from './shortcuts';
import type { ModeFamily } from './playbackMode';
import type { FmReasons } from './fm';
export interface MiniFmFeedbackView {
  epoch: string;
  entryId: string;
  title: string;
  pending: boolean;
}
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
  modeFamily: ModeFamily | null;
  fmModePending?: { mode: PlaybackMode; revision: string } | null;
  queueRevision?: string | null;
  canIntelligence?: boolean;
  intelligence?: 'seeded' | 'active' | 'fallback' | null;
  intelligenceOperation?: string | null;
  intelligenceRead?: { operationId: string; preparing: boolean } | null;
  like: { observed: boolean | null; pending: boolean } | null;
  error: string | null;
}
export interface MiniView {
  generation: string;
  ticket: string;
  media: MiniMedia;
  preferences: MiniPreferences;
  pinApplied: boolean | null;
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
  returnMain(fm?: boolean): Promise<void>;
  feedbackView(ticket: string, refresh: boolean): Promise<MiniFmFeedbackView>;
  feedbackReasons(ticket: string): Promise<FmReasons>;
  feedbackSubmit(ticket: string, reasonKey?: string): Promise<void>;
  action(ticket: string, action: MiniAction): Promise<void>;
  intelligence(ticket: string, command: MiniIntelligenceCommand): Promise<MiniPlaybackReceipt>;
  mode(ticket: string, mode: PlaybackMode): Promise<MiniPlaybackReceipt>;
  cancelIntelligence(epoch: string, operationId: string): Promise<boolean>;
  queue(ticket: string, offset: number): Promise<MiniQueuePage>;
  panel(expanded: boolean): Promise<void>;
  preferences(ticket: string, command: MiniPreferencesCommand): Promise<void>;
  applyPreferences(ticket: string, expectedRevision: string): Promise<void>;
  drag(): Promise<void>;
  close(): Promise<void>;
}
export type MiniIntelligenceCommand = { kind: 'start' | 'continue'; operation_id: string };
export interface MiniPlaybackReceipt {
  epoch: string;
  queueRevision: string;
  playId: string | null;
  mode: PlaybackMode;
  fm: boolean;
  intelligence: MiniMedia['intelligence'];
  intelligenceOperation: string | null;
}
