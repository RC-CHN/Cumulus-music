import { computed, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import { playlistPending } from '../services/playlists.ts';
import type {
  VoiceSongGroupDestinationCommand,
  VoiceSongGroupDownloadClient,
  VoiceSongGroupDownloadTarget,
  VoiceSongGroupPlaylistCreation,
  VoiceSongGroupPlaylistResult,
  VoiceSongLikedDestination,
} from '../services/voiceSongs.ts';

type Phase = 'pick' | 'create' | 'saving' | 'reading' | 'pending' | 'created_ready' | 'failed';

interface Picker {
  target: VoiceSongGroupDownloadTarget;
  live: () => boolean;
  start: (continuation: string, addFailed: boolean) => Promise<void>;
  retired: () => void;
  tail: Promise<unknown>;
}

export function useVoiceSongGroupDestination(
  core: VoiceSongGroupDownloadClient,
  expired: () => void,
) {
  const picker = shallowRef<Picker>();
  const visible = shallowRef(false);
  const phase = shallowRef<Phase>('pick');
  const message = shallowRef('');
  const liked = shallowRef<VoiceSongLikedDestination>();
  const likedLoading = shallowRef(false);
  const likedError = shallowRef('');
  let retired: Promise<void> | undefined;
  let selected = '';
  let operation = '';
  let creating = false;
  let creation: VoiceSongGroupPlaylistCreation | undefined;
  const busy = computed(() => phase.value === 'saving' || phase.value === 'reading');
  const owns = (owner: Picker) => picker.value === owner && owner.live();

  // All of these commands borrow the same exclusive native selection lease.
  // Queue reads in this visit; never issue a queued call after it was retired.
  function call(owner: Picker, command: VoiceSongGroupDestinationCommand) {
    const reply = owner.tail.then(() => {
      if (!owns(owner)) throw new Error('这次下载选择已失效');
      return core.playerVoiceSongsDownloadDestination(owner.target, command);
    });
    owner.tail = reply.catch(() => {});
    return reply;
  }

  function retire(cancel: boolean) {
    const owner = picker.value;
    picker.value = undefined;
    visible.value = false;
    if (owner && cancel)
      retired = owner.tail
        .then(() => core.playerVoiceSongsDownloadCancel(owner.target))
        .catch(() => {});
  }

  function open(owner: Omit<Picker, 'tail'>) {
    picker.value = { ...owner, tail: Promise.resolve() };
    phase.value = 'pick';
    message.value = '';
    selected = '';
    operation = '';
    creating = false;
    creation = undefined;
    liked.value = undefined;
    likedLoading.value = false;
    likedError.value = '';
    visible.value = true;
    void readLiked();
  }

  function close() {
    const owner = picker.value;
    if (!owner || busy.value) return;
    // Keep uncertain/created operations accessible without another mutation.
    if (phase.value === 'pending' || phase.value === 'created_ready') visible.value = false;
    else {
      retire(true);
      owner.retired();
    }
  }

  function reopen() {
    if (picker.value && owns(picker.value)) visible.value = true;
  }

  function issue(error: unknown) {
    const problem = failure(error);
    if (problem.code === 'session_expired' || problem.code === 'profile_changed') expired();
    return problem.message;
  }

  function page(sort: 'default' | 'recently', number: number) {
    const owner = picker.value;
    if (!owner || !owns(owner) || !visible.value || phase.value !== 'pick') return;
    return call(owner, { kind: 'page', sort, page: number }).then((reply) => {
      if (reply.kind !== 'page' || reply.page.epoch !== owner.target.epoch)
        throw new Error('歌单列表已失效，请重试');
      return reply.page.value;
    });
  }

  async function readLiked() {
    const owner = picker.value;
    if (!owner || !owns(owner) || !visible.value || phase.value !== 'pick' || likedLoading.value)
      return;
    likedLoading.value = true;
    likedError.value = '';
    liked.value = undefined;
    try {
      const reply = await call(owner, { kind: 'liked_destination' });
      if (!owns(owner)) return;
      if (reply.kind !== 'liked_destination' || reply.destination.epoch !== owner.target.epoch)
        throw new Error('喜欢歌单已变化，请重试');
      liked.value = reply.destination.value ?? undefined;
    } catch (error) {
      if (owns(owner) && phase.value === 'pick') likedError.value = issue(error);
    } finally {
      if (owns(owner)) likedLoading.value = false;
    }
  }

  function exact(
    owner: Picker,
    reply: Pick<VoiceSongGroupPlaylistResult, 'epoch' | 'playId' | 'selectionReceipt'>,
  ) {
    return (
      reply.epoch === owner.target.epoch &&
      reply.playId === owner.target.playId &&
      reply.selectionReceipt === owner.target.selectionReceipt
    );
  }

  async function settlePlaylist(owner: Picker, result: VoiceSongGroupPlaylistResult) {
    if (!owns(owner)) return;
    if (
      !exact(owner, result) ||
      !selected ||
      result.playlistId !== selected ||
      !result.operationId ||
      (operation && result.operationId !== operation)
    ) {
      phase.value = 'pending';
      message.value = '无法确认此次歌单操作，请核对结果。';
      return;
    }
    operation = result.operationId;
    if (result.continuation) {
      // Only the native journal can issue this one continuation. Closing the
      // picker never sends an add or starts a download.
      retire(false);
      await owner.start(result.continuation, result.addFailed);
    } else if (playlistPending(result.phase)) {
      phase.value = 'pending';
      message.value = '歌单操作待核对。';
    } else {
      phase.value = 'failed';
      message.value = '这次未能将歌曲加入歌单，没有添加下载任务。';
    }
  }

  async function settleCreation(owner: Picker, result: VoiceSongGroupPlaylistCreation) {
    if (!owns(owner)) return;
    if (
      !exact(owner, result) ||
      !result.createOperationId ||
      (creation &&
        (creation.createOperationId !== result.createOperationId ||
          (creation.playlistId && creation.playlistId !== result.playlistId))) ||
      (result.canAdd && (!result.playlistId || result.add))
    ) {
      phase.value = 'pending';
      message.value = '无法确认此次创建，请核对结果。';
      return;
    }
    creation = result;
    if (result.add && result.playlistId) {
      selected = result.playlistId;
      await settlePlaylist(owner, result.add);
    } else if (result.canAdd) {
      phase.value = 'created_ready';
      message.value = '歌单已创建。请将歌曲加入该歌单后继续下载。';
    } else if (playlistPending(result.createPhase)) {
      phase.value = 'pending';
      message.value = '歌单创建结果待核对。';
    } else {
      phase.value = 'failed';
      message.value = '没有确认新歌单已接收歌曲，没有添加下载任务。';
    }
  }

  async function apply(command: VoiceSongGroupDestinationCommand, from: Phase) {
    const owner = picker.value;
    if (!owner || !owns(owner) || !visible.value || phase.value !== from) return;
    const status: VoiceSongGroupDestinationCommand = creating
      ? { kind: 'creation_status', reconcile: false }
      : { kind: 'status', reconcile: false };
    const reading = command.kind === 'status' || command.kind === 'creation_status';
    phase.value = reading ? 'reading' : 'saving';
    message.value = reading ? '正在核对结果…' : '正在处理歌单…';
    try {
      let reply;
      try {
        reply = await call(owner, command);
      } catch (error) {
        if (!owns(owner)) return;
        if (reading) throw error;
        // Check only the local journal after a lost IPC response. The
        // explicit reconcile button below is the only remote read-back.
        try {
          reply = await call(owner, status);
        } catch {
          throw error;
        }
      }
      if (!owns(owner)) return;
      if (reply.kind === 'playlist' && !creating) await settlePlaylist(owner, reply.playlist);
      else if (reply.kind === 'creation' && creating) await settleCreation(owner, reply.creation);
      else throw new Error('未能确认歌单操作，请核对结果。');
    } catch (error) {
      if (!owns(owner)) return;
      phase.value = 'pending';
      message.value = issue(error);
    }
  }

  function choose(id: string) {
    if (phase.value !== 'pick') return;
    selected = id;
    return apply({ kind: 'choose', playlistId: id }, 'pick');
  }

  function chooseLiked() {
    const row = liked.value;
    if (!row || phase.value !== 'pick') return;
    selected = row.playlistId;
    return apply({ kind: 'choose_liked', destinationReceipt: row.destinationReceipt }, 'pick');
  }

  function openCreate() {
    if (!visible.value || phase.value !== 'pick') return;
    creating = true;
    phase.value = 'create';
  }

  function back() {
    if (phase.value !== 'create') return;
    creating = false;
    phase.value = 'pick';
  }

  return {
    open,
    close,
    reopen,
    retire,
    drained: () => retired,
    page,
    readLiked,
    choose,
    chooseLiked,
    openCreate,
    back,
    visible,
    phase,
    message,
    liked,
    likedLoading,
    likedError,
    busy,
    recoverable: computed(
      () => !!picker.value && (phase.value === 'pending' || phase.value === 'created_ready'),
    ),
    create: (name: string, privatePlaylist: boolean) =>
      apply({ kind: 'create', name, privatePlaylist }, 'create'),
    addCreated: () => creation?.canAdd && apply({ kind: 'add_created' }, 'created_ready'),
    reconcile: () =>
      apply({ kind: creating ? 'creation_status' : 'status', reconcile: true }, 'pending'),
  };
}

export type VoiceSongGroupDestinationModel = ReturnType<typeof useVoiceSongGroupDestination>;
