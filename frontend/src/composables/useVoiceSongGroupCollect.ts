import { computed, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import { playlistLength, playlistPending, playlistSaved } from '../services/playlists.ts';
import type {
  VoiceSongCollectCard,
  VoiceSongCollectClient,
  VoiceSongCollectCreation,
  VoiceSongCollectCreationCommand,
  VoiceSongCollectCreationReply,
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

type Phase =
  | 'preparing'
  | 'picking'
  | 'creating'
  | 'created_ready'
  | 'saving'
  | 'pending'
  | 'reading'
  | 'failed'
  | 'empty';

export function validVoiceSongCollectionName(name: string) {
  const value = name.trim();
  return !!value && !/[@#]/.test(value) && playlistLength(value) <= 40;
}

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
  const newDestination = shallowRef(false);
  const creation = shallowRef<VoiceSongCollectCreation>();
  const saving = computed(() => phase.value === 'saving' || phase.value === 'reading');
  const canCreate = computed(
    () =>
      phase.value === 'picking' &&
      plan.value?.canCreate === true &&
      !!target.value &&
      owns(opening.value),
  );

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
    newDestination.value = false;
    creation.value = undefined;
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
    if (['pending', 'created_ready'].includes(phase.value) && owns(opening.value)) {
      visible.value = false;
      return;
    }
    const destination = target.value;
    if (phase.value === 'creating' && destination && owns(opening.value))
      void core
        .playerVoiceSongsCollectCreateAction(destination, { kind: 'cancel_prepared' })
        .catch(() => {});
    reset();
  }
  function issue(error: unknown) {
    const problem = failure(error);
    if (['session_expired', 'profile_changed'].includes(problem.code)) expired();
    return problem;
  }
  async function open(intent: VoiceSongCollectOpening) {
    if (!intent.live() || visible.value || saving.value) return;
    if (['pending', 'created_ready'].includes(phase.value) && owns(opening.value)) {
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
        !!reply.selectionReceipt !== reply.eligibleCount > 0 ||
        (reply.canCreate && !reply.selectionReceipt)
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
    if (newDestination.value) return creationAction('pending', { kind: 'status', reconcile: true });
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
  function chooseCreate() {
    if (!visible.value || !canCreate.value) return;
    newDestination.value = true;
    phase.value = 'creating';
  }
  function create(name: string, privatePlaylist: boolean) {
    if (!validVoiceSongCollectionName(name) || typeof privatePlaylist !== 'boolean') return;
    return creationAction('creating', { kind: 'create', name: name.trim(), privatePlaylist });
  }
  function addCreated() {
    if (!creation.value?.canAdd) return;
    return creationAction('created_ready', { kind: 'add_created' });
  }
  function settleCreation(view: VoiceSongCollectOpening, reply: VoiceSongCollectCreationReply) {
    const destination = target.value;
    if (!owns(view) || !destination) return;
    const result = reply.kind === 'creation' ? reply.creation : undefined;
    const previous = creation.value;
    const add = result?.add;
    if (
      !result ||
      result.epoch !== destination.epoch ||
      result.playId !== destination.playId ||
      result.selectionReceipt !== destination.selectionReceipt ||
      !result.createOperationId ||
      (previous && previous.createOperationId !== result.createOperationId) ||
      (previous?.playlistId && previous.playlistId !== result.playlistId) ||
      (previous?.add && previous.add.operationId !== add?.operationId) ||
      (result.playlistId !== null && result.createPhase !== 'confirmed') ||
      result.canAdd !== (result.playlistId !== null && !add) ||
      (result.addFailed && !add) ||
      (add &&
        (add.epoch !== destination.epoch ||
          add.playId !== destination.playId ||
          add.selectionReceipt !== destination.selectionReceipt ||
          add.playlistId !== result.playlistId ||
          !add.operationId ||
          add.operationId === result.createOperationId))
    ) {
      phase.value = 'pending';
      message.value = '无法确认此次创建或收藏，请核对结果';
      return;
    }
    creation.value = result;
    if (result.canAdd) {
      phase.value = 'created_ready';
      message.value = '已确认新歌单，可以继续收藏所选歌曲。';
    } else if (result.addFailed) {
      phase.value = 'failed';
      message.value = '歌单已创建，但所选歌曲未能收藏。';
    } else if (add && playlistSaved(add.phase)) {
      reset();
      completed();
    } else if (playlistPending(result.createPhase) || (add && playlistPending(add.phase))) {
      phase.value = 'pending';
      message.value = add ? '歌单已创建，收藏结果待核对。' : '歌单创建结果待核对。';
    } else {
      phase.value = 'failed';
      message.value = add
        ? '歌单已创建，但所选歌曲未能收藏。'
        : result.createPhase === 'confirmed'
          ? '已核对创建结果，但没有可确认的新歌单编号，未收藏歌曲。'
          : '未能创建歌单，也未收藏歌曲。';
    }
  }
  async function creationAction(
    from: 'creating' | 'created_ready' | 'pending',
    command: VoiceSongCollectCreationCommand,
  ) {
    const view = opening.value;
    const destination = target.value;
    if (
      !view ||
      !owns(view) ||
      !destination ||
      !visible.value ||
      !newDestination.value ||
      phase.value !== from
    )
      return;
    phase.value = command.kind === 'status' ? 'reading' : 'saving';
    message.value = command.kind === 'create' ? '正在创建歌单并收藏…' : '正在收藏所选歌曲…';
    try {
      settleCreation(view, await core.playerVoiceSongsCollectCreateAction(destination, command));
    } catch (error) {
      if (!owns(view)) return;
      const problem = issue(error);
      if (!owns(view)) return;
      if (command.kind !== 'status') {
        try {
          // A transport error is never a reason to submit a second write.
          settleCreation(
            view,
            await core.playerVoiceSongsCollectCreateAction(destination, {
              kind: 'status',
              reconcile: false,
            }),
          );
          return;
        } catch {
          if (!owns(view)) return;
        }
      }
      const pending =
        command.kind === 'status' ||
        ['connection', 'network', 'mutation_uncertain', 'write_uncertain'].includes(problem.code);
      phase.value = pending ? 'pending' : 'failed';
      message.value = problem.message;
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
    newDestination,
    canCreate,
    open,
    close,
    choose,
    chooseLiked,
    chooseCreate,
    create,
    addCreated,
    readLiked,
    reconcile,
    destinations,
  };
}

export type VoiceSongGroupCollectModel = ReturnType<typeof useVoiceSongGroupCollect>;
