import { invoke } from '@tauri-apps/api/core';
import type { FontClient, InterfaceFontClient } from '../services/fonts';
export const createFontClient = (): FontClient => ({
  read: (epoch) => invoke('system_font_catalog', { epoch }),
});
export const createInterfaceFontClient = (): InterfaceFontClient => ({
  read: () => invoke('interface_font'),
  write: (family) => invoke('set_interface_font', { family: family ?? '' }),
  catalog: (epoch) => invoke('interface_font_catalog', { epoch }),
});
