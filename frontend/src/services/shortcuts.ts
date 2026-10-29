import type { AccountData, CoreFailure } from './contracts';
export type ShortcutAction =
  | 'play_pause'
  | 'prev'
  | 'next'
  | 'add_volume'
  | 'cut_volume'
  | 'mini_max'
  | 'love'
  | 'lyric'
  | 'translate';
export type ShortcutScope = 'local' | 'global';
export type Hotkey =
  | { kind: 'configured'; action: ShortcutAction }
  | {
      kind:
        | 'f5'
        | 'space'
        | 'close'
        | 'media_play_pause'
        | 'media_next'
        | 'media_previous'
        | 'media_stop';
    };
export interface ShortcutSettings {
  revision: string;
  global_enabled: boolean;
  media_enabled: boolean;
  bindings: Partial<Record<ShortcutAction, { local: number[] | null; global: number[] | null }>>;
}
export interface ShortcutSettingsView {
  current: ShortcutSettings | null;
  damaged: boolean;
}
export type ShortcutEdit =
  | { kind: 'bind'; action: ShortcutAction; scope: ShortcutScope; keys: number[] }
  | { kind: 'global' | 'media'; enabled: boolean }
  | { kind: 'reset' };
export type ShortcutCommand =
  | { kind: 'change'; expected_revision: string; edit: ShortcutEdit }
  | { kind: 'repair' };
export interface KeyInput {
  keyCode: number;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}
export type KeyCapture = { kind: 'ignore' } | { kind: 'clear' } | { kind: 'set'; keys: number[] };
export interface ShortcutRow {
  hotkey: Hotkey;
  scope: ShortcutScope;
  chord: number[];
  status:
    | { kind: 'active' }
    | { kind: 'pending' }
    | { kind: 'conflict'; code: number }
    | { kind: 'unavailable'; code: number };
}
export interface ShortcutStatus {
  revision: string;
  settingsRevision: string | null;
  ticket: string | null;
  recording: string | null;
  phase:
    | 'not_installed'
    | 'starting'
    | 'ready'
    | 'recording'
    | 'damaged'
    | 'read_failed'
    | 'unavailable'
    | 'stopped';
  rows: ShortcutRow[];
}
export interface ShortcutLease {
  id: string;
  ticket: string;
}
export interface ShortcutSelection {
  audioPlayId: string | null;
  audioEntryId: string | null;
  videoPlayId: string | null;
}
export interface ShortcutPress {
  lease: string;
  ticket: string;
  epoch: string;
  keys: number[];
  selection: ShortcutSelection;
}
export interface ShortcutPreferencesClient {
  shortcutSettings(epoch: string): Promise<AccountData<ShortcutSettingsView>>;
  shortcutSettingsCommand(
    epoch: string,
    command: ShortcutCommand,
  ): Promise<AccountData<ShortcutSettingsView>>;
  shortcutCapture(input: KeyInput, scope: ShortcutScope): Promise<KeyCapture>;
  shortcutBeginRecording(epoch: string): Promise<string>;
  shortcutEndRecording(id: string): Promise<void>;
}
export interface ShortcutStatusClient {
  shortcutStatus(): Promise<ShortcutStatus>;
  onShortcuts(handler: (value: ShortcutStatus) => void): Promise<() => void>;
}
export interface ShortcutKeysClient extends ShortcutStatusClient {
  shortcutAttach(epoch: string, ticket: string): Promise<ShortcutLease>;
  shortcutDetach(lease: string): Promise<void>;
  shortcutPress(press: ShortcutPress): Promise<void>;
  onShortcutError(
    handler: (value: { epoch: string; error: CoreFailure }) => void,
  ): Promise<() => void>;
}
export interface ShortcutsClient extends ShortcutPreferencesClient, ShortcutKeysClient {}
export const shortcutLabels: Record<ShortcutAction, string> = {
  play_pause: '播放／暂停',
  prev: '上一首',
  next: '下一首',
  add_volume: '增加音量',
  cut_volume: '降低音量',
  mini_max: '迷你模式',
  love: '喜欢／取消喜欢',
  lyric: '桌面歌词',
  translate: '翻译',
};
export const shortcutOrder: ShortcutAction[] = [
  'play_pause',
  'prev',
  'next',
  'add_volume',
  'cut_volume',
  'mini_max',
  'love',
  'lyric',
  'translate',
];
const names: Record<number, string> = {
  16: 'Shift',
  17: 'Ctrl',
  18: 'Alt',
  32: '空格',
  33: 'Page Up',
  34: 'Page Down',
  35: 'End',
  36: 'Home',
  37: '←',
  38: '↑',
  39: '→',
  40: '↓',
  106: '数字键盘 *',
  107: '数字键盘 +',
  109: '数字键盘 −',
  110: '数字键盘 .',
  111: '数字键盘 /',
  186: ';',
  187: '=',
  188: ',',
  189: '−',
  190: '.',
  191: '/',
  192: '`',
  219: '[',
  220: '\\',
  221: ']',
  222: "'",
};
export function shortcutText(keys: readonly number[] | null | undefined): string {
  if (!keys?.length) return '未设置';
  const ordered = [
    ...[17, 18, 16].filter((k) => keys.includes(k)),
    ...keys.filter((k) => ![16, 17, 18].includes(k)),
  ];
  return ordered
    .map(
      (k) =>
        names[k] ??
        (k >= 112 && k <= 122
          ? `F${k - 111}`
          : k >= 96 && k <= 105
            ? `数字键盘 ${k - 96}`
            : String.fromCharCode(k)),
    )
    .join(' + ');
}
export function keyInput(event: KeyboardEvent): KeyInput {
  let keyCode = event.keyCode;
  if (!keyCode) {
    const codes: Record<string, number> = {
      Space: 32,
      ArrowLeft: 37,
      ArrowUp: 38,
      ArrowRight: 39,
      ArrowDown: 40,
      Backspace: 8,
      Enter: 13,
    };
    keyCode =
      codes[event.code] ??
      (/^Key[A-Z]$/.test(event.code)
        ? event.code.charCodeAt(3)
        : /^Digit[0-9]$/.test(event.code)
          ? event.code.charCodeAt(5)
          : /^F(?:[1-9]|1[0-1])$/.test(event.code)
            ? 111 + Number(event.code.slice(1))
            : 0);
  }
  return {
    keyCode,
    ctrlKey: event.ctrlKey,
    shiftKey: event.shiftKey,
    altKey: event.altKey,
    metaKey: event.metaKey,
  };
}
export function eventChord(event: KeyboardEvent): number[] | undefined {
  const k = keyInput(event);
  if (!k.keyCode || k.metaKey || [16, 17, 18].includes(k.keyCode)) return;
  return [
    ...(k.ctrlKey ? [17] : []),
    ...(k.shiftKey ? [16] : []),
    ...(k.altKey ? [18] : []),
    k.keyCode,
  ].sort((a, b) => a - b);
}
export function shortcutStatusText(row: ShortcutRow | undefined): string {
  if (!row) return '未启用';
  const status = row.status;
  if (status.kind === 'active') return '可用';
  if (status.kind === 'pending') return '正在启用…';
  if (status.kind === 'conflict') return '被其他应用占用';
  if (status.code === 5) return '录入期间暂停';
  if (status.code === 4) return '等待窗口连接';
  if (status.code === 2) return '系统不支持此按键';
  return '当前不可用';
}
