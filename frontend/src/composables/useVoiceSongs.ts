import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import { sameVoicePlaybackScope } from '../services/voiceDetail.ts';
import type {
  VoiceSongRowTarget,
  VoiceSongsClient,
  VoiceSongsScope,
  VoiceSongsView,
} from '../services/voiceSongs.ts';

export function useVoiceSongs(
  api: Pick<VoiceSongsClient, 'playerVoiceSongs'>,
  scope: Readonly<Ref<VoiceSongsScope | undefined>>,
) {
  const key = computed(() => {
    const current = scope.value;
    return current && JSON.stringify([current.epoch, current.playId, current.voiceId]);
  });
  const generation = ref(0);
  const view = shallowRef<VoiceSongsView>();

  function invalidate() {
    generation.value++;
    view.value = undefined;
  }
  watch(key, invalidate, { flush: 'sync' });
  watch(
    generation,
    async (ticket) => {
      const current = scope.value;
      if (!current) return;
      try {
        const reply = await api.playerVoiceSongs(current.epoch, current.playId);
        if (
          ticket === generation.value &&
          sameVoicePlaybackScope(scope.value, current) &&
          reply.playId === current.playId &&
          reply.voiceId === current.voiceId &&
          reply.tracks.length === reply.availability.length
        )
          view.value = reply;
      } catch {
        // Optional card; close/reopen starts a new official current-episode read.
      }
    },
    { immediate: true },
  );
  onScopeDispose(invalidate);
  return voiceSongActions(scope, view);
}

export function voiceSongActions(
  scope: Readonly<Ref<VoiceSongsScope | undefined>>,
  view: Readonly<Ref<VoiceSongsView | undefined>>,
) {
  function contains(page: VoiceSongsView, index: number) {
    return Number.isInteger(index) && index >= 0 && index < page.tracks.length;
  }
  function target(page: VoiceSongsView, index: number): VoiceSongRowTarget | undefined {
    const current = scope.value;
    if (!current || view.value !== page || !page.receipt || !contains(page, index)) return;
    return { ...current, receipt: page.receipt, index };
  }
  function ownsTarget(target: VoiceSongRowTarget) {
    const page = view.value;
    return (
      !!page &&
      page.receipt === target.receipt &&
      sameVoicePlaybackScope(scope.value, target) &&
      contains(page, target.index)
    );
  }
  return { view, target, ownsTarget };
}

export type VoiceSongsModel = ReturnType<typeof voiceSongActions>;
