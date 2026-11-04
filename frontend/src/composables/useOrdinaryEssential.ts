import { onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import type {
  EssentialCategory,
  EssentialRail,
  OrdinaryEssentialClient,
  OrdinaryEssentialView,
} from '../services/ordinaryEssential';

// M/460 -> M/32.n fallback when preload#cacheValidateTime.choice.common is absent.
const FRESH_MS = 3 * 60 * 60 * 1000 - 3000;

export function useOrdinaryEssential(
  core: OrdinaryEssentialClient,
  account: () => AccountView,
  active: () => boolean,
  expired: () => void,
) {
  const data = shallowRef<OrdinaryEssentialView>();
  const category = ref<EssentialCategory>(0);
  const loading = ref(false);
  const changing = ref(false);
  const error = ref('');
  const categoryError = ref('');
  const viewport = { scroll: 0, focus: '', rails: new Map<EssentialRail, number>() };
  let owner: string | undefined;
  let version = 0;
  let observed = 0;
  function reset(epoch?: string) {
    ++version;
    owner = epoch;
    data.value = undefined;
    category.value = 0;
    error.value = '';
    categoryError.value = '';
    changing.value = false;
    loading.value = false;
    observed = 0;
    viewport.scroll = 0;
    viewport.focus = '';
    viewport.rails.clear();
  }
  function issue(error: unknown) {
    const problem = failure(error);
    if (problem.code === 'session_expired' || problem.code === 'profile_changed') {
      reset();
      if (active()) expired();
    }
    return problem.message;
  }
  async function load(force = false) {
    if (!owner || !active() || loading.value || changing.value || navigator.onLine === false)
      return;
    if (!force && observed && Date.now() - observed < FRESH_MS) return;
    const epoch = owner;
    const request = ++version;
    loading.value = true;
    error.value = '';
    try {
      const reply = await core.ordinaryEssential(epoch, category.value);
      if (request !== version || reply.epoch !== epoch) return;
      data.value = reply.value;
      if (reply.value.failedBlocks === 4) {
        error.value = '暂时无法加载精选，请重试。';
        observed = 0;
      } else observed = Date.now();
    } catch (error_) {
      if (request === version) error.value = issue(error_);
    } finally {
      if (request === version) loading.value = false;
    }
  }
  async function select(category_: EssentialCategory) {
    if (
      !owner ||
      !active() ||
      !data.value ||
      changing.value ||
      loading.value ||
      category.value === category_
    )
      return;
    const epoch = owner;
    const request = ++version;
    changing.value = true;
    categoryError.value = '';
    try {
      const reply = await core.ordinaryEssentialBooks(epoch, category_);
      if (request !== version || reply.epoch !== epoch || !data.value) return;
      if (!reply.value) {
        categoryError.value = '这个分类暂时没有可显示的推荐。';
        return;
      }
      category.value = category_;
      viewport.rails.delete('books');
      data.value = { ...data.value, content: { ...data.value.content, books: reply.value } };
    } catch (error_) {
      if (request === version) categoryError.value = issue(error_);
    } finally {
      if (request === version) changing.value = false;
    }
  }
  const online = ref(navigator.onLine);
  function connectivity() {
    online.value = navigator.onLine;
    if (online.value) void load();
  }
  window.addEventListener('online', connectivity);
  window.addEventListener('offline', connectivity);
  onScopeDispose(() => {
    window.removeEventListener('online', connectivity);
    window.removeEventListener('offline', connectivity);
    reset();
  });
  watch(
    [() => account().epoch, () => account().status, active],
    ([epoch, status, visible]) => {
      const next = status === 'authenticated' ? epoch : undefined;
      if (owner !== next) reset(next);
      if (next && visible) void load();
    },
    { immediate: true, flush: 'sync' },
  );
  return {
    data,
    category,
    loading,
    changing,
    error,
    categoryError,
    viewport,
    online,
    select,
    retry: () => load(true),
  };
}

export type OrdinaryEssentialModel = ReturnType<typeof useOrdinaryEssential>;
