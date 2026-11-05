import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import type { CommentItem } from '../services/comments';
import type { PlayerClient } from '../services/player';
import { useVinylCommentLikes, type VinylCommentLikeClient } from './useVinylCommentLikes.ts';

export function useVinylComments(
  api: Pick<PlayerClient, 'playerVinylComments'> & VinylCommentLikeClient,
  target: Readonly<Ref<string | undefined>>,
  visible: Readonly<Ref<boolean>>,
  count: Readonly<Ref<number | undefined>>,
) {
  const fetched = shallowRef<CommentItem[]>([]);
  const published = shallowRef<CommentItem[]>([]);
  const latest = computed(() => {
    if (!published.value.length) return fetched.value;
    const seen = new Set<string>();
    return [...published.value, ...fetched.value].filter((item) => {
      if (seen.has(item.id)) return false;
      seen.add(item.id);
      return true;
    });
  });
  const hot = shallowRef<CommentItem[]>([]);
  const moreHot = ref(false);
  const nextOffset = ref<number | null>(0);
  const working = ref(false);
  const failed = ref(false);
  const loaded = ref(false);
  let request = 0;
  let delay: ReturnType<typeof setTimeout> | undefined;
  const canAppend = computed(
    () =>
      visible.value && loaded.value && !working.value && !failed.value && nextOffset.value !== null,
  );
  function invalidate() {
    request++;
    clearTimeout(delay);
    delay = undefined;
    working.value = false;
  }
  onScopeDispose(invalidate);
  watch(
    target,
    () => {
      invalidate();
      fetched.value = [];
      published.value = [];
      hot.value = [];
      moreHot.value = false;
      nextOffset.value = 0;
      loaded.value = false;
      failed.value = false;
    },
    { flush: 'sync' },
  );
  watch(
    [target, visible, count],
    ([key, active, total], _, cleanup) => {
      if (!key || !active || !total || loaded.value || failed.value) return;
      delay = setTimeout(() => {
        delay = undefined;
        void load();
      }, 600);
      cleanup(() => clearTimeout(delay));
    },
    { immediate: true },
  );
  watch(
    visible,
    (active) => {
      if (!active) invalidate();
    },
    { flush: 'sync' },
  );
  async function load() {
    const key = target.value;
    const offset = nextOffset.value;
    if (!key || !visible.value || !count.value || offset === null || working.value) return;
    const [epoch, playId] = JSON.parse(key) as [string, string];
    const started = ++request;
    const current = () => started === request && key === target.value && visible.value;
    working.value = true;
    failed.value = false;
    try {
      const { epoch: responseEpoch, value } = await api.playerVinylComments(epoch, playId, offset);
      if (!current()) return;
      if (responseEpoch !== epoch) throw new Error('stale vinyl comments');
      if (offset === 0) {
        hot.value = value.hot;
        moreHot.value = value.moreHot;
      }
      fetched.value = [...fetched.value, ...value.latest];
      nextOffset.value = value.nextOffset;
      loaded.value = true;
    } catch {
      if (current()) failed.value = true;
    } finally {
      if (current()) working.value = false;
    }
  }
  function prependConfirmed(key: string, item: CommentItem) {
    if (key !== target.value || !visible.value || published.value.some((row) => row.id === item.id))
      return false;
    // Display-only: pagination still advances by rows returned by the server.
    published.value = [item, ...published.value];
    return true;
  }
  const likes = useVinylCommentLikes(
    api,
    target,
    visible,
    loaded,
    (item) => latest.value.includes(item) || hot.value.includes(item),
    (item) => {
      // Do not let a page started before this verified observation undo it.
      // The next append, if any, still uses the unmodified service cursor.
      invalidate();
      const patch = (row: CommentItem) =>
        row.id === item.id ? { ...row, liked: item.liked, likedCount: item.likedCount } : row;
      fetched.value = fetched.value.map(patch);
      published.value = published.value.map(patch);
      hot.value = hot.value.map(patch);
    },
  );
  return {
    latest,
    hot,
    moreHot,
    working,
    failed,
    loaded,
    canAppend,
    load,
    prependConfirmed,
    likes,
  };
}
export type VinylCommentsModel = ReturnType<typeof useVinylComments>;
