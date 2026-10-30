import { ref, shallowRef } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import type {
  SearchResultQuery,
  SearchResultView,
  SearchResultsClient,
} from '../services/search.ts';

export function useSearchResults(core: SearchResultsClient, expired: () => void) {
  const view = shallowRef<SearchResultView>(),
    loading = ref(false),
    error = ref('');
  let generation = 0,
    account: AccountView | undefined;
  function connect(next: AccountView) {
    generation++;
    account = next;
    view.value = undefined;
    error.value = '';
    loading.value = false;
  }
  async function open(query: SearchResultQuery) {
    if (account?.status !== 'authenticated') return;
    const epoch = account.epoch,
      current = ++generation;
    view.value = undefined;
    loading.value = true;
    error.value = '';
    try {
      const result = await core.searchResults(epoch, query);
      if (current === generation && account?.epoch === epoch && result.epoch === epoch)
        view.value = result.value;
    } catch (e) {
      if (current !== generation) return;
      const issue = failure(e);
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
    } finally {
      if (current === generation) loading.value = false;
    }
  }
  function dispose() {
    generation++;
    account = undefined;
    view.value = undefined;
    loading.value = false;
  }
  return { view, loading, error, connect, open, dispose };
}
