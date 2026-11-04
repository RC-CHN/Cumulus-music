import { computed, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import type { PlaybackPowerClient, PlaybackPowerView } from '../services/playbackPower.ts';

export function usePlaybackPower(core: PlaybackPowerClient = useCore()) {
  const view = shallowRef<PlaybackPowerView>();
  const loading = ref(false),
    busy = ref(false),
    saved = ref(false),
    error = ref('');
  let epoch: string | undefined,
    serial = 0,
    disposed = false;
  const visible = computed(() => view.value?.available === true);
  const canChange = computed(() => visible.value && !loading.value && !busy.value && !error.value);
  function begin(owner: string) {
    const current = ++serial;
    return () => !disposed && epoch === owner && serial === current;
  }
  function message(cause: unknown, fallback: string) {
    const issue = failure(cause);
    return issue.code === 'connection' ? fallback : issue.message;
  }
  async function read() {
    if (disposed || !epoch || loading.value || busy.value) return;
    const owner = epoch,
      active = begin(owner);
    loading.value = true;
    error.value = '';
    saved.value = false;
    try {
      const result = await core.playbackPower(owner);
      if (active()) view.value = result;
    } catch (cause) {
      if (active()) error.value = message(cause, '暂时无法读取防休眠设置，请重试');
    } finally {
      if (active()) loading.value = false;
    }
  }
  function connect(owner: string) {
    if (disposed || epoch === owner) return;
    epoch = owner;
    serial++;
    view.value = undefined;
    loading.value = false;
    busy.value = false;
    saved.value = false;
    error.value = '';
    return read();
  }
  async function change(enabled: boolean) {
    if (disposed || !epoch || !canChange.value || view.value?.enabled === enabled) return false;
    const owner = epoch,
      active = begin(owner);
    busy.value = true;
    error.value = '';
    saved.value = false;
    try {
      const result = await core.setPlaybackPower(owner, enabled);
      if (!active()) return false;
      view.value = result;
      if (!result.available) return false;
      if (result.enabled !== enabled) {
        error.value = '设置未保存，请重新读取播放设置';
        return false;
      }
      saved.value = true;
      return true;
    } catch (cause) {
      if (active()) {
        error.value = message(cause, '暂时无法保存防休眠设置，请重新读取');
        // A lost response can follow a durable local write. Read once; a read
        // does not confirm which actor wrote it, and must never replay a set.
        try {
          const result = await core.playbackPower(owner);
          if (active()) view.value = result;
        } catch {
          /* Keep the last verified value; the refresh button remains available. */
        }
      }
      return false;
    } finally {
      if (active()) busy.value = false;
    }
  }
  function dispose() {
    disposed = true;
    serial++;
  }
  return { view, visible, loading, busy, saved, error, canChange, connect, read, change, dispose };
}
