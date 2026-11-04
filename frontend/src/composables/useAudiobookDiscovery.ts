import { ref, shallowRef, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import type { AudiobookCategory, PodcastCard } from '../services/podcast';
import { useCore } from '../services/core';

export function useAudiobookDiscovery(account: () => AccountView, expired: () => void) {
  const core = useCore();
  const categories = shallowRef<AudiobookCategory[]>([]);
  const books = shallowRef<PodcastCard[]>([]);
  const subId = ref('');
  const pending = ref(false);
  const error = ref('');
  const continuation = ref<string>();
  const complete = ref(false);
  let seen = new Set<string>();
  let request = 0;
  watch(
    () => [account().epoch, account().status],
    () => {
      request++;
      categories.value = [];
      subId.value = '';
      clearBooks();
      pending.value = false;
      error.value = '';
    },
    { flush: 'sync' },
  );
  function clearBooks() {
    books.value = [];
    continuation.value = undefined;
    complete.value = false;
    seen = new Set();
  }
  function live(epoch: string, id: number) {
    return id === request && epoch === account().epoch && account().status === 'authenticated';
  }
  function issue(value: unknown) {
    const issue = failure(value);
    error.value = issue.message;
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
  }
  async function activate() {
    if (pending.value || categories.value.length || account().status !== 'authenticated') return;
    const epoch = account().epoch;
    const id = ++request;
    pending.value = true;
    error.value = '';
    try {
      const result = await core.audiobookDiscovery(epoch, { kind: 'categories' });
      if (live(epoch, id) && result.epoch === epoch && result.value.kind === 'categories') {
        categories.value = result.value.categories;
        subId.value = result.value.categories[0]?.id ?? '';
      }
    } catch (value) {
      if (live(epoch, id)) issue(value);
    } finally {
      if (live(epoch, id)) pending.value = false;
    }
    if (live(epoch, id) && subId.value) await load();
  }
  async function load() {
    if (!subId.value || pending.value || complete.value || account().status !== 'authenticated')
      return;
    const epoch = account().epoch;
    const id = ++request;
    pending.value = true;
    error.value = '';
    try {
      const result = await core.audiobookDiscovery(epoch, {
        kind: 'books',
        subId: subId.value,
        continuation: continuation.value,
      });
      if (!live(epoch, id) || result.epoch !== epoch || result.value.kind !== 'books') return;
      const next = result.value.continuation;
      if (next && seen.has(next)) {
        error.value = '目录未继续更新，请重新读取当前分类';
        complete.value = true;
        return;
      }
      if (next) seen.add(next);
      const identities = new Set(books.value.map((card) => card.id));
      books.value = [
        ...books.value,
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
    if ((!refresh && subId.value === id) || !categories.value.some((item) => item.id === id))
      return;
    request++;
    subId.value = id;
    clearBooks();
    pending.value = false;
    void load();
  }
  return {
    categories,
    books,
    subId,
    pending,
    error,
    complete,
    continuation,
    activate,
    load,
    choose,
  };
}
export type AudiobookDiscoveryModel = ReturnType<typeof useAudiobookDiscovery>;
