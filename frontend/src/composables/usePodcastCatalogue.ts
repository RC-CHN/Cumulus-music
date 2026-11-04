import { computed, ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import type { PodcastCategory, PodcastCatalogueCard } from '../services/podcast';
import { useCore } from '../services/core';

export function usePodcastCatalogue(account: () => AccountView, expired: () => void) {
  const core = useCore();
  const categories = shallowRef<PodcastCategory[]>([]);
  const categoriesPending = ref(false),
    categoriesError = ref('');
  const cards = shallowRef<PodcastCatalogueCard[]>([]);
  const parentId = ref('');
  const parent = computed(() =>
    categories.value.find((category) => category.id === parentId.value),
  );
  const subId = ref('');
  const pending = ref(false);
  const error = ref('');
  const continuation = ref<string>();
  const complete = ref(false);
  let seen = new Set<string>();
  let request = 0;
  let generation = 0;
  let metadata: Promise<void> | undefined;
  watch(
    () => [account().epoch, account().status],
    () => {
      request++;
      generation++;
      metadata = undefined;
      categories.value = [];
      categoriesPending.value = false;
      categoriesError.value = '';
      parentId.value = '';
      subId.value = '';
      clearCards();
      pending.value = false;
      error.value = '';
    },
    { flush: 'sync' },
  );
  function clearCards() {
    cards.value = [];
    continuation.value = undefined;
    complete.value = false;
    seen = new Set();
  }
  function live(epoch: string, id: number) {
    return id === request && epoch === account().epoch && account().status === 'authenticated';
  }
  function issue(value: unknown, slot = error) {
    const issue = failure(value);
    slot.value = issue.message;
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
  }
  function loadCategories() {
    if (metadata) return metadata;
    if (categories.value.length || account().status !== 'authenticated') return;
    const epoch = account().epoch;
    const owner = generation;
    const active = () =>
      owner === generation && account().epoch === epoch && account().status === 'authenticated';
    categoriesPending.value = true;
    categoriesError.value = '';
    metadata = Promise.resolve().then(async () => {
      if (!active()) return;
      try {
        const result = await core.podcastCatalogue(epoch, { kind: 'categories' });
        if (active() && result.epoch === epoch && result.value.kind === 'categories')
          categories.value = result.value.categories;
      } catch (value) {
        if (active()) issue(value, categoriesError);
      } finally {
        if (active()) {
          categoriesPending.value = false;
          metadata = undefined;
        }
      }
    });
    return metadata;
  }
  async function activate(id: string) {
    if (parentId.value !== id) {
      request++;
      parentId.value = id;
      subId.value = '';
      clearCards();
      pending.value = false;
      error.value = '';
    }
    await loadCategories();
    if (parentId.value !== id || subId.value || !categories.value.length) return;
    const first = parent.value?.children[0];
    if (first) choose(first.id);
    else error.value = '这个播客分类暂不可用';
  }
  async function load() {
    if (!subId.value || pending.value || complete.value || account().status !== 'authenticated')
      return;
    const epoch = account().epoch;
    const id = ++request;
    pending.value = true;
    error.value = '';
    try {
      const result = await core.podcastCatalogue(epoch, {
        kind: 'radios',
        parentId: parentId.value,
        subId: subId.value,
        continuation: continuation.value,
      });
      if (!live(epoch, id) || result.epoch !== epoch || result.value.kind !== 'radios') return;
      const next = result.value.continuation;
      if (next && seen.has(next)) {
        error.value = '目录未继续更新，请重新读取当前分类';
        complete.value = true;
        return;
      }
      if (next) seen.add(next);
      const identities = new Set(cards.value.map((card) => card.id));
      cards.value = [
        ...cards.value,
        ...result.value.cards.filter((card) => {
          if (identities.has(card.id)) return false;
          identities.add(card.id);
          return true;
        }),
      ];
      continuation.value = next ?? undefined;
      complete.value = !next || seen.size >= 20;
      if (next && complete.value) error.value = '本次目录已达到读取上限';
    } catch (value) {
      if (live(epoch, id)) {
        if (continuation.value && failure(value).code === 'stale') {
          complete.value = true;
          error.value = '目录已更新，请重新读取当前分类';
        } else issue(value);
      }
    } finally {
      if (live(epoch, id)) pending.value = false;
    }
  }
  function choose(id: string, refresh = false) {
    if ((!refresh && subId.value === id) || !parent.value?.children.some((item) => item.id === id))
      return;
    request++;
    subId.value = id;
    clearCards();
    pending.value = false;
    void load();
  }
  return {
    categories,
    categoriesPending,
    categoriesError,
    parent,
    cards,
    subId,
    pending,
    error,
    complete,
    continuation,
    activate,
    loadCategories,
    load,
    choose,
  };
}
export type PodcastCatalogueModel = ReturnType<typeof usePodcastCatalogue>;
