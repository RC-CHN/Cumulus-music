import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
/** Listen only on this WebView; leases are additionally scoped by native window identity. */
export function createWindowShortcutClient(): import('../services/shortcuts').ShortcutKeysClient {
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
