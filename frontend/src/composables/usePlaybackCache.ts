import { computed, ref, shallowRef } from 'vue';
import { useCore } from '../services/core.ts';
import {
  playbackCacheCapacityAllowed,
  type PlaybackCacheClient,
  type PlaybackCacheStatus,
} from '../services/playbackCache.ts';

type Action = 'capacity' | 'clear';

export function usePlaybackCache(core: PlaybackCacheClient = useCore()) {
  const view = shallowRef<PlaybackCacheStatus | null>();
  const draft = ref<number>();
  const pending = ref<'read' | Action>();
  const issue = ref('');
  const result = ref<Action>();
  const stale = ref(false);
  const capacity = computed(() => draft.value ?? view.value?.capacityBytes);
  const canChange = computed(() => !!view.value && !stale.value && !pending.value);
  // Empty snapshots can still precede an admitted fill; native clear fences it.
  const canClear = canChange;
  let disposed = false;

  function accept(snapshot: PlaybackCacheStatus | null) {
    view.value = snapshot;
    draft.value = undefined;
    stale.value = false;
  }

  async function read() {
    if (disposed || pending.value) return;
    pending.value = 'read';
    issue.value = '';
    result.value = undefined;
    draft.value = undefined;
    try {
      const snapshot = await core.playbackCacheState();
      if (!disposed) accept(snapshot);
    } catch {
      if (!disposed) {
        stale.value = true;
        issue.value = '暂时无法读取播放缓存，请刷新重试。';
      }
    } finally {
      if (!disposed) pending.value = undefined;
    }
  }

  function preview(value: number) {
    const snapshot = view.value;
    if (!canChange.value || !snapshot || !playbackCacheCapacityAllowed(snapshot, value)) return;
    draft.value = value;
    result.value = undefined;
  }

  async function mutate(action: Action, send: () => Promise<PlaybackCacheStatus>) {
    pending.value = action;
    issue.value = '';
    result.value = undefined;
    try {
      const snapshot = await send();
      if (!disposed) {
        accept(snapshot);
        result.value = action;
      }
    } catch {
      if (disposed) return;
      // A failed IPC result can follow a successful native write. Re-read the
      // current snapshot; never repeat a capacity change or clear implicitly.
      stale.value = true;
      draft.value = undefined;
      try {
        const snapshot = await core.playbackCacheState();
        if (!disposed) {
          accept(snapshot);
          issue.value = '未能确认缓存操作，已重新读取当前状态。';
        }
      } catch {
        if (!disposed) issue.value = '未能确认缓存操作，请先刷新再决定是否重试。';
      }
    } finally {
      if (!disposed) pending.value = undefined;
    }
  }

  async function setCapacity(value: number) {
    const snapshot = view.value;
    if (disposed || !canChange.value || !snapshot || !playbackCacheCapacityAllowed(snapshot, value))
      return;
    if (snapshot.capacityBytes === value) {
      draft.value = undefined;
      return;
    }
    draft.value = value;
    return mutate('capacity', () => core.playbackCacheSetCapacity(snapshot.revision, value));
  }

  async function clear() {
    const snapshot = view.value;
    if (disposed || !canClear.value || !snapshot) return;
    return mutate('clear', () => core.playbackCacheClear(snapshot.revision));
  }

  function dispose() {
    disposed = true;
  }

  return {
    view,
    capacity,
    pending,
    issue,
    result,
    stale,
    canChange,
    canClear,
    read,
    preview,
    setCapacity,
    clear,
    dispose,
  };
}
