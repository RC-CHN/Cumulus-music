import { onScopeDispose, ref, watch } from 'vue';
import type { LocalModel } from './useLocal.ts';
import type { LocalGroup, LocalQuery } from '../services/local.ts';

/** Recovered FlexSearchInput uses a 10 ms debounce; a visit owns its pending input. */
export function useLocalSearch(model: LocalModel) {
  const keyword = ref(model.query.value.keyword);
  const owns = model.captureVisit();
  let composing = false,
    alive = true,
    timer: ReturnType<typeof setTimeout> | undefined;
  function stop() {
    clearTimeout(timer);
    timer = undefined;
  }
  function flush() {
    stop();
    if (!alive || !owns() || composing) return;
    if (keyword.value !== model.query.value.keyword || model.readError.value)
      model.filter({ keyword: keyword.value });
  }
  function input(value: string) {
    if (!alive || !owns() || composing) return;
    keyword.value = value;
    stop();
    timer = setTimeout(flush, 10);
  }
  function compositionStart() {
    composing = true;
    stop();
  }
  function compositionEnd(value: string) {
    composing = false;
    if (!alive || !owns()) return;
    keyword.value = value;
    flush();
  }
  function clear() {
    composing = false;
    keyword.value = '';
    flush();
  }
  function refine(
    value: Partial<Pick<LocalQuery, 'sort' | 'descending' | 'groupKey' | 'includeMissing'>>,
  ) {
    if (!alive || !owns()) return;
    stop();
    model.filter(composing ? value : { ...value, keyword: keyword.value });
  }
  function group(value: LocalGroup) {
    stop();
    if (!alive || !owns()) return;
    composing = false;
    keyword.value = '';
    model.filter({ group: value, groupKey: null, keyword: '' });
  }
  watch(
    [() => model.query.value.keyword, () => model.query.value.group],
    (next, before) => {
      stop();
      if (next[1] !== before?.[1]) composing = false;
      if (!composing) keyword.value = model.query.value.keyword;
    },
    { flush: 'sync' },
  );
  onScopeDispose(() => {
    alive = false;
    stop();
  });
  return { keyword, input, flush, clear, refine, group, compositionStart, compositionEnd };
}
