import { ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type { CatalogClient, CatalogQuery, CatalogView } from '../services/catalog.ts';
import { useCore } from '../services/core.ts';

function key(query: CatalogQuery) {
  return JSON.stringify([
    query.kind,
    query.id,
    'offset' in query ? query.offset : null,
    'limit' in query ? query.limit : null,
    'cursor' in query ? query.cursor : null,
    'size' in query ? query.size : null,
  ]);
}
const isExpired = (code: string) => ['session_expired', 'profile_changed'].includes(code);

/** One independent page slot. A header and a tab can refresh separately. */
export function useCatalog(
  core: Pick<CatalogClient, 'catalogPage'> = useCore(),
  expired = () => {},
) {
  const view = shallowRef<CatalogView>();
  const query = shallowRef<CatalogQuery>();
  const loading = ref(false);
  const error = ref('');
  let epoch: string | undefined;
  let serial = 0;
  let disposed = false;

  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch) return;
    epoch = next;
    serial++;
    view.value = undefined;
    loading.value = false;
    error.value = '';
  }
  async function open(target: CatalogQuery, refresh = false) {
    if (disposed) return;
    const same = query.value !== undefined && key(query.value) === key(target);
    if (same && loading.value) return;
    const current = ++serial;
    const owner = epoch;
    // Do not let a caller mutating its route object change an in-flight query.
    target = { ...target };
    query.value = target;
    if (!same) view.value = undefined;
    error.value = '';
    if (!owner) return;
    loading.value = true;
    const active = () => !disposed && serial === current && owner === epoch;
    const validate = (value: CatalogView) => {
      if (key(value.query) !== key(target) || value.page.kind !== target.kind) {
        throw { code: 'invalid_response', message: '详情响应不匹配，请重试' };
      }
      return value;
    };
    try {
      if (!refresh) {
        try {
          const saved = await core.catalogPage(owner, target, false);
          if (!active()) return;
          if (saved.epoch === owner && saved.value) view.value = validate(saved.value);
        } catch (e) {
          if (!active()) return;
          if (isExpired(failure(e).code)) throw e;
        }
      }
      if (!active()) return;
      // M/15 disables the official page cache: a local snapshot never skips
      // revalidation or establishes current collection/playback permission.
      const result = await core.catalogPage(owner, target, true);
      if (!active()) return;
      if (result.epoch !== owner || result.value === null) {
        throw { code: 'invalid_response', message: '暂时无法读取详情，请重试' };
      }
      view.value = validate(result.value);
    } catch (e) {
      if (!active()) return;
      const issue = failure(e);
      error.value = issue.message;
      if (isExpired(issue.code)) {
        view.value = undefined;
        expired();
      }
    } finally {
      if (active()) loading.value = false;
    }
  }
  function dispose() {
    disposed = true;
    serial++;
    loading.value = false;
    view.value = undefined;
  }
  return {
    view,
    query,
    loading,
    error,
    connect,
    open,
    refresh: () => (query.value ? open(query.value, true) : Promise.resolve()),
    dispose,
  };
}
