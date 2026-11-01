import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import type { MiniClient } from '../services/mini';
import { prepareMiniViewport } from '../services/miniViewport';
export const enterMiniPlayer = () => invoke<void>('mini_enter');
export const onMiniReturn = (handler: (value: { epoch: string; fm: boolean }) => void) =>
  listen<{ epoch: string; fm: boolean }>('cumulus:mini-return', (event) => handler(event.payload));
export function createMiniClient(generation: string): MiniClient {
  return {
    read: () => invoke('mini_read', { generation }),
    ready: () => prepareMiniViewport((viewport) => invoke('mini_ready', { generation, viewport })),
    returnMain: (fm = false) => invoke('mini_return', { generation, fm }),
    feedbackView: (ticket, refresh) =>
      invoke('mini_fm_feedback_view', { generation, ticket, refresh }),
    feedbackReasons: (ticket) => invoke('mini_fm_feedback_reasons', { generation, ticket }),
    feedbackSubmit: (ticket, reasonKey) =>
      invoke('mini_fm_feedback', { generation, ticket, reasonKey: reasonKey ?? null }),
    action: (ticket, action) => invoke('mini_action', { generation, ticket, action }),
    intelligence: (ticket, command) => invoke('mini_intelligence', { generation, ticket, command }),
    mode: (ticket, mode) => invoke('mini_mode', { generation, ticket, mode }),
    cancelIntelligence: (epoch, operationId) =>
      invoke('mini_intelligence_cancel', { generation, epoch, operationId }),
    queue: (ticket, offset) => invoke('mini_queue', { generation, ticket, offset }),
    panel: (expanded) => invoke('mini_panel', { generation, expanded, scale: devicePixelRatio }),
    preferences: (ticket, command) =>
      invoke('mini_preferences_command', { generation, ticket, command }),
    applyPreferences: (ticket, expectedRevision) =>
      invoke('mini_preferences_apply', { generation, ticket, expectedRevision }),
    lyricMenu: (ticket) => invoke('mini_lyric_menu_read', { generation, ticket }),
    lyricMenuAction: (menuTicket, action) =>
      invoke('mini_lyric_menu_action', { generation, menuTicket, action }),
    exit: () => invoke('mini_exit', { generation }),
    reportLyrics: (ticket) => invoke('mini_lyric_report', { generation, ticket }),
    drag: () => getCurrentWindow().startDragging(),
    close: () => getCurrentWindow().close(),
  };
}

export { createWindowShortcutClient as createMiniShortcutClient } from './windowShortcuts';
