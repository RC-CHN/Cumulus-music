import { computed, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../services/contracts';
import {
  messageTabs,
  type MessageClient,
  type MessageCountStream,
  type MessageCountView,
  type MessageQuery,
  type MessageRow,
  type MessageTab,
} from '../services/messages';

const createList = () => ({
  rows: [] as MessageRow[],
  cursor: null as number | null,
  hint: null as string | null,
  replyReceipt: undefined as string | undefined,
  loaded: false,
  busy: false,
  error: false,
  errorMode: 'first' as 'first' | 'more',
  capped: false,
});
const MAX_ROWS = 300;

export function useMessageDrawer(
  core: MessageClient,
  account: () => AccountView,
  expired: () => void,
) {
  const shown = ref(false);
  const tab = ref<MessageTab>('private');
  const peer = ref<{ id: string; name: string }>();
  const lists = reactive({
    private: createList(),
    comments: createList(),
    forwards: createList(),
    notices: createList(),
  });
  const thread = ref(createList());
  const current = computed(() => (peer.value ? thread.value : lists[tab.value]));
  const nativeCounts = shallowRef<MessageCountView>();
  const currentCounts = computed(() => {
    const owner = account();
    return owner.status === 'authenticated' && owner.epoch === nativeCounts.value?.epoch
      ? nativeCounts.value
      : undefined;
  });
  const counts = computed(() => currentCounts.value?.counts);
  const refreshing = ref<string>();
  const streamFailure = ref<string>();
  const countsBusy = computed(
    () => currentCounts.value?.refreshing || refreshing.value === account().epoch,
  );
  const countsError = computed(
    () => currentCounts.value?.failed || streamFailure.value === account().epoch,
  );
  let countStream: MessageCountStream | undefined;
  let disposed = false;
  let countVisibility = false;
  let sequence = 0;

  function syncCountVisibility() {
    const owner = account();
    if (!countStream || owner.status !== 'authenticated') return;
    void countStream.visibility(owner.epoch, ++sequence, countVisibility).catch(() => {});
  }

  function setCountVisibility(visible: boolean) {
    if (countVisibility === visible) return;
    countVisibility = visible;
    syncCountVisibility();
  }

  onMounted(async () => {
    try {
      const stream = await core.watchMessageCounts((view) => {
        if (disposed) return;
        nativeCounts.value = view;
        if (!view.failed) streamFailure.value = undefined;
      });
      if (disposed) return stream.close();
      countStream = stream;
      syncCountVisibility();
    } catch {
      if (!disposed && account().status === 'authenticated') streamFailure.value = account().epoch;
    }
  });
  onBeforeUnmount(() => {
    disposed = true;
    countStream?.close();
  });
  let visit = 0;
  let threadVisit = 0;

  function close() {
    visit++;
    threadVisit++;
    shown.value = false;
    peer.value = undefined;
    thread.value = createList();
    tab.value = 'private';
    for (const key of messageTabs) lists[key] = createList();
  }
  watch(() => [account().epoch, account().status] as const, close, { flush: 'sync' });

  async function readCounts() {
    if (!shown.value || countsBusy.value || account().status !== 'authenticated') return;
    const epoch = account().epoch;
    refreshing.value = epoch;
    streamFailure.value = undefined;
    try {
      if (!countStream) throw new Error('message count stream unavailable');
      await countStream.refresh(epoch);
    } catch {
      if (account().epoch === epoch) streamFailure.value = epoch;
    } finally {
      if (refreshing.value === epoch) refreshing.value = undefined;
    }
  }

  async function load(mode: 'first' | 'more' = 'first') {
    const owner = account();
    const target = current.value;
    if (!shown.value || owner.status !== 'authenticated' || target.busy) return false;
    const cursor = mode === 'more' ? target.cursor : null;
    if (mode === 'more' && (!cursor || target.capped)) return false;
    const generation = visit;
    const talkGeneration = threadVisit;
    const query: MessageQuery = peer.value
      ? { tab: 'thread', peerId: peer.value.id, cursor }
      : { tab: tab.value, cursor };
    const valid = () =>
      generation === visit &&
      owner.epoch === account().epoch &&
      (query.tab !== 'thread' || talkGeneration === threadVisit);
    target.busy = true;
    target.error = false;
    try {
      const result = await core.messagePage(owner.epoch, query);
      if (!valid() || result.epoch !== owner.epoch) return false;
      const rows = mode === 'more' ? target.rows.slice() : [];
      const seen = new Set(rows.map((item) => item.id));
      for (const row of result.value.rows) {
        if (seen.has(row.id)) continue;
        seen.add(row.id);
        rows.push(row);
      }
      // An ambiguous write may have been applied. A fresh bounded read
      // cannot establish this, so preserve unresolved rows during this visit.
      if (mode === 'first') {
        for (const row of target.rows.filter((row) => row.deletionPending)) {
          if (!seen.has(row.id)) rows.push(row);
        }
      }
      target.rows = rows.slice(0, MAX_ROWS);
      target.cursor = result.value.nextCursor;
      target.capped = rows.length > MAX_ROWS || (rows.length === MAX_ROWS && !!target.cursor);
      if (query.tab === 'thread' && result.value.replyReceipt)
        target.replyReceipt = result.value.replyReceipt;
      target.hint = result.value.hint || (mode === 'more' ? target.hint : null);
      target.loaded = true;
      return true;
    } catch (error) {
      if (valid()) {
        target.error = true;
        target.errorMode = mode;
        if ((error as { code?: string })?.code === 'session_expired') expired();
      }
      return false;
    } finally {
      if (valid()) target.busy = false;
    }
  }

  function open() {
    if (shown.value || account().status !== 'authenticated') return;
    shown.value = true;
    void load();
  }
  function select(next: MessageTab) {
    tab.value = next;
    if (!lists[next].loaded && !lists[next].error) void load();
  }
  function enter(item: MessageRow) {
    if (!shown.value || tab.value !== 'private' || peer.value || !item.peerId) return;
    const rows = lists.private.rows;
    const index = rows.findIndex((row) => row.id === item.id && row.peerId === item.peerId);
    const row = rows[index];
    if (!row) return;
    if (row.unread) rows[index] = { ...row, unread: 0 };
    threadVisit++;
    thread.value = createList();
    peer.value = { id: item.peerId, name: row.author };
    return load();
  }
  function back() {
    threadVisit++;
    peer.value = undefined;
    thread.value = createList();
  }
  return {
    shown,
    tab,
    peer,
    current,
    counts,
    countsBusy,
    countsError,
    setCountVisibility,
    open,
    close,
    readCounts,
    load,
    select,
    enter,
    back,
  };
}
