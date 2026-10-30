import { ref, shallowRef } from 'vue';
import { failure, type AccountView, type CoreClient } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { coverTerminal, type CoverTask } from '../services/playlistCover.ts';
import {
  playlistSaved,
  type PlaylistCommand,
  type PlaylistEditor,
  type PlaylistMutationView,
} from '../services/playlists.ts';

/** View coordination only. Core journals/compares/writes/reconciles resources. */
export function usePlaylists(
  core: CoreClient = useCore(),
  changed: (result: PlaylistMutationView) => Promise<void> = async () => {},
  expired: () => void = () => {},
) {
  const editor = shallowRef<PlaylistEditor>();
  const operations = shallowRef<PlaylistMutationView[]>([]);
  const coverOperations = shallowRef<CoverTask[]>([]);
  const tags = shallowRef<{ name: string; category: string }[]>([]);
  const tagError = ref('');
  const tagsLoading = ref(false);
  const result = shallowRef<PlaylistMutationView>();
  const busy = ref(false),
    loading = ref(false),
    error = ref('');
  let epoch = '',
    revision = 0,
    read = 0,
    operationRead = 0,
    tagRead = 0,
    connected = false;
  function problem(e: unknown) {
    const v = failure(e);
    if (v.code !== 'stale') error.value = v.message;
    if (['session_expired', 'profile_changed'].includes(v.code)) expired();
  }
  function connect(account: AccountView) {
    epoch = account.epoch;
    revision++;
    read++;
    operationRead++;
    tagRead++;
    connected = account.status === 'authenticated';
    busy.value = false;
    loading.value = false;
    error.value = '';
    editor.value = undefined;
    result.value = undefined;
    operations.value = [];
    coverOperations.value = [];
    tags.value = [];
    tagError.value = '';
    tagsLoading.value = false;
    if (connected) void refreshOperations();
  }
  async function refreshOperations() {
    const e = epoch,
      n = ++operationRead;
    try {
      const [normal, covers] = await Promise.allSettled([
        core.playlistOperations(e),
        core.playlistCoverTasks(e),
      ]);
      if (!connected || e !== epoch || n !== operationRead) return;
      if (normal.status === 'fulfilled' && normal.value.epoch === e)
        operations.value = normal.value.value;
      else if (normal.status === 'rejected') problem(normal.reason);
      if (covers.status === 'fulfilled' && covers.value.epoch === e)
        coverOperations.value = covers.value.value.filter(
          (t) => !coverTerminal(t) || t.cleanupPending,
        );
      else if (covers.status === 'rejected') problem(covers.reason);
    } catch (x) {
      if (e === epoch && n === operationRead) problem(x);
    }
  }
  async function loadTags() {
    if (!connected) return;
    const e = epoch,
      n = ++tagRead;
    tagError.value = '';
    tagsLoading.value = true;
    try {
      const r = await core.playlistTags(e);
      if (e === epoch && n === tagRead && r.epoch === epoch) tags.value = r.value;
    } catch (x) {
      if (e === epoch && n === tagRead) tagError.value = failure(x).message;
    } finally {
      if (e === epoch && n === tagRead) tagsLoading.value = false;
    }
  }
  async function open(id: string) {
    const r = ++read,
      e = epoch;
    editor.value = undefined;
    result.value = undefined;
    error.value = '';
    loading.value = true;
    try {
      const v = await core.playlistEditor(e, id);
      if (r === read && e === epoch && v.epoch === epoch) editor.value = v.value;
    } catch (x) {
      if (r === read && e === epoch) problem(x);
    } finally {
      if (r === read) loading.value = false;
    }
  }
  function reset() {
    read++;
    editor.value = undefined;
    result.value = undefined;
    error.value = '';
    loading.value = false;
  }
  async function refreshAfterCover(
    expectedEpoch: string,
    id: string,
    observed?: PlaylistEditor | null,
  ) {
    if (!connected || epoch !== expectedEpoch || editor.value?.id !== id) return;
    const n = ++read;
    try {
      let value = observed;
      if (!value) {
        const response = await core.playlistEditor(expectedEpoch, id);
        if (response.epoch !== expectedEpoch) return;
        value = response.value;
      }
      if (connected && epoch === expectedEpoch && read === n && value.id === id)
        editor.value = value;
    } catch (value) {
      if (epoch === expectedEpoch && read === n) problem(value);
    }
    if (epoch === expectedEpoch) await refreshOperations();
  }
  async function run(
    operation: (
      e: string,
    ) => Promise<import('../services/contracts').AccountData<PlaylistMutationView>>,
  ) {
    if (!connected || busy.value) return;
    const e = epoch,
      r = ++revision;
    busy.value = true;
    error.value = '';
    result.value = undefined;
    try {
      const v = await operation(e);
      if (r !== revision || e !== epoch || v.epoch !== epoch) return;
      result.value = v.value;
      if (v.value.observed) editor.value = v.value.observed;
      await changed(v.value);
      if (r !== revision || e !== epoch) return;
      await refreshOperations();
      return v.value;
    } catch (x) {
      if (r === revision && e === epoch) {
        problem(x);
        await refreshOperations();
      }
    } finally {
      if (r === revision) busy.value = false;
    }
  }
  const mutate = (command: PlaylistCommand) => run((e) => core.mutatePlaylist(e, command));
  const reconcile = (operationId: string) => run((e) => core.reconcilePlaylist(e, operationId));
  async function tracks(
    id: string,
    change: 'add' | 'delete' | 'clear' | 'up' | 'down',
    trackId?: string,
  ) {
    return run(async (e) => {
      const current = await core.playlistEditor(e, id);
      if (current.epoch !== e || epoch !== e) throw { code: 'stale', message: '账号已切换' };
      const p = current.value;
      let ids = trackId ? [trackId] : [];
      let action: 'add' | 'delete' | 'clear' | 'reorder' =
        change === 'up' || change === 'down' ? 'reorder' : change;
      if (action === 'reorder') {
        ids = [...p.trackIds];
        const from = ids.indexOf(trackId!),
          to = from + (change === 'up' ? -1 : 1);
        if (from < 0 || to < 0 || to >= ids.length)
          throw { code: 'invalid_input', message: '这首歌已经在边界位置，或歌单已改变' };
        [ids[from], ids[to]] = [ids[to]!, ids[from]!];
      }
      return core.mutatePlaylist(e, {
        kind: 'tracks',
        id,
        revision: p.revision,
        change: action,
        ids,
      });
    });
  }
  async function move(id: string, owned: boolean, direction: -1 | 1, favoriteId?: string) {
    return run(async (e) => {
      const value = await core.playlistOrderView(e);
      if (value.epoch !== e || epoch !== e) throw { code: 'stale', message: '账号已切换' };
      const group = owned ? 'created' : 'subscribed';
      const ids = [...value.value[group]],
        from = ids.indexOf(id),
        to = from + direction;
      if (from < 0 || to < 0 || to >= ids.length || ids[to] === favoriteId)
        throw { code: 'invalid_input', message: '歌单已经在边界位置，或列表已改变' };
      [ids[from], ids[to]] = [ids[to]!, ids[from]!];
      return core.mutatePlaylist(e, { kind: 'order', group, revision: value.value.revision, ids });
    });
  }
  return {
    editor,
    operations,
    coverOperations,
    result,
    busy,
    loading,
    error,
    tags,
    tagError,
    tagsLoading,
    loadTags,
    connect,
    open,
    reset,
    refreshAfterCover,
    refreshOperations,
    mutate,
    reconcile,
    tracks,
    move,
    saved: playlistSaved,
  };
}
export type PlaylistManager = ReturnType<typeof usePlaylists>;
