import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { MiniClient } from '../services/mini';
import { prepareMiniViewport } from '../services/miniViewport';
export const enterMiniPlayer = () => invoke<void>('mini_enter');
export function createMiniClient(generation: string): MiniClient {
  return {
    read: () => invoke('mini_read', { generation }),
    ready: () => prepareMiniViewport((viewport) => invoke('mini_ready', { generation, viewport })),
    returnMain: () => invoke('mini_return', { generation }),
    action: (ticket, action) => invoke('mini_action', { generation, ticket, action }),
    intelligence: (ticket, command) => invoke('mini_intelligence', { generation, ticket, command }),
    mode: (ticket, mode) => invoke('mini_mode', { generation, ticket, mode }),
    cancelIntelligence: (epoch, operationId) =>
      invoke('mini_intelligence_cancel', { generation, epoch, operationId }),
    queue: (ticket, offset) => invoke('mini_queue', { generation, ticket, offset }),
    panel: (expanded) => invoke('mini_panel', { generation, expanded, scale: devicePixelRatio }),
    preferences: (ticket, command) =>
      invoke('mini_preferences_command', { generation, ticket, command }),
    drag: () => getCurrentWindow().startDragging(),
    close: () => getCurrentWindow().close(),
  };
}

export { createWindowShortcutClient as createMiniShortcutClient } from './windowShortcuts';
