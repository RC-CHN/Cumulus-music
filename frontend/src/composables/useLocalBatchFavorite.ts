import { computed, reactive, ref, shallowRef } from 'vue';
import { failure, type PlaylistView } from '../services/contracts.ts';
import type {
  LocalBatchPlayback,
  LocalCollectClient,
  LocalCollectPlan,
  LocalCollectSort,
} from '../services/local.ts';
import { playlistPending, playlistSaved } from '../services/playlists.ts';

const preference = 'cumulus.playlist-collect-sort.v1';
function rememberedSort(): LocalCollectSort {
  try {
    return localStorage.getItem(preference) === 'recently' ? 'recently' : 'default';
  } catch {
    return 'default';
  }
}
function newPage() {
  return {
    rows: [] as PlaylistView[],
    next: 0 as number | null,
    loading: false,
    error: '',
    top: 0,
  };
}

export function useLocalBatchFavorite(core: LocalCollectClient, live: (epoch: string) => boolean) {
  type Scope = LocalCollectPlan & { epoch: string };
  const scope = shallowRef<Scope>();
  const sort = ref<LocalCollectSort>(rememberedSort());
  const pages = reactive({ default: newPage(), recently: newPage() });
  const current = computed(() => pages[sort.value]);
  const preparing = ref(false);
  const busy = ref(false);
  const target = ref('');
  const pending = ref(false);
  const status = ref('');
  const error = ref('');
  const notice = ref('');
  let sequence = 0;
  const valid = (source: Scope) => scope.value === source && live(source.epoch);
  function release(epoch: string, receipt: string) {
    void core.localCollectCancel(epoch, receipt).catch(() => {});
  }

  function close() {
    const source = scope.value;
    ++sequence;
    scope.value = undefined;
    preparing.value = busy.value = pending.value = false;
    target.value = status.value = '';
    if (source) release(source.epoch, source.receipt);
  }

  async function open(epoch: string, selection: LocalBatchPlayback) {
    if (!live(epoch) || scope.value || preparing.value) return;
    preparing.value = true;
    error.value = notice.value = '';
    const request = ++sequence;
    try {
      const result = await core.localCollectPrepare(epoch, selection);
      if (request !== sequence || !live(epoch) || result.epoch !== epoch) {
        release(epoch, result.value.receipt);
        return;
      }
      Object.assign(pages, { default: newPage(), recently: newPage() });
      sort.value = rememberedSort();
      scope.value = { epoch, ...result.value };
      await load();
    } catch (cause) {
      if (request === sequence && live(epoch)) {
        const issue = failure(cause);
        error.value =
          issue.code === 'invalid_input'
            ? '请确认所选本地歌曲均已匹配到不同的目录歌曲（最多 100 首）。'
            : issue.message;
      }
    } finally {
      if (request === sequence) preparing.value = false;
    }
  }

  async function load(automatic = false) {
    const source = scope.value;
    const order = sort.value;
    const state = pages[order];
    const page = state.next;
    if (
      !source ||
      !valid(source) ||
      target.value ||
      state.loading ||
      page === null ||
      (automatic && state.error)
    )
      return;
    state.loading = true;
    try {
      const reply = await core.localCollectDestinations(source.epoch, source.receipt, order, page);
      if (!valid(source) || reply.epoch !== source.epoch || target.value) return;
      const known = new Set(state.rows.map((playlist) => playlist.id));
      state.rows = [...state.rows, ...reply.value.playlists.filter((row) => !known.has(row.id))];
      state.next = reply.value.nextPage;
      state.error = '';
    } catch (cause) {
      if (valid(source) && !target.value) state.error = failure(cause).message;
    } finally {
      if (valid(source)) state.loading = false;
    }
  }

  function changeSort(order: LocalCollectSort) {
    if (sort.value === order || target.value) return;
    sort.value = order;
    try {
      localStorage.setItem(preference, order);
    } catch {
      // The current picker can still retain this sort without storage.
    }
    if (current.value.next === 0 && !current.value.error) void load();
  }

  async function collect(id: string, reconcile = false) {
    const source = scope.value;
    if (!source || !valid(source) || busy.value) return;
    if (
      reconcile
        ? target.value !== id || !pending.value
        : target.value || !current.value.rows.some((row) => row.id === id)
    )
      return;
    target.value = id;
    busy.value = true;
    status.value = '';
    try {
      const reply = await core.localCollectExisting(source.epoch, source.receipt, id, reconcile);
      if (!valid(source) || reply.epoch !== source.epoch) return;
      if (playlistSaved(reply.value.phase)) {
        notice.value = '已收藏到歌单。';
        close();
        return;
      }
      pending.value = playlistPending(reply.value.phase);
      status.value = reply.value.duplicate
        ? '歌单内歌曲重复'
        : pending.value
          ? '收藏结果尚未确认，请核对结果。'
          : '此次收藏未完成。';
    } catch (cause) {
      if (!valid(source)) return;
      const issue = failure(cause);
      // A lost response may follow actual dispatch. The picker never offers
      // another write for this attempt; any follow-up is explicitly read-only.
      pending.value = ![
        'snapshot_changed',
        'stale',
        'invalid_input',
        'session_expired',
        'profile_changed',
        'write_uncertain',
        'request_not_sent',
        'busy',
      ].includes(issue.code);
      status.value = pending.value
        ? '收藏结果尚未确认，请核对结果。关闭后仍可在歌单操作记录中查看。'
        : issue.code === 'write_uncertain'
          ? '这个歌单已有尚未确认的操作，请在歌单操作记录中核对。'
          : issue.message;
    } finally {
      if (valid(source)) busy.value = false;
    }
  }

  return {
    scope,
    sort,
    current,
    preparing,
    busy,
    target,
    pending,
    status,
    error,
    notice,
    open,
    close,
    load,
    changeSort,
    collect,
  };
}
