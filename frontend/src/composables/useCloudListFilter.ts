import { onScopeDispose, ref, watch } from 'vue';
import type { CloudTab } from '../services/cloud.ts';

type Scope = { epoch: string; tab: CloudTab; keyword: string };

// M/2040 sends normal changes after 10 ms and IME changes immediately.
// Each native read is a local snapshot projection; useCloud fences its result.
export function useCloudListFilter(scope: () => Scope, apply: (keyword: string) => void) {
  const keyword = ref(scope().keyword);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let composing = false;
  function cancel() {
    clearTimeout(timer);
    timer = undefined;
  }
  function commit(value: string) {
    cancel();
    keyword.value = value;
    const current = scope();
    if (current.epoch && current.tab === 'uploaded' && current.keyword !== value) apply(value);
  }
  function change(value: string, isComposing = false) {
    cancel();
    keyword.value = value;
    if (composing || isComposing) {
      commit(value);
    } else {
      const { epoch, tab } = scope();
      timer = setTimeout(() => {
        const current = scope();
        if (current.epoch === epoch && current.tab === tab) commit(value);
      }, 10);
    }
  }
  function startComposition() {
    cancel();
    composing = true;
  }
  function endComposition(value: string) {
    composing = false;
    commit(value);
  }
  function submit() {
    if (!composing) commit(keyword.value);
  }
  function clear() {
    composing = false;
    commit('');
  }
  watch(
    [() => scope().epoch, () => scope().tab],
    () => {
      cancel();
      composing = false;
      keyword.value = scope().keyword;
    },
    { flush: 'sync' },
  );
  watch(
    () => scope().keyword,
    (value) => {
      if (value !== keyword.value) {
        cancel();
        keyword.value = value;
      }
    },
    { flush: 'sync' },
  );
  onScopeDispose(cancel);
  return { keyword, change, startComposition, endComposition, submit, clear };
}
