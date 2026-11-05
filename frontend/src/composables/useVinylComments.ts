import { computed, onScopeDispose, ref, shallowRef, watch, type Ref } from 'vue';
import type { CommentItem } from '../services/comments';
import type { PlayerClient } from '../services/player';

export function useVinylComments(
  api: Pick<PlayerClient, 'playerVinylComments'>,
  target: Readonly<Ref<string | undefined>>,
  visible: Readonly<Ref<boolean>>,
  count: Readonly<Ref<number | undefined>>,
) {
  const latest = shallowRef<CommentItem[]>([]);
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
      latest.value = [];
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
      latest.value = [...latest.value, ...value.latest];
      nextOffset.value = value.nextOffset;
      loaded.value = true;
    } catch {
      if (current()) failed.value = true;
    } finally {
      if (current()) working.value = false;
    }
  }
  return { latest, hot, moreHot, working, failed, loaded, canAppend, load };
}
export type VinylCommentsModel = ReturnType<typeof useVinylComments>;
