import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { MiniClient } from '../services/mini';
export const enterMiniPlayer = () => invoke<void>('mini_enter');
export function createMiniClient(generation: string): MiniClient {
  return {
    read: () => invoke('mini_read', { generation }),
    ready: () => invoke('mini_ready', { generation }),
    returnMain: () => invoke('mini_return', { generation }),
    action: (ticket, action) => invoke('mini_action', { generation, ticket, action }),
    queue: (ticket, offset) => invoke('mini_queue', { generation, ticket, offset }),
    panel: (expanded) => invoke('mini_panel', { generation, expanded }),
    preferences: (ticket, command) =>
      invoke('mini_preferences_command', { generation, ticket, command }),
    drag: () => getCurrentWindow().startDragging(),
    close: () => getCurrentWindow().close(),
  };
}

/** Listen only on this WebView; leases are additionally scoped by native window identity. */
export function createMiniShortcutClient(): import('../services/shortcuts').ShortcutKeysClient {
  const window = getCurrentWindow();
  return {
    shortcutStatus: () => invoke('shortcut_status'),
    shortcutAttach: (epoch, ticket) => invoke('shortcut_attach', { epoch, ticket }),
    shortcutDetach: (lease) => invoke('shortcut_detach', { lease }),
    shortcutPress: (press) => invoke('shortcut_press', { press }),
    onShortcuts: (handler) =>
      window.listen<import('../services/shortcuts').ShortcutStatus>('cumulus:shortcuts', (e) =>
        handler(e.payload),
      ),
    onShortcutError: (handler) =>
      window.listen<{ epoch: string; error: import('../services/contracts').CoreFailure }>(
        'cumulus:shortcut-error',
        (e) => handler(e.payload),
      ),
  };
}
