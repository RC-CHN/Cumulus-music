import { computed, reactive, ref, watch } from 'vue';
import type { AccountView } from '../services/contracts';
import {
  messageTabs,
  type MessageClient,
  type MessageCounts,
  type MessageQuery,
  type MessageRow,
  type MessageTab,
} from '../services/messages';

const createList = () => ({
  rows: [] as MessageRow[],
  cursor: null as number | null,
  hint: null as string | null,
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
  const counts = ref<MessageCounts>();
  const countsBusy = ref(false);
  const countsError = ref(false);
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
    counts.value = undefined;
    countsBusy.value = countsError.value = false;
  }
  watch(() => [account().epoch, account().status] as const, close, { flush: 'sync' });

  async function readCounts() {
    if (!shown.value || countsBusy.value || account().status !== 'authenticated') return;
    const epoch = account().epoch;
    const generation = visit;
    countsBusy.value = true;
    countsError.value = false;
    try {
      const result = await core.messageCounts(epoch);
      if (generation === visit && result.epoch === epoch && account().epoch === epoch)
        counts.value = result.value;
    } catch (error) {
      if (generation !== visit || account().epoch !== epoch) return;
      countsError.value = true;
      if ((error as { code?: string })?.code === 'session_expired') expired();
    } finally {
      if (generation === visit) countsBusy.value = false;
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
      target.rows = rows.slice(0, MAX_ROWS);
      target.cursor = result.value.nextCursor;
      target.capped = rows.length > MAX_ROWS || (rows.length === MAX_ROWS && !!target.cursor);
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
    void readCounts();
    void load();
  }
  function select(next: MessageTab) {
    tab.value = next;
    if (!lists[next].loaded && !lists[next].error) void load();
  }
  function enter(item: MessageRow) {
    if (!shown.value || tab.value !== 'private' || peer.value || !item.peerId) return;
    threadVisit++;
    thread.value = createList();
    peer.value = { id: item.peerId, name: item.author };
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
    open,
    close,
    readCounts,
    load,
    select,
    enter,
    back,
  };
}
