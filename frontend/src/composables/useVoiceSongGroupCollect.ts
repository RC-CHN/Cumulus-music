import { computed, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import { playlistPending, playlistSaved } from '../services/playlists.ts';
import type {
  VoiceSongCollectCard,
  VoiceSongCollectClient,
  VoiceSongCollectPlan,
  VoiceSongCollectResult,
  VoiceSongCollectTarget,
  VoiceSongLikedCollectClient,
  VoiceSongLikedDestination,
} from '../services/voiceSongs.ts';

export interface VoiceSongCollectOpening {
  card: VoiceSongCollectCard;
  indices: readonly number[];
  live: () => boolean;
}

type Phase = 'preparing' | 'picking' | 'saving' | 'pending' | 'reading' | 'failed' | 'empty';

export function useVoiceSongGroupCollect(
  core: VoiceSongCollectClient & VoiceSongLikedCollectClient,
  expired: () => void,
  completed: () => void,
) {
  const opening = shallowRef<VoiceSongCollectOpening>();
  const visible = shallowRef(false);
  const phase = shallowRef<Phase>('preparing');
  const plan = shallowRef<VoiceSongCollectPlan>();
  const target = shallowRef<VoiceSongCollectTarget>();
  const selected = shallowRef('');
  const operation = shallowRef('');
  const message = shallowRef('');
  const liked = shallowRef<VoiceSongLikedDestination>();
  const likedLoading = shallowRef(false);
  const likedError = shallowRef('');
  const saving = computed(() => phase.value === 'saving' || phase.value === 'reading');

  function reset() {
    visible.value = false;
    opening.value = undefined;
    target.value = undefined;
    plan.value = undefined;
    selected.value = '';
    operation.value = '';
    message.value = '';
    liked.value = undefined;
    likedLoading.value = false;
    likedError.value = '';
    phase.value = 'preparing';
  }
  watch(
    () => opening.value?.live() ?? true,
    (live) => {
      if (!live) reset();
    },
    { flush: 'sync' },
  );
  function owns(view: VoiceSongCollectOpening | undefined) {
    return !!view && view === opening.value && view.live();
  }
  function close() {
    if (saving.value) return;
    // A still-live pending receipt remains available to reopen and reconcile.
    // Once the card changes, the normal playlist journal owns recovery.
    if (phase.value === 'pending' && owns(opening.value)) visible.value = false;
    else reset();
  }
  function issue(error: unknown) {
    const problem = failure(error);
    if (['session_expired', 'profile_changed'].includes(problem.code)) expired();
    return problem;
  }
  async function open(intent: VoiceSongCollectOpening) {
    if (!intent.live() || visible.value || saving.value) return;
    if (phase.value === 'pending' && owns(opening.value)) {
      visible.value = true;
      return;
    }
    reset();
    const view = { ...intent, card: { ...intent.card }, indices: [...intent.indices] };
    opening.value = view;
    visible.value = true;
    try {
      const reply = await core.playerVoiceSongsCollectPrepare(view.card, view.indices);
      if (!owns(view)) return;
      if (
        reply.epoch !== view.card.epoch ||
        reply.playId !== view.card.playId ||
        reply.selectedCount !== view.indices.length ||
        reply.eligibleCount < 0 ||
        reply.eligibleCount + reply.omittedIndices.length !== reply.selectedCount ||
        !!reply.selectionReceipt !== reply.eligibleCount > 0
      ) {
        phase.value = 'failed';
        message.value = '所选歌曲已变化，请重新选择';
        return;
      }
      plan.value = reply;
      if (reply.selectionReceipt) {
        target.value = { ...view.card, selectionReceipt: reply.selectionReceipt };
        phase.value = 'picking';
        void readLiked();
      } else {
        phase.value = 'empty';
        message.value = '所选歌曲没有可加入歌单的曲目';
      }
    } catch (error) {
      if (!owns(view)) return;
      phase.value = 'failed';
      message.value = issue(error).message;
    }
  }
  async function readLiked() {
    const view = opening.value;
    const destination = target.value;
    if (!owns(view) || !destination || phase.value !== 'picking' || likedLoading.value) return;
    liked.value = undefined;
    likedError.value = '';
    likedLoading.value = true;
    const current = () => owns(view) && destination === target.value;
    try {
      const reply = await core.playerVoiceSongsCollectLikedDestination(destination);
      if (!current() || phase.value !== 'picking') return;
      if (reply.epoch !== destination.epoch) {
        likedError.value = '喜欢歌单已变化，请重试';
        return;
      }
      liked.value = reply.value ?? undefined;
    } catch (error) {
      if (!current() || phase.value !== 'picking') return;
      const problem = issue(error);
      if (current()) likedError.value = problem.message;
    } finally {
      if (current()) likedLoading.value = false;
    }
  }
  function settle(view: VoiceSongCollectOpening, result: VoiceSongCollectResult) {
    const destination = target.value;
    if (!owns(view) || !destination) return;
    if (
      result.epoch !== destination.epoch ||
      result.playId !== destination.playId ||
      result.selectionReceipt !== destination.selectionReceipt ||
      result.playlistId !== selected.value ||
      !result.operationId ||
      (operation.value && result.operationId !== operation.value)
    ) {
      phase.value = 'pending';
      message.value = '无法确认此次收藏，请核对结果';
      return;
    }
    operation.value = result.operationId;
    if (playlistSaved(result.phase)) {
      reset();
      completed();
    } else if (playlistPending(result.phase)) {
      phase.value = 'pending';
      message.value = '收藏操作待核对';
    } else {
      phase.value = 'failed';
      message.value = '所选歌曲未能收藏到这个歌单';
    }
  }
  function choose(playlistId: string) {
    return chooseDestination(playlistId, (destination) =>
      core.playerVoiceSongsCollectExisting(destination, playlistId),
    );
  }
  function chooseLiked() {
    const row = liked.value;
    if (!row || likedLoading.value) return;
    return chooseDestination(row.playlistId, (destination) =>
      core.playerVoiceSongsCollectLiked(destination, row.destinationReceipt),
    );
  }
  async function chooseDestination(
    playlistId: string,
    write: (destination: VoiceSongCollectTarget) => Promise<VoiceSongCollectResult>,
  ) {
    const view = opening.value;
    const destination = target.value;
    if (!view || !owns(view) || !destination || phase.value !== 'picking') return;
    selected.value = playlistId;
    phase.value = 'saving';
    message.value = '';
    try {
      settle(view, await write(destination));
    } catch (error) {
      if (!owns(view)) return;
      const problem = issue(error);
      try {
        // Read only the native journal here. Only the explicit button below
        // may ask the service to reconcile; never send a second add.
        settle(view, await core.playerVoiceSongsCollectStatus(destination, false));
      } catch {
        if (!owns(view)) return;
        const mayHaveWritten = [
          'connection',
          'network',
          'mutation_uncertain',
          'write_uncertain',
        ].includes(problem.code);
        phase.value = mayHaveWritten ? 'pending' : 'failed';
        message.value = problem.message;
      }
    }
  }
  async function reconcile() {
    const view = opening.value;
    const destination = target.value;
    if (!view || !owns(view) || !destination || phase.value !== 'pending') return;
    phase.value = 'reading';
    try {
      settle(view, await core.playerVoiceSongsCollectStatus(destination, true));
    } catch (error) {
      if (!owns(view)) return;
      phase.value = 'pending';
      message.value = issue(error).message;
    }
  }
  function destinations(sort: 'default' | 'recently', page: number) {
    const destination = target.value;
    if (phase.value === 'picking' && owns(opening.value) && destination)
      return core.playerVoiceSongsCollectDestinations(destination, sort, page);
  }
  return {
    visible,
    phase,
    plan,
    selected,
    message,
    saving,
    liked,
    likedLoading,
    likedError,
    open,
    close,
    choose,
    chooseLiked,
    readLiked,
    reconcile,
    destinations,
  };
}

export type VoiceSongGroupCollectModel = ReturnType<typeof useVoiceSongGroupCollect>;
