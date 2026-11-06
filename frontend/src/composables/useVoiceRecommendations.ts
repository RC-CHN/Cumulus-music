import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import {
  voiceRecommendationMatches,
  type VoiceRecommendationsClient,
  type VoiceRecommendationsPage,
  type VoiceRecommendationsSeed,
  type VoiceRecommendationTarget,
} from '../services/voiceRecommendations.ts';

export function useVoiceRecommendations(
  api: Pick<
    VoiceRecommendationsClient,
    'playerVinylVoiceRecommendations' | 'playerVinylVoiceRecommendationsNext'
  >,
  seed: Readonly<Ref<VoiceRecommendationsSeed | undefined>>,
) {
  const scope = computed(() => {
    const current = seed.value;
    return current && JSON.stringify([current.epoch, current.playId, current.voiceId]);
  });
  const view = shallowRef<VoiceRecommendationsPage>();
  const generation = ref(0);
  function invalidate() {
    generation.value++;
    view.value = undefined;
  }
  async function load() {
    const current = seed.value;
    if (!current) return;
    const ticket = generation.value;
    try {
      const reply = await api.playerVinylVoiceRecommendations(current.epoch, current.playId);
      if (
        ticket === generation.value &&
        voiceRecommendationMatches(seed.value, current) &&
        replyMatches(reply, current)
      )
        view.value = reply;
    } catch {
      // This optional shelf remains absent. The next panel visit can read it again.
    }
  }
  const actions = useVoiceRecommendationActions(api, {
    scope: seed,
    generation,
    view,
    replace: (page) => (view.value = page),
  });
  watch(scope, invalidate, { flush: 'sync' });
  watch(generation, () => void load(), { immediate: true });
  onScopeDispose(invalidate);
  return actions;
}

function replyMatches(reply: VoiceRecommendationsPage, current: VoiceRecommendationsSeed) {
  return reply.playId === current.playId && reply.voiceId === current.voiceId;
}

// Used both by the independent shelf and by the joint VoicePage snapshot.
export function useVoiceRecommendationActions(
  api: Pick<VoiceRecommendationsClient, 'playerVinylVoiceRecommendationsNext'>,
  {
    scope: seed,
    generation,
    view,
    replace,
  }: {
    scope: Readonly<Ref<VoiceRecommendationsSeed | undefined>>;
    generation: Readonly<Ref<number>>;
    view: Readonly<Ref<VoiceRecommendationsPage | undefined>>;
    replace(page: VoiceRecommendationsPage): void;
  },
) {
  const paging = ref(false);
  const clearPaging = () => (paging.value = false);
  watch(generation, clearPaging, { flush: 'sync' });
  onScopeDispose(clearPaging);
  function live(ticket: number, current: VoiceRecommendationsSeed) {
    return ticket === generation.value && voiceRecommendationMatches(seed.value, current);
  }
  async function next() {
    const current = seed.value;
    const previous = view.value;
    if (!current || !previous?.cards.length || paging.value) return;
    const ticket = generation.value;
    paging.value = true;
    try {
      const reply = await api.playerVinylVoiceRecommendationsNext(
        current.epoch,
        current.playId,
        previous.receipt,
      );
      if (
        live(ticket, current) &&
        view.value === previous &&
        replyMatches(reply, current) &&
        reply.receipt === previous.receipt
      )
        replace(reply);
    } catch {
      // No request is retried and the previously displayed page remains intact.
    } finally {
      if (live(ticket, current)) paging.value = false;
    }
  }
  function target(page: VoiceRecommendationsPage, radioId: string) {
    const current = seed.value;
    if (current && view.value === page && page.cards.some((card) => card.radioId === radioId)) {
      return { ...current, receipt: page.receipt, page: page.page, radioId };
    }
  }
  function ownsTarget(target: VoiceRecommendationTarget) {
    const page = view.value;
    return (
      voiceRecommendationMatches(seed.value, target) &&
      page?.receipt === target.receipt &&
      page.page === target.page &&
      page.cards.some((card) => card.radioId === target.radioId)
    );
  }
  return { view, paging, next, target, ownsTarget };
}

export type VoiceRecommendationsModel = ReturnType<typeof useVoiceRecommendationActions>;
