import { ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type { FmClient, FmDiscovery, FmDiscoveryQuery } from '../services/fm.ts';
export function useFmDiscovery(core: FmClient, expired: () => void) {
  const home = shallowRef<FmDiscovery>(),
    page = shallowRef<FmDiscovery>(),
    query = shallowRef<FmDiscoveryQuery>({ kind: 'home' });
  const loading = ref(false),
    error = ref('');
  let epoch: string | undefined,
    serial = 0,
    disposed = false;
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch) return;
    epoch = next;
    serial++;
    home.value = undefined;
    page.value = undefined;
    loading.value = false;
    error.value = '';
    query.value = { kind: 'home' };
  }
  async function load(next: FmDiscoveryQuery = query.value) {
    const owner = epoch,
      current = ++serial;
    if (!owner || disposed) return;
    const active = () => !disposed && owner === epoch && current === serial;
    query.value = next;
    page.value = next.kind === 'home' ? home.value : undefined;
    loading.value = true;
    error.value = '';
    const accept = (value: FmDiscovery) => {
      if (JSON.stringify(value.query) !== JSON.stringify(next)) return;
      page.value = value;
      if (next.kind === 'home') home.value = value;
    };
    try {
      try {
        const r = await core.fmDiscovery(owner, next, false);
        if (active() && r.epoch === owner) accept(r.value);
      } catch {
        /* A missing cache is normal; refresh below reports failures. */
      }
      if (!active()) return;
      const r = await core.fmDiscovery(owner, next, true);
      if (active() && r.epoch === owner) accept(r.value);
    } catch (e) {
      if (active()) {
        const issue = failure(e);
        error.value = issue.message;
        if (['session_expired', 'profile_changed'].includes(issue.code)) {
          page.value = undefined;
          home.value = undefined;
          expired();
        }
      }
    } finally {
      if (active()) loading.value = false;
    }
  }
  function dispose() {
    disposed = true;
    serial++;
  }
  return { home, page, query, loading, error, connect, load, dispose };
}
