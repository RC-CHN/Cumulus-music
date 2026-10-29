import { invoke } from '@tauri-apps/api/core';
import type { FontClient } from '../services/fonts';
export const createFontClient = (): FontClient => ({
  read: (epoch) => invoke('system_font_catalog', { epoch }),
});
