import { ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import type { CatalogClient, CatalogQuery, CatalogView } from '../services/catalog.ts';
import { catalogPageKey, nextCatalogQuery } from '../services/catalogPages.ts';
import { useCore } from '../services/core.ts';
import { useCatalog } from './useCatalog.ts';

export function useCatalogPages(
  core: Pick<CatalogClient, 'catalogPage'> = useCore(),
  expired = () => {},
) {
  const slot = useCatalog(core, expired);
  const pages = shallowRef<CatalogView[]>([]);
  const loadingMore = ref(false);
  let serial = 0,
    epoch: string | undefined,
    firstKey: string | undefined;
  const stop = watch(
    slot.view,
    (value) => {
      if (value && firstKey === catalogPageKey(value.query) && !loadingMore.value)
        pages.value = [value];
    },
    { flush: 'sync' },
  );
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next !== epoch) {
      serial++;
      epoch = next;
      pages.value = [];
      loadingMore.value = false;
    }
    slot.connect(account);
  }
  function clear() {
    serial++;
    firstKey = undefined;
    pages.value = [];
    loadingMore.value = false;
  }
  async function open(query: CatalogQuery, refresh = false) {
    const current = ++serial;
    firstKey = catalogPageKey(query);
    const same = pages.value[0] && catalogPageKey(pages.value[0].query) === firstKey;
    // Keep recovery content while refreshing; replace continuations only when
    // a new first page has actually arrived.
    if (!same) pages.value = [];
    loadingMore.value = false;
    await slot.open(query, refresh);
    if (
      current === serial &&
      (!same || !slot.error.value) &&
      slot.view.value &&
      catalogPageKey(slot.view.value.query) === catalogPageKey(query)
    )
      pages.value = [slot.view.value];
  }
  async function more() {
    const last = pages.value.at(-1);
    const next = last && nextCatalogQuery(last);
    if (!next || loadingMore.value || slot.loading.value) return;
    const current = serial;
    loadingMore.value = true;
    await slot.open(next, true);
    if (current !== serial) return;
    if (
      !slot.error.value &&
      slot.view.value &&
      catalogPageKey(slot.view.value.query) === catalogPageKey(next)
    )
      pages.value = [...pages.value, slot.view.value];
    loadingMore.value = false;
  }
  function dispose() {
    clear();
    stop();
    slot.dispose();
  }
  return {
    pages,
    loading: slot.loading,
    loadingMore,
    error: slot.error,
    connect,
    open,
    more,
    clear,
    dispose,
  };
}
