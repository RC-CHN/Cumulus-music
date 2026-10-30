import { computed, reactive, ref, shallowRef } from 'vue';
import { failure, type AccountView, type CoreClient } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import type {
  SearchChart,
  SearchWord,
  SearchDiscoveryQuery,
  SearchHistoryCommand,
} from '../services/search.ts';

export function useSearchBox(core: CoreClient = useCore(), expired: () => void = () => {}) {
  const query = ref(''),
    open = ref(false),
    composing = ref(false);
  const history = shallowRef<string[]>([]),
    defaults = shallowRef<SearchWord[]>([]);
  const recommendations = shallowRef<SearchWord[]>([]),
    charts = shallowRef<SearchChart[]>([]);
  const suggestions = shallowRef<SearchWord[]>([]),
    recommended = shallowRef<SearchWord[]>([]);
  const suggestionTitle = ref('猜你想搜'),
    defaultIndex = ref(0);
  const errors = reactive<Record<string, string>>({}),
    pending = reactive<Record<string, boolean>>({});
  const defaultWord = computed(() => defaults.value[defaultIndex.value]);
  const searching = computed(() => !!query.value.trim());
  let epoch = '',
    connected = false,
    generation = 0,
    suggestionRequest = 0;
  let debounce: ReturnType<typeof setTimeout> | undefined,
    rotate: ReturnType<typeof setInterval> | undefined;
  let historyQueue: Promise<void> = Promise.resolve();
  const requests = new Map<string, number>();
  const live = (e: string, g: number) => connected && epoch === e && generation === g;
  function timers() {
    clearTimeout(debounce);
    clearInterval(rotate);
    debounce = undefined;
    rotate = undefined;
  }
  function issue(value: unknown, slot: string) {
    const p = failure(value);
    if (p.code === 'stale') return;
    errors[slot] = p.message;
    if (p.code === 'session_expired' || p.code === 'profile_changed') {
      dispose();
      expired();
    }
  }
  async function load(q: SearchDiscoveryQuery, slot = q.kind as string) {
    if (!connected) return;
    if (q.kind === 'charts')
      for (const key of requests.keys())
        if (key.startsWith('chart:')) {
          requests.set(key, (requests.get(key) ?? 0) + 1);
          pending[key] = false;
        }
    const e = epoch,
      g = generation,
      n = (requests.get(slot) ?? 0) + 1;
    requests.set(slot, n);
    pending[slot] = true;
    errors[slot] = '';
    try {
      const result = await core.searchDiscovery(e, q);
      if (!live(e, g) || result.epoch !== e || requests.get(slot) !== n) return;
      const v = result.value;
      switch (v.kind) {
        case 'defaults':
          defaults.value = v.words;
          defaultIndex.value = 0;
          clearInterval(rotate);
          if (v.words.length > 1)
            rotate = setInterval(() => {
              if (!open.value && !composing.value && !query.value)
                defaultIndex.value = (defaultIndex.value + 1) % v.words.length;
            }, v.refreshMs);
          break;
        case 'recommendations':
          recommendations.value = v.words;
          break;
        case 'charts':
          charts.value = v.charts;
          for (const chart of v.charts)
            if (!chart.items.length)
              void load({ kind: 'chart', id: chart.id }, 'chart:' + chart.id);
          break;
        case 'chart':
          charts.value = charts.value.map((c) => (c.id === v.id ? { ...c, items: v.items } : c));
          break;
        case 'suggestions':
          suggestionTitle.value = v.title;
          recommended.value = v.recommended;
          suggestions.value = v.suggestions;
          break;
      }
    } catch (value) {
      if (live(e, g) && requests.get(slot) === n) issue(value, slot);
    } finally {
      if (live(e, g) && requests.get(slot) === n) pending[slot] = false;
    }
  }
  function historyCommand(command: SearchHistoryCommand) {
    const e = epoch,
      g = generation;
    const work = historyQueue.then(async () => {
      if (!live(e, g)) return;
      pending.history = true;
      errors.history = '';
      try {
        const result = await core.searchHistory(e, command);
        if (live(e, g) && result.epoch === e) history.value = result.value;
      } catch (value) {
        if (live(e, g)) issue(value, 'history');
      } finally {
        if (live(e, g)) pending.history = false;
      }
    });
    historyQueue = work;
    return work;
  }
  function invalidateSuggestions() {
    clearTimeout(debounce);
    requests.set('suggestions', (requests.get('suggestions') ?? 0) + 1);
    suggestionRequest++;
    pending.suggestions = false;
    errors.suggestions = '';
    recommended.value = [];
    suggestions.value = [];
  }
  function home() {
    void load({ kind: 'recommendations', lastQuery: history.value[0] ?? '' });
    void load({ kind: 'charts' });
  }
  function input(value: string) {
    query.value = value;
    invalidateSuggestions();
    if (!connected || !open.value || composing.value) return;
    if (!value.trim()) {
      home();
      return;
    }
    pending.suggestions = true;
    const current = suggestionRequest;
    debounce = setTimeout(() => {
      if (current === suggestionRequest)
        void load({ kind: 'suggestions', keyword: query.value.trim() });
    }, 200);
  }
  function show() {
    if (!connected) return;
    if (open.value) return;
    open.value = true;
    if (query.value.trim()) input(query.value);
    else home();
  }
  function close() {
    open.value = false;
    invalidateSuggestions();
  }
  function composition(start: boolean) {
    composing.value = start;
    if (start) invalidateSuggestions();
    else input(query.value);
  }
  async function connect(account: AccountView) {
    dispose();
    epoch = account.epoch;
    connected = account.status === 'authenticated';
    if (!connected) return;
    await Promise.all([load({ kind: 'defaults' }), historyCommand({ kind: 'read' })]);
  }
  function dispose() {
    connected = false;
    generation++;
    timers();
    requests.clear();
    suggestionRequest++;
    open.value = false;
    composing.value = false;
    query.value = '';
    history.value = [];
    defaults.value = [];
    recommendations.value = [];
    charts.value = [];
    suggestions.value = [];
    recommended.value = [];
    for (const k of Object.keys(errors)) delete errors[k];
    for (const k of Object.keys(pending)) delete pending[k];
    // Old account work retains its own generation and cannot block the new account.
    historyQueue = Promise.resolve();
  }
  function submit(word?: string) {
    if (!connected || composing.value) return;
    const value = (word ?? (query.value.trim() || defaultWord.value?.keyword || '')).trim();
    if (!value) return;
    query.value = value;
    close();
    void historyCommand({ kind: 'remember', keyword: value });
    return value;
  }
  return {
    query,
    open,
    composing,
    history,
    defaultWord,
    recommendations,
    charts,
    suggestions,
    recommended,
    suggestionTitle,
    errors,
    pending,
    searching,
    connect,
    input,
    show,
    close,
    composition,
    submit,
    load,
    historyCommand,
    dispose,
  };
}
export type SearchBoxModel = ReturnType<typeof useSearchBox>;
