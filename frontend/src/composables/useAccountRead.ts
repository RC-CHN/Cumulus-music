import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure, type AccountData } from '../services/contracts.ts';

export type AccountReadState = 'loading' | 'ready' | 'empty' | 'error' | 'signed-out';

export function useAccountRead<T>(
  owner: () => string | null,
  read: (epoch: string) => Promise<AccountData<T>>,
  empty: (value: T) => boolean = () => false,
  expired: () => void = () => {},
) {
  const value = shallowRef<T>();
  const loading = ref(false);
  const error = ref('');
  let revision = 0;
  let disposed = false;
  const state = computed<AccountReadState>(() =>
    !owner()
      ? 'signed-out'
      : loading.value
        ? 'loading'
        : error.value
          ? 'error'
          : value.value !== undefined && empty(value.value)
            ? 'empty'
            : 'ready',
  );
  function clear() {
    revision++;
    value.value = undefined;
    loading.value = false;
    error.value = '';
  }
  async function refresh() {
    const epoch = owner();
    if (!epoch || disposed || loading.value) return;
    const request = ++revision;
    const active = () => !disposed && request === revision && epoch === owner();
    value.value = undefined;
    error.value = '';
    loading.value = true;
    try {
      const next = await read(epoch);
      if (!active()) return;
      if (next.epoch !== epoch) {
        error.value = '读取结果与当前账号不匹配，请重新读取。';
        return;
      }
      value.value = next.value;
    } catch (cause) {
      if (!active()) return;
      if (['session_expired', 'profile_changed', 'stale'].includes(failure(cause).code)) {
        error.value = '账号状态已变化，请重新登录后查看。';
        expired();
      } else error.value = '暂时无法读取，请重试。';
    } finally {
      if (active()) loading.value = false;
    }
  }
  const stop = watch(
    owner,
    () => {
      clear();
      void refresh();
    },
    { immediate: true, flush: 'sync' },
  );
  onScopeDispose(() => {
    disposed = true;
    stop();
    clear();
  });
  return { value, loading, error, state, refresh };
}
