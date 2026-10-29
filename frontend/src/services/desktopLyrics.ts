import type { MiniMedia } from './mini';
export type LyricAuxiliary = 'none' | 'translate' | 'roman';
export type LyricAlignment = 'center' | 'split' | 'left' | 'right';
export type LyricPreset =
  | 'gray'
  | 'sunset'
  | 'pink'
  | 'purple'
  | 'blue'
  | 'green'
  | 'golden'
  | 'red';
export interface LyricColors {
  text: [string, string, string, string];
  outline: [string, string];
}
export interface LyricAppearance {
  auxiliary: LyricAuxiliary;
  alwaysOnTop: boolean;
  font: string;
  fontSize: number;
  bold: boolean;
  outline: boolean;
  singleLine: boolean;
  composition: 'horizontal' | 'vertical';
  singleAlignment: LyricAlignment;
  doubleAlignment: LyricAlignment;
  palette: { kind: 'preset'; preset: LyricPreset } | { kind: 'custom'; colors: LyricColors };
}
export interface DesktopLyricPreferences {
  revision: string;
  enabled: boolean;
  appearance: LyricAppearance;
  geometry: { x: number; y: number; width: number; height: number } | null;
  customPalette: LyricColors | null;
}
export interface LyricPreferencesView {
  current: DesktopLyricPreferences | null;
  damaged: boolean;
}
export interface LyricSettingsView {
  epoch: string;
  preferences: LyricPreferencesView;
  colors: LyricColors | null;
  visible: boolean;
  opening: boolean;
  locked: boolean;
  error: string | null;
}
export type LyricEdit =
  | { kind: 'enabled'; value: boolean }
  | { kind: 'appearance'; value: LyricAppearance }
  | { kind: 'geometry'; value: DesktopLyricPreferences['geometry'] }
  | { kind: 'custom_colors'; colors: LyricColors }
  | { kind: 'reset' };
export type LyricPreferencesCommand =
  | { kind: 'repair' }
  | { kind: 'change'; expected_revision: string; edit: LyricEdit };
export interface LyricSettingsClient {
  read(epoch: string): Promise<LyricSettingsView>;
  change(epoch: string, command: LyricPreferencesCommand): Promise<LyricSettingsView>;
  window(epoch: string, action: 'retry' | 'unlock'): Promise<LyricSettingsView>;
}
export interface DesktopLyricWord {
  text: string;
  start: number;
  duration: number;
  progress: number;
}
export interface DesktopLyricLine {
  index: number;
  text: string;
  auxiliary: string;
  start: number | null;
  end: number | null;
  active: boolean;
  progress: number;
  words: DesktopLyricWord[];
}
export interface DesktopLyricView {
  generation: string;
  ticket: string;
  media: MiniMedia;
  preferences: LyricPreferencesView;
  colors: [number, number, number, number][];
  fontFamilies: string[];
  alignments: ['left' | 'center' | 'right', 'left' | 'center' | 'right'];
  locked: boolean;
  state: 'idle' | 'loading' | 'ready' | 'failed';
  lyrics: {
    epoch: string;
    playId: string;
    revision: string;
    playing: boolean;
    speed: number;
    seekRevision: number;
    duration: number | null;
    info: { scrollable: boolean; networkFailed: boolean; fromCache: boolean; fetchFailed: boolean };
    frame: {
      displayType: 'default' | 'pure' | 'ask';
      timedWords: boolean;
      position: number;
      offset: number;
      lines: DesktopLyricLine[];
    };
  } | null;
  error: string | null;
}
export type DesktopLyricAction =
  | 'toggle'
  | 'previous'
  | 'next'
  | 'offset_back'
  | 'offset_forward'
  | 'retry'
  | 'lock'
  | 'close'
  | 'details'
  | 'settings';
export interface DesktopLyricClient {
  ready(): Promise<void>;
  read(): Promise<DesktopLyricView>;
  action(ticket: string, action: DesktopLyricAction): Promise<void>;
  drag(): Promise<void>;
  close(): Promise<void>;
  resize(edge: 'East' | 'South' | 'SouthEast'): Promise<void>;
}
