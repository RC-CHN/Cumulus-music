import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import {
  collectionKey,
  firstCollectionQuery,
  type CollectionClient,
  type CollectionScope,
  type CollectionQuery,
  type CollectionView,
} from '../services/collections.ts';
import { useCore } from '../services/core.ts';

export function useCollections(core: CollectionClient = useCore(), expired = () => {}) {
  const scope = ref<CollectionScope>('saved_albums'),
    pages = shallowRef<CollectionView[]>([]);
  const loading = ref(false),
    loadingMore = ref(false),
    error = ref('');
  const next = computed(() => pages.value.at(-1)?.next ?? null);
  const total = computed(() => pages.value[0]?.total ?? null);
  let epoch: string | undefined,
    serial = 0,
    disposed = false;
  let retryTarget: { query: CollectionQuery; append: boolean } | undefined;
  function connect(account: AccountView) {
    const owner = account.status === 'authenticated' ? account.epoch : undefined;
    if (owner === epoch) return;
    epoch = owner;
    serial++;
    pages.value = [];
    loading.value = false;
    loadingMore.value = false;
    error.value = '';
    retryTarget = undefined;
  }
  async function request(query: CollectionQuery, append: boolean, cache: boolean) {
    if (!epoch || disposed) return;
    const current = ++serial,
      owner = epoch;
    query = { ...query };
    const active = () => !disposed && current === serial && owner === epoch;
    const validate = (view: CollectionView) => {
      if (
        collectionKey(view.query) !== collectionKey(query) ||
        view.page.kind !== (query.kind === 'followed_artists' ? 'follows' : 'albums') ||
        (view.next && view.next.kind !== query.kind)
      )
        throw { code: 'invalid_response', message: '收藏列表响应不匹配，请重试' };
      return view;
    };
    loading.value = true;
    loadingMore.value = append;
    error.value = '';
    retryTarget = undefined;
    try {
      if (cache) {
        try {
          const result = await core.collectionPage(owner, query, false);
          if (!active()) return;
          if (result.epoch === owner && result.value) pages.value = [validate(result.value)];
        } catch (e) {
          if (!active()) return;
          if (['session_expired', 'profile_changed'].includes(failure(e).code)) throw e;
        }
      }
      if (!active()) return;
      const result = await core.collectionPage(owner, query, true);
      if (!active()) return;
      if (result.epoch !== owner || !result.value)
        throw { code: 'invalid_response', message: '暂时无法读取收藏列表，请重试' };
      const view = validate(result.value);
      pages.value = append ? [...pages.value, view] : [view];
    } catch (e) {
      if (!active()) return;
      const issue = failure(e);
      error.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) {
        pages.value = [];
        expired();
      } else retryTarget = { query, append };
    } finally {
      if (active()) {
        loading.value = false;
        loadingMore.value = false;
      }
    }
  }
  async function open(target: CollectionScope, refresh = false) {
    if (disposed || (target === scope.value && loading.value)) return;
    const same = target === scope.value;
    scope.value = target;
    if (!same) pages.value = [];
    await request(firstCollectionQuery(target), false, !refresh && !pages.value.length);
  }
  async function more() {
    const query = next.value;
    if (!query || loading.value) return;
    if (
      pages.value.length >= 500 ||
      pages.value.some((page) => collectionKey(page.query) === collectionKey(query))
    ) {
      error.value = '服务端分页没有继续推进，请刷新列表';
      retryTarget = { query: firstCollectionQuery(scope.value), append: false };
      return;
    }
    await request(query, true, false);
  }
  function dispose() {
    disposed = true;
    serial++;
    pages.value = [];
    loading.value = false;
    loadingMore.value = false;
  }
  const retry = () =>
    !loading.value && retryTarget
      ? request(retryTarget.query, retryTarget.append, false)
      : Promise.resolve();
  return {
    scope,
    pages,
    loading,
    loadingMore,
    error,
    next,
    total,
    connect,
    open,
    more,
    retry,
    refresh: () => open(scope.value, true),
    dispose,
  };
}
