import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import { sameVoicePlaybackScope } from '../services/voiceDetail.ts';
import type {
  VinylVoiceDetailView,
  VoicePageClient,
  VoicePageScope,
} from '../services/voicePage.ts';
import type { VoiceRecommendationsPage } from '../services/voiceRecommendations.ts';
import type { VoiceSongsView } from '../services/voiceSongs.ts';
import { useVoiceRecommendationActions } from './useVoiceRecommendations.ts';
import { voiceSongActions } from './useVoiceSongs.ts';

interface VoicePageSnapshot {
  detail: VinylVoiceDetailView;
  recommendations: VoiceRecommendationsPage;
  songs: VoiceSongsView;
}

type VoicePageState =
  | { status: 'idle' | 'loading' | 'failed' }
  | { status: 'ready'; snapshot: VoicePageSnapshot };

/** M69.initVoicePage: start the three reads together, then expose one current visit. */
export function useVoicePage(
  api: VoicePageClient,
  scope: Readonly<Ref<VoicePageScope | undefined>>,
) {
  const generation = ref(0);
  const state = shallowRef<VoicePageState>({ status: 'idle' });
  const status = computed(() => state.value.status);
  const snapshot = computed(() =>
    state.value.status === 'ready' ? state.value.snapshot : undefined,
  );
  const detail = computed(() => snapshot.value?.detail);
  // Retain the existing action-target ABI. Entry identity is an extra fence
  // for the visit, not an extra renderer argument to the receipt commands.
  const actionScope = computed(() => {
    const current = scope.value;
    if (current) return { epoch: current.epoch, playId: current.playId, voiceId: current.voiceId };
  });
  const key = computed(() => {
    const current = scope.value;
    return (
      current && JSON.stringify([current.epoch, current.playId, current.entryId, current.voiceId])
    );
  });
  const recommendations = useVoiceRecommendationActions(api, {
    scope: actionScope,
    generation,
    view: computed(() => snapshot.value?.recommendations),
    replace(recommendations) {
      const current = snapshot.value;
      if (current) state.value = { status: 'ready', snapshot: { ...current, recommendations } };
    },
  });
  const songs = voiceSongActions(
    actionScope,
    computed(() => snapshot.value?.songs),
  );

  function invalidate() {
    state.value = { status: 'idle' };
    generation.value++;
  }
  function live(ticket: number, current: VoicePageScope) {
    return (
      generation.value === ticket &&
      scope.value?.entryId === current.entryId &&
      sameVoicePlaybackScope(scope.value, current)
    );
  }
  async function load(ticket: number) {
    const current = scope.value;
    if (!current) return;
    const { epoch, playId, entryId, voiceId } = current;
    state.value = { status: 'loading' };
    try {
      const [detail, recommendations, songs] = await Promise.all([
        api.playerVinylVoiceDetail(epoch, playId),
        api.playerVinylVoiceRecommendations(epoch, playId),
        api.playerVoiceSongs(epoch, playId),
      ]);
      if (!live(ticket, current)) return;
      if (
        detail.epoch !== epoch ||
        detail.entryId !== entryId ||
        detail.playId !== playId ||
        detail.voiceId !== voiceId ||
        detail.detail.id !== voiceId ||
        recommendations.playId !== playId ||
        recommendations.voiceId !== voiceId ||
        songs.playId !== playId ||
        songs.voiceId !== voiceId ||
        songs.tracks.length !== songs.availability.length
      ) {
        state.value = { status: 'failed' };
        return;
      }
      state.value = { status: 'ready', snapshot: { detail, recommendations, songs } };
    } catch {
      // One failure cannot expose two successful lanes, nor trigger a retry.
      if (live(ticket, current)) state.value = { status: 'failed' };
    }
  }
  watch(key, invalidate, { flush: 'sync' });
  watch(generation, (ticket) => void load(ticket), { immediate: true });
  onScopeDispose(invalidate);
  return { status, snapshot, detail, recommendations, songs };
}

export type VoicePageModel = ReturnType<typeof useVoicePage>;
