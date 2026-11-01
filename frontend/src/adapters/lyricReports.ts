import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { LyricReportClient } from '../services/lyricReports';
export function createLyricReportClient(): LyricReportClient {
  return {
    open: (epoch, playId) => invoke('lyric_report_open', { epoch, playId }),
    read: (epoch, id) => invoke('lyric_report_read', { epoch, id }),
    types: (epoch, id, revision) => invoke('lyric_report_types', { epoch, id, revision }),
    submit: (epoch, id, revision, key, content) =>
      invoke('lyric_report_submit', { epoch, id, revision, key, content }),
    abandon: (epoch, id, revision) => invoke('lyric_report_abandon', { epoch, id, revision }),
  };
}
export const onLyricReportOpen = (handle: (value: { epoch: string; id: string }) => void) =>
  listen<{ epoch: string; id: string }>('cumulus:lyric-report-open', (event) =>
    handle(event.payload),
  );
