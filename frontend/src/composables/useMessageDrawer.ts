import { computed, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../services/contracts';
import { mergePrivateInbox } from './messageInbox.ts';
import {
  messageTabs,
  type MessageClient,
  type MessageCountNavigation,
  type MessageCountStream,
  type MessageCountView,
  type MessageQuery,
  type MessageRow,
  type MessageTab,
} from '../services/messages.ts';

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
  const hidePrivateRowBadges = computed(() => currentCounts.value?.hidePrivateRowBadges === true);
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
  let countActions = Promise.resolve();
  let visit = 0;
  let threadVisit = 0;
  let inboxPresence = 0;
  let inboxRevision = 0;
  let inboxSyncQueued = false;
  let inboxCountAtRead: readonly [number | null | undefined, boolean] | undefined;
  const inboxSyncBlocked = ref(false);
  const deletingInbox = ref(false);

  function queueCountAction(
    send: (stream: MessageCountStream, epoch: string, sequence: number) => Promise<void>,
  ) {
    const owner = account();
    const stream = countStream;
    if (!stream || owner.status !== 'authenticated') return;
    const order = ++sequence;
    countActions = countActions
      .then(() => {
        if (!disposed && countStream === stream && account().epoch === owner.epoch)
          return send(stream, owner.epoch, order);
      })
      .catch(() => {});
  }

  function syncCountVisibility() {
    const visible = countVisibility;
    queueCountAction((stream, epoch, order) => stream.visibility(epoch, order, visible));
  }

  function countNavigated(navigation: MessageCountNavigation) {
    const generation = visit;
    // The official tab clears its host count on the next turn, after the
    // selected view changes. Returning to private or leaving a conversation
    // rearms the same counter without clearing the private total.
    setTimeout(() => {
      if (disposed || generation !== visit || !shown.value || !countVisibility) return;
      queueCountAction((stream, epoch, order) => stream.navigate(epoch, order, navigation));
    }, 0);
  }

  function setCountVisibility(visible: boolean) {
    if (countVisibility === visible) return;
    countVisibility = visible;
    if (!visible) leavePrivateInbox();
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
  function close() {
    visit++;
    threadVisit++;
    leavePrivateInbox();
    inboxCountAtRead = undefined;
    deletingInbox.value = false;
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

  function leavePrivateInbox() {
    inboxPresence++;
    inboxSyncQueued = false;
  }

  function privateInboxVisible() {
    return (
      shown.value &&
      countVisibility &&
      currentCounts.value?.drawerVisible === true &&
      tab.value === 'private' &&
      !peer.value
    );
  }

  function drainInboxSync() {
    if (
      !inboxSyncQueued ||
      !privateInboxVisible() ||
      inboxSyncBlocked.value ||
      deletingInbox.value ||
      lists.private.busy
    )
      return;
    inboxSyncQueued = false;
    void load('first', true);
  }

  function requestInboxSync() {
    if (!privateInboxVisible()) return;
    inboxSyncQueued = true;
    drainInboxSync();
  }

  // A settled count value change (including zero) rereads the first inbox
  // page only while it is mounted and visible. One queued read survives an
  // in-flight page, but a hidden/retired view never starts it.
  watch(
    [() => currentCounts.value?.drawerVisible, () => counts.value?.private, hidePrivateRowBadges],
    ([visible, count, hidden], [wasVisible]) => {
      if (!visible) {
        if (wasVisible && countVisibility) leavePrivateInbox();
        return;
      }
      if (inboxCountAtRead?.[0] !== count || inboxCountAtRead?.[1] !== hidden) requestInboxSync();
    },
    { flush: 'post' },
  );
  watch([() => lists.private.busy, inboxSyncBlocked, deletingInbox], drainInboxSync, {
    flush: 'post',
  });

  function blockInboxSync(blocked: boolean) {
    inboxSyncBlocked.value = blocked;
  }

  function beginInboxDeletion(scope: object) {
    if (scope !== lists.private) return () => {};
    const generation = visit;
    inboxRevision++;
    deletingInbox.value = true;
    return () => {
      if (generation === visit) deletingInbox.value = false;
    };
  }

  async function load(mode: 'first' | 'more' = 'first', automatic = false) {
    const owner = account();
    const target = current.value;
    if (!shown.value || owner.status !== 'authenticated' || target.busy) return false;
    const cursor = mode === 'more' ? target.cursor : null;
    if (mode === 'more' && (!cursor || target.capped)) return false;
    const generation = visit;
    const talkGeneration = threadVisit;
    const inboxGeneration = inboxRevision;
    const presence = inboxPresence;
    const query: MessageQuery = peer.value
      ? { tab: 'thread', peerId: peer.value.id, cursor }
      : { tab: tab.value, cursor };
    if (query.tab === 'private' && deletingInbox.value) return false;
    const countAtStart =
      query.tab === 'private' && mode === 'first'
        ? ([counts.value?.private, hidePrivateRowBadges.value] as const)
        : undefined;
    if (countAtStart) inboxCountAtRead = countAtStart;
    const owned = () =>
      generation === visit &&
      owner.epoch === account().epoch &&
      (query.tab !== 'thread' || talkGeneration === threadVisit);
    const valid = () =>
      owned() &&
      (query.tab !== 'private' ||
        (inboxGeneration === inboxRevision && (!automatic || presence === inboxPresence)));
    target.busy = true;
    target.error = false;
    try {
      const result = await core.messagePage(owner.epoch, query);
      if (!valid() || result.epoch !== owner.epoch) return false;
      let rows: MessageRow[];
      let cursor = result.value.nextCursor;
      if (query.tab === 'private' && mode === 'first') {
        ({ rows, cursor } = mergePrivateInbox(target.rows, result.value.rows, cursor));
      } else {
        rows = mode === 'more' ? target.rows.slice() : [];
        const seen = new Set(rows.map((item) => item.id));
        for (const row of result.value.rows) {
          if (seen.has(row.id)) continue;
          seen.add(row.id);
          rows.push(row);
        }
        // A bounded read cannot settle an ambiguous write by omission.
        if (mode === 'first') {
          for (const row of target.rows.filter((row) => row.deletionPending)) {
            if (!seen.has(row.id)) rows.push(row);
          }
        }
      }
      target.rows = rows.slice(0, MAX_ROWS);
      target.cursor = cursor;
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
      if (countAtStart && inboxCountAtRead === countAtStart && !valid())
        inboxCountAtRead = undefined;
      if (owned()) target.busy = false;
    }
  }

  function open() {
    if (shown.value || account().status !== 'authenticated') return;
    shown.value = true;
    void load();
  }
  function select(next: MessageTab) {
    if (!shown.value || peer.value || next === tab.value) return;
    if (tab.value === 'private') leavePrivateInbox();
    tab.value = next;
    countNavigated(next);
    if (next === 'private') requestInboxSync();
    else if (!lists[next].loaded && !lists[next].error) void load();
  }
  function enter(item: MessageRow) {
    if (!shown.value || tab.value !== 'private' || peer.value || !item.peerId) return;
    const rows = lists.private.rows;
    const index = rows.findIndex((row) => row.id === item.id && row.peerId === item.peerId);
    const row = rows[index];
    if (!row) return;
    if (row.unread) rows[index] = { ...row, unread: 0 };
    leavePrivateInbox();
    threadVisit++;
    thread.value = createList();
    peer.value = { id: item.peerId, name: row.author };
    countNavigated('conversation');
    return load();
  }
  function back() {
    if (!shown.value || !peer.value) return;
    threadVisit++;
    peer.value = undefined;
    thread.value = createList();
    countNavigated('conversation');
    requestInboxSync();
  }
  function clearConversationBadges() {
    const rows = lists.private.rows;
    for (const [index, row] of rows.entries()) if (row.unread) rows[index] = { ...row, unread: 0 };
  }
  return {
    shown,
    tab,
    peer,
    current,
    counts,
    hidePrivateRowBadges,
    countsBusy,
    countsError,
    setCountVisibility,
    blockInboxSync,
    beginInboxDeletion,
    open,
    close,
    readCounts,
    load,
    select,
    enter,
    back,
    clearConversationBadges,
  };
}
