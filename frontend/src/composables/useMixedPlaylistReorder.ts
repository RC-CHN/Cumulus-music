import { computed, ref, shallowRef, watch } from 'vue';
import { failure, type AccountData, type CoreFailure } from '../services/contracts.ts';
import type {
  PlaylistClient,
  PlaylistMixedOrderRow,
  PlaylistMixedReorderReceipt,
} from '../services/playlists';

export type MixedReorderVisit = Readonly<{ id: string; epoch: string }>;

type MixedTransport<T> = {
  read: PlaylistClient['playlistMixedReorderReceipt'];
  verify: PlaylistClient['reconcilePlaylistMixedReorder'];
  send: (
    epoch: string,
    id: string,
    revision: string,
    selected: PlaylistMixedOrderRow[],
    detail: T,
  ) => Promise<AccountData<PlaylistMixedReorderReceipt>>;
};
type MixedCopy = {
  saved: string;
  refresh: string;
  rejected: string;
  unreadable: string;
  changed: string;
  uncertain: string;
};

function useMixedPlaylistMutation<T>(
  core: MixedTransport<T>,
  scope: () => MixedReorderVisit | undefined,
  refresh: (visit: MixedReorderVisit, focusId?: string) => Promise<boolean>,
  expired: () => void,
  copy: MixedCopy,
) {
  const receipt = shallowRef<PlaylistMixedReorderReceipt | null>(null);
  const known = ref(false);
  const busy = ref<'read' | 'send' | 'verify'>();
  const message = ref('');
  const needsRefresh = ref(false);
  const pending = computed(
    () => receipt.value?.phase === 'sending' || receipt.value?.phase === 'uncertain',
  );
  const ready = computed(() => known.value && !pending.value && !busy.value && !needsRefresh.value);
  const active = (visit: MixedReorderVisit) => scope() === visit;

  function sessionEnded(issue: CoreFailure) {
    if (issue.code !== 'session_expired' && issue.code !== 'profile_changed') return false;
    expired();
    return true;
  }

  function accept<T extends PlaylistMixedReorderReceipt | null>(
    response: AccountData<T>,
    visit: MixedReorderVisit,
  ): T {
    if (
      response.epoch !== visit.epoch ||
      (response.value && response.value.playlistId !== visit.id)
    )
      throw { code: 'stale', message: '当前歌单或账号已变化' };
    return response.value;
  }

  async function read(visit = scope()) {
    if (!visit || !active(visit) || busy.value) return;
    busy.value = 'read';
    message.value = '';
    try {
      const response = await core.read(visit.epoch, visit.id);
      if (!active(visit)) return;
      receipt.value = accept(response, visit);
      known.value = true;
    } catch (error) {
      if (!active(visit)) return;
      known.value = false;
      const issue = failure(error);
      if (!sessionEnded(issue)) message.value = issue.message;
    } finally {
      if (active(visit)) busy.value = undefined;
    }
  }

  watch(
    scope,
    (visit) => {
      receipt.value = null;
      needsRefresh.value = false;
      known.value = false;
      busy.value = undefined;
      message.value = '';
      if (visit) void read(visit);
    },
    { immediate: true, flush: 'sync' },
  );

  async function settle(
    visit: MixedReorderVisit,
    value: PlaylistMixedReorderReceipt,
    focusId?: string,
  ) {
    receipt.value = value;
    known.value = true;
    if (value.phase === 'confirmed') {
      const updated = await refresh(visit, focusId);
      if (active(visit)) {
        needsRefresh.value = !updated;
        message.value = updated ? copy.saved : copy.refresh;
      }
    } else if (value.phase === 'rejected') {
      message.value = copy.rejected;
    }
  }

  async function send(
    revision: string,
    selected: PlaylistMixedOrderRow[],
    detail: T,
    focusId?: string,
  ) {
    const visit = scope();
    if (!visit || !ready.value) return;
    busy.value = 'send';
    message.value = '';
    try {
      const response = await core.send(visit.epoch, visit.id, revision, selected, detail);
      if (active(visit)) await settle(visit, accept(response, visit), focusId);
    } catch (error) {
      if (!active(visit)) return;
      const original = failure(error);
      if (sessionEnded(original)) return;
      // IPC can fail after native has durably journaled the send. Only reread
      // that receipt; never synthesize a rejection or replay the gesture.
      try {
        const stored = await core.read(visit.epoch, visit.id);
        if (!active(visit)) return;
        const value = accept(stored, visit);
        const prior = receipt.value?.operationId;
        receipt.value = value;
        known.value = true;
        if (value && value.operationId !== prior) await settle(visit, value, focusId);
        else message.value = original.message;
      } catch {
        if (!active(visit)) return;
        known.value = false;
        message.value = copy.unreadable;
      }
    } finally {
      if (active(visit)) busy.value = undefined;
    }
  }

  async function verify() {
    const visit = scope();
    const operationId = receipt.value?.operationId;
    if (!visit || !operationId || busy.value || !pending.value) return;
    busy.value = 'verify';
    message.value = '';
    try {
      const response = await core.verify(visit.epoch, visit.id, operationId);
      if (!active(visit)) return;
      const value = accept(response, visit);
      if (value.operationId !== operationId) throw { code: 'stale', message: copy.changed };
      await settle(visit, value);
      if (active(visit) && pending.value) message.value = copy.uncertain;
    } catch (error) {
      const issue = failure(error);
      if (active(visit) && !sessionEnded(issue)) message.value = issue.message;
    } finally {
      if (active(visit)) busy.value = undefined;
    }
  }

  async function retryRefresh() {
    const visit = scope();
    if (!visit || !needsRefresh.value || busy.value) return;
    busy.value = 'read';
    try {
      if ((await refresh(visit)) && active(visit)) {
        needsRefresh.value = false;
        message.value = copy.saved;
      }
    } finally {
      if (active(visit)) busy.value = undefined;
    }
  }

  return { busy, known, pending, ready, message, needsRefresh, read, send, verify, retryRefresh };
}

export function useMixedPlaylistReorder(
  core: Pick<
    PlaylistClient,
    'playlistMixedReorderReceipt' | 'movePlaylistMixed' | 'reconcilePlaylistMixedReorder'
  >,
  scope: () => MixedReorderVisit | undefined,
  refresh: (visit: MixedReorderVisit, focusId?: string) => Promise<boolean>,
  expired: () => void = () => {},
) {
  return useMixedPlaylistMutation<number>(
    {
      read: core.playlistMixedReorderReceipt,
      verify: core.reconcilePlaylistMixedReorder,
      send: core.movePlaylistMixed,
    },
    scope,
    refresh,
    expired,
    {
      saved: '歌单顺序已更新。',
      refresh: '排序已确认。重新读取歌单可查看新顺序。',
      rejected: '歌单排序未完成，原本机顺序已保留。',
      unreadable: '暂时无法读取排序回执。',
      changed: '排序记录已变化',
      uncertain: '仍无法确认这次排序。',
    },
  );
}

export function useMixedPlaylistRemove(
  core: Pick<
    PlaylistClient,
    'playlistMixedRemoveReceipt' | 'removePlaylistMixed' | 'reconcilePlaylistMixedRemove'
  >,
  scope: () => MixedReorderVisit | undefined,
  refresh: (visit: MixedReorderVisit) => Promise<boolean>,
  expired: () => void = () => {},
) {
  const mutation = useMixedPlaylistMutation<undefined>(
    {
      read: core.playlistMixedRemoveReceipt,
      verify: core.reconcilePlaylistMixedRemove,
      send: core.removePlaylistMixed,
    },
    scope,
    refresh,
    expired,
    {
      saved: '已从歌单中删除。',
      refresh: '删除已确认。重新读取歌单可查看最新歌曲。',
      rejected: '未能完成删除，原本机成员已保留。',
      unreadable: '暂时无法读取删除回执。',
      changed: '删除记录已变化',
      uncertain: '仍无法确认这次删除。',
    },
  );
  return {
    ...mutation,
    send: (revision: string, selected: PlaylistMixedOrderRow[]) =>
      mutation.send(revision, selected, undefined),
  };
}
