import { computed, ref } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type { SceneFmAutoplayClient } from '../services/sceneFm.ts';

export function useSceneFmAutoplay(core: SceneFmAutoplayClient) {
  const enabled = ref<boolean>();
  const loading = ref(false);
  const busy = ref(false);
  const saved = ref(false);
  const error = ref('');
  const canChange = computed(
    () => enabled.value !== undefined && !loading.value && !busy.value && !error.value,
  );
  let epoch: string | undefined;
  let generation = 0;
  let disposed = false;

  function message(cause: unknown, fallback: string) {
    const issue = failure(cause);
    return issue.code === 'connection' ? fallback : issue.message;
  }
  async function read() {
    if (!epoch || disposed || loading.value || busy.value) return;
    const owner = epoch;
    const current = generation;
    const valid = () => !disposed && owner === epoch && current === generation;
    loading.value = true;
    saved.value = false;
    error.value = '';
    try {
      const result = await core.sceneFmAutoplayPreference(owner);
      if (!valid()) return;
      if (result.epoch !== owner || typeof result.value !== 'boolean') {
        error.value = '暂时无法读取播客自动播放设置，请重试';
        return;
      }
      enabled.value = result.value;
    } catch (cause) {
      if (valid()) error.value = message(cause, '暂时无法读取播客自动播放设置，请重试');
    } finally {
      if (valid()) loading.value = false;
    }
  }
  function connect(account: AccountView) {
    const owner = account.status === 'authenticated' ? account.epoch : undefined;
    if (disposed || epoch === owner) return;
    epoch = owner;
    generation++;
    enabled.value = undefined;
    loading.value = false;
    busy.value = false;
    saved.value = false;
    error.value = '';
    return read();
  }
  async function change(value: boolean) {
    if (!epoch || disposed || !canChange.value || value === enabled.value) return false;
    const owner = epoch;
    const current = generation;
    const valid = () => !disposed && owner === epoch && current === generation;
    busy.value = true;
    saved.value = false;
    error.value = '';
    try {
      const result = await core.setSceneFmAutoplayPreference(owner, value);
      if (!valid()) return false;
      if (result.epoch !== owner || typeof result.value !== 'boolean') {
        error.value = '未能确认播客自动播放设置，请重新读取';
        return false;
      }
      enabled.value = result.value;
      if (result.value !== value) {
        error.value = '未能确认播客自动播放设置，请重新读取';
        return false;
      }
      saved.value = true;
      return true;
    } catch (cause) {
      if (!valid()) return false;
      error.value = message(cause, '暂时无法保存播客自动播放设置，请重新读取');
      // The local write may have succeeded even if the response was lost.
      // Read once to refresh the checkbox, and keep the failure visible.
      try {
        const result = await core.sceneFmAutoplayPreference(owner);
        if (valid() && result.epoch === owner && typeof result.value === 'boolean')
          enabled.value = result.value;
      } catch {
        /* An explicit settings refresh remains available. */
      }
      return false;
    } finally {
      if (valid()) busy.value = false;
    }
  }
  function dispose() {
    disposed = true;
    generation++;
  }
  return { enabled, loading, busy, saved, error, canChange, connect, read, change, dispose };
}
