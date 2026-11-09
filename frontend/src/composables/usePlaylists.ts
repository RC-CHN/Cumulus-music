import { computed, ref, shallowRef } from 'vue';
import {
  failure,
  type AccountView,
  type CoreClient,
  type CoreFailure,
} from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { coverTerminal, type CoverTask } from '../services/playlistCover.ts';
import { sidebarMoveOrder, type SidebarPlaylistMove } from '../services/sidebar.ts';
import {
  playlistSaved,
  type PlaylistCommand,
  type PlaylistEditor,
  type PlaylistMutationView,
} from '../services/playlists.ts';

/** View coordination only. Core journals/compares/writes/reconciles resources. */
export function usePlaylists(
  core: CoreClient = useCore(),
  changed: (result: PlaylistMutationView, epoch: string) => Promise<void> = async () => {},
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
    loading = ref(false);
  const issue = shallowRef<CoreFailure>();
  const error = computed(() => issue.value?.message ?? '');
  let epoch = '',
    revision = 0,
    read = 0,
    operationRead = 0,
    tagRead = 0,
    connected = false;
  function problem(e: unknown) {
    const v = failure(e);
    if (v.code !== 'stale') issue.value = v;
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
    issue.value = undefined;
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
    issue.value = undefined;
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
    issue.value = undefined;
    loading.value = false;
  }
  async function refreshEditor(
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
      if (connected && epoch === expectedEpoch && read === n && value.id === id) {
        if (issue.value?.code === 'snapshot_changed') issue.value = undefined;
        editor.value = value;
        return value;
      }
    } catch (value) {
      if (epoch === expectedEpoch && read === n) problem(value);
    }
  }
  async function refreshAfterCover(
    expectedEpoch: string,
    id: string,
    observed?: PlaylistEditor | null,
  ) {
    if (!connected || epoch !== expectedEpoch || editor.value?.id !== id) return;
    const latest = await refreshEditor(expectedEpoch, id, observed);
    if (epoch === expectedEpoch) await refreshOperations();
    return latest;
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
    issue.value = undefined;
    result.value = undefined;
    try {
      const v = await operation(e);
      if (r !== revision || e !== epoch || v.epoch !== epoch) return;
      result.value = v.value;
      if (v.value.observed) editor.value = v.value.observed;
      await changed(v.value, e);
      if (r !== revision || e !== epoch) return;
      await refreshOperations();
      return v.value;
    } catch (x) {
      if (r === revision && e === epoch) {
        problem(x);
        await refreshOperations();
        if (r === revision && e === epoch && failure(x).code === 'snapshot_changed') problem(x);
      }
    } finally {
      if (r === revision) busy.value = false;
    }
  }
  const mutate = (command: PlaylistCommand) => run((e) => core.mutatePlaylist(e, command));
  const reconcile = (operationId: string) => run((e) => core.reconcilePlaylist(e, operationId));
  const addDownloadedTrack = (scope: { epoch: string; receipt: string }, id: string) =>
    run((e) => {
      if (scope.epoch !== e) throw { code: 'stale', message: '账号已切换' };
      return core.downloadCollectExisting(e, scope.receipt, id);
    });
  const collectDownloadedLiked = (
    scope: { epoch: string; receipt: string },
    destination: string,
    reconcile: boolean,
  ) =>
    run((e) => {
      if (scope.epoch !== e) throw { code: 'stale', message: '账号已切换' };
      return core.downloadCollectLiked(e, scope.receipt, destination, reconcile);
    });
  async function addTracks(id: string, ids: string[]) {
    return run(async (e) => {
      const current = await core.playlistEditor(e, id);
      if (current.epoch !== e || epoch !== e) throw { code: 'stale', message: '账号已切换' };
      return core.mutatePlaylist(e, {
        kind: 'tracks',
        id,
        revision: current.value.revision,
        change: 'add',
        ids: [...new Set(ids)],
      });
    });
  }
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
  async function reorderSidebar(move: SidebarPlaylistMove, favoriteId?: string) {
    const desired = sidebarMoveOrder(move);
    if (!desired) return;
    return run(async (e) => {
      const value = await core.playlistOrderView(e);
      if (value.epoch !== e || epoch !== e) throw { code: 'stale', message: '账号已切换' };
      const { group } = move;
      const native = value.value[group];
      const fixed = group === 'created' ? favoriteId : undefined;
      const visible = native.filter((id) => id !== fixed);
      if (visible.length !== move.ids.length || visible.some((id, index) => id !== move.ids[index]))
        throw { code: 'snapshot_changed', message: '歌单列表已变化，请刷新后重新拖动' };
      let index = 0;
      const ids = native.map((id) => (id === fixed ? id : desired[index++]!));
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
    issue,
    tags,
    tagError,
    tagsLoading,
    loadTags,
    connect,
    open,
    reset,
    refreshAfterCover,
    refreshEditor,
    refreshOperations,
    mutate,
    reconcile,
    tracks,
    addTracks,
    addDownloadedTrack,
    collectDownloadedLiked,
    move,
    reorderSidebar,
    saved: playlistSaved,
  };
}
export type PlaylistManager = ReturnType<typeof usePlaylists>;
