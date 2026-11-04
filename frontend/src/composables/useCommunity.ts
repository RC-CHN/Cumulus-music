import { computed, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  CommunityCard,
  CommunityClient,
  CommunityComment,
  CommunityDetail,
  CommunityFeedAction,
} from '../services/community.ts';

const GROUP_SIZE = 24;
const MAX_CARDS = 440;

export function useCommunity(core: CommunityClient, expired: () => void) {
  let epoch: string | null = null;
  let feedVersion = 0;
  let detailVersion = 0;
  const cards = shallowRef<CommunityCard[]>([]);
  const continuation = ref<string | null>(null);
  const loaded = ref(false);
  const pending = ref(false);
  const feedError = ref('');
  const group = ref(0);
  const scrollTop = ref(0);
  const opener = ref<string | null>(null);
  let retryAction: CommunityFeedAction | null = null;
  const groups = computed(() => Math.max(1, Math.ceil(cards.value.length / GROUP_SIZE)));
  const visible = computed(() =>
    cards.value.slice(group.value * GROUP_SIZE, (group.value + 1) * GROUP_SIZE),
  );
  const autoMore = computed(
    () =>
      continuation.value !== null &&
      group.value === groups.value - 1 &&
      visible.value.length < GROUP_SIZE,
  );

  const handle = ref<string | null>(null);
  const detail = shallowRef<CommunityDetail | null>();
  const detailPending = ref(false);
  const detailError = ref('');
  const commentsPending = ref(false);
  const commentsError = ref('');
  const latest = shallowRef<CommunityComment[]>([]);
  const hot = shallowRef<CommunityComment[]>([]);
  const total = ref<number | null>(null);
  const nextOffset = ref<number | null>(null);

  function report(reason: unknown, into: typeof feedError) {
    const issue = failure(reason);
    into.value =
      issue.code === 'service_rejected'
        ? into === feedError
          ? '暂时无法读取关注动态。'
          : into === detailError
            ? '暂时无法读取这条动态。'
            : '暂时无法读取评论。'
        : issue.message;
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
  }

  function clearDetail() {
    ++detailVersion;
    handle.value = null;
    detail.value = undefined;
    detailPending.value = false;
    detailError.value = '';
    commentsPending.value = false;
    commentsError.value = '';
    latest.value = [];
    hot.value = [];
    total.value = null;
    nextOffset.value = null;
  }

  function connect(owner: string | null) {
    if (owner === epoch) return;
    epoch = owner;
    ++feedVersion;
    clearDetail();
    cards.value = [];
    continuation.value = null;
    loaded.value = false;
    pending.value = false;
    feedError.value = '';
    group.value = 0;
    scrollTop.value = 0;
    opener.value = null;
    retryAction = null;
  }

  async function read(action: CommunityFeedAction) {
    if (epoch === null || pending.value) return;
    const owner = epoch;
    const ticket = ++feedVersion;
    pending.value = true;
    feedError.value = '';
    try {
      const reply = await core.communityFeed(owner, action);
      if (ticket !== feedVersion || reply.epoch !== owner) return;
      const result = reply.value;
      const openKey = handle.value;
      if (
        openKey &&
        (result.replace || result.cards.some((item) => item.key === openKey && !item.detail))
      ) {
        // A feed response can finish while the detail is being read. Never
        // leave an old author's detail or comments open after it was revoked.
        clearDetail();
        handle.value = openKey;
        detail.value = null;
      }
      if (result.replace) {
        cards.value = result.cards;
        group.value = 0;
        scrollTop.value = 0;
        opener.value = null;
      } else {
        const merged = new Map(cards.value.map((item) => [item.key, item]));
        result.cards.forEach((item) => merged.set(item.key, item));
        const rows = [...merged.values()];
        // Drop complete display groups only. Native handles outlive this window.
        const drop = Math.ceil(Math.max(0, rows.length - MAX_CARDS) / GROUP_SIZE) * GROUP_SIZE;
        cards.value = rows.slice(drop);
        group.value = Math.max(0, group.value - drop / GROUP_SIZE);
      }
      continuation.value = result.continuation;
      loaded.value = true;
      retryAction = null;
    } catch (reason) {
      if (ticket === feedVersion) {
        retryAction = action;
        report(reason, feedError);
      }
    } finally {
      if (ticket === feedVersion) pending.value = false;
    }
  }

  function ensure() {
    if (!loaded.value && !feedError.value) return read({ kind: 'first' });
  }
  function fresh() {
    if (pending.value) return;
    return read({ kind: loaded.value ? 'refresh' : 'first' });
  }
  function next() {
    if (continuation.value !== null)
      return read({ kind: 'next', continuation: continuation.value });
  }
  function retry() {
    if (retryAction) return read(retryAction);
  }
  function choose(next: number) {
    if (next < 0 || next >= groups.value) return;
    group.value = next;
    scrollTop.value = 0;
    opener.value = null;
  }

  async function readComments(offset = 0) {
    const owner = epoch;
    const key = handle.value;
    const ticket = detailVersion;
    if (owner === null || key === null || !detail.value?.commentsAvailable || commentsPending.value)
      return;
    commentsPending.value = true;
    commentsError.value = '';
    try {
      const reply = await core.communityComments(owner, key, offset);
      if (ticket !== detailVersion || reply.epoch !== owner) return;
      const value = reply.value;
      // The native API still echoes the official offset; display only the
      // current page to keep a long comment section usable in a small WebView.
      latest.value = value.comments;
      if (offset === 0) hot.value = value.hotComments;
      nextOffset.value = value.nextOffset;
      total.value = value.total;
    } catch (reason) {
      if (ticket === detailVersion) report(reason, commentsError);
    } finally {
      if (ticket === detailVersion) commentsPending.value = false;
    }
  }

  async function openDetail(key: string) {
    const owner = epoch;
    if (owner === null) return;
    clearDetail();
    const ticket = detailVersion;
    handle.value = key;
    detailPending.value = true;
    try {
      const reply = await core.communityDetail(owner, key);
      if (ticket !== detailVersion || reply.epoch !== owner) return;
      detail.value = reply.value;
      if (detail.value?.commentsAvailable) void readComments();
    } catch (reason) {
      if (ticket === detailVersion) report(reason, detailError);
    } finally {
      if (ticket === detailVersion) detailPending.value = false;
    }
  }

  return {
    visible,
    group,
    groups,
    continuation,
    loaded,
    pending,
    feedError,
    autoMore,
    scrollTop,
    opener,
    connect,
    ensure,
    fresh,
    next,
    retry,
    choose,
    detail,
    handle,
    detailPending,
    detailError,
    clearDetail,
    openDetail,
    readComments,
    latest,
    hot,
    total,
    nextOffset,
    commentsPending,
    commentsError,
  };
}
