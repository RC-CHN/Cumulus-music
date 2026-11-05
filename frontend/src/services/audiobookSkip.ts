import { shallowRef } from 'vue';
import type { AccountData } from './contracts.ts';
import type { PodcastScope } from './podcast.ts';

export type AudiobookSkipContext =
  | { kind: 'detail'; scope: Extract<PodcastScope, { kind: 'radio' }>; revision: string }
  | { kind: 'player'; playId: string; entryId: string };

export type AudiobookSkipSetting =
  | { kind: 'close' }
  | { kind: 'smart' }
  | { kind: 'custom'; start: number; end: number };

export type AudiobookSkipAction =
  | { kind: 'open' }
  | { kind: 'smart'; editId: string }
  | { kind: 'save'; editId: string; setting: AudiobookSkipSetting };

export interface AudiobookSkipView {
  radioId: string;
  editId: string;
  setting: AudiobookSkipSetting;
  smartAvailable: boolean;
}

export interface AudiobookSkipClient {
  audiobookSkipSettings(
    epoch: string,
    context: AudiobookSkipContext,
    action: AudiobookSkipAction,
  ): Promise<AccountData<AudiobookSkipView>>;
}

export const audiobookSkipSaved = shallowRef<AccountData<AudiobookSkipView>>();

export const MAX_SKIP_SECONDS = 90;
type Kind = AudiobookSkipSetting['kind'];
export interface SkipDraft {
  kind: Kind;
  start: number;
  endRail: number;
  manualTouched: boolean;
}

export function audiobookSkipDraft(setting: AudiobookSkipSetting): SkipDraft {
  return {
    kind: setting.kind,
    start: setting.kind === 'custom' ? setting.start : 0,
    endRail: setting.kind === 'custom' ? MAX_SKIP_SECONDS - setting.end : MAX_SKIP_SECONDS,
    manualTouched: false,
  };
}

export function chooseAudiobookSkip(draft: SkipDraft, kind: Kind): SkipDraft {
  return { ...draft, kind, manualTouched: false };
}

export function moveAudiobookSkip(draft: SkipDraft, rail: 'start' | 'endRail', value: number) {
  if (draft.kind !== 'custom' || !Number.isFinite(value)) return draft;
  return {
    ...draft,
    [rail]: Math.max(0, Math.min(MAX_SKIP_SECONDS, Math.round(value))),
    manualTouched: true,
  };
}

export function audiobookSkipSetting(draft: SkipDraft): AudiobookSkipSetting | undefined {
  if (draft.kind !== 'custom') return { kind: draft.kind };
  if (draft.manualTouched)
    return { kind: 'custom', start: draft.start, end: MAX_SKIP_SECONDS - draft.endRail };
  return undefined;
}
