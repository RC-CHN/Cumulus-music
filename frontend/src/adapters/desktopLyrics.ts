import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { DesktopLyricClient, LyricSettingsClient } from '../services/desktopLyrics';
export const createLyricSettingsClient = (): LyricSettingsClient => ({
  read: (epoch) => invoke('desktop_lyric_settings', { epoch }),
  change: (epoch, command) => invoke('desktop_lyric_change', { epoch, command }),
  window: (epoch, action) => invoke('desktop_lyric_window_command', { epoch, action }),
});
export const createDesktopLyricClient = (generation: string): DesktopLyricClient => ({
  ready: () => invoke('desktop_lyric_ready', { generation }),
  read: () => invoke('desktop_lyric_read', { generation }),
  action: (ticket, action) => invoke('desktop_lyric_action', { generation, ticket, action }),
  drag: () => getCurrentWindow().startDragging(),
  close: () => getCurrentWindow().close(),
  resize: (edge) => getCurrentWindow().startResizeDragging(edge),
});
export function onDesktopLyricOpen(
  handler: (value: { epoch: string; playId: string | null; settings: boolean }) => void,
) {
  return getCurrentWindow().listen<{ epoch: string; playId: string | null; settings: boolean }>(
    'cumulus:desktop-lyric-open',
    (event) => handler(event.payload),
  );
}
