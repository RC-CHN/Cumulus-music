import type { AccountView, CoreClient } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { catalogVisitKey, type CatalogLocation } from '../services/catalogNavigation.ts';
import { useCatalogSession } from './useCatalogSession.ts';

/** Account-owned LRU of models; route history does not keep rendered pages alive. */
export function useCatalogWorkspace(
  core: CoreClient = useCore(),
  expired = () => {},
  capacity = 8,
) {
  if (!Number.isInteger(capacity) || capacity < 2) throw Error('invalid catalog memory capacity');
  const sessions = new Map<string, ReturnType<typeof useCatalogSession>>();
  let owner = '',
    active = '';
  function clear() {
    active = '';
    for (const session of sessions.values()) session.dispose();
    sessions.clear();
  }
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : '';
    if (!next || owner !== next) clear();
    owner = next;
  }
  function open(account: AccountView, location?: CatalogLocation) {
    connect(account);
    if (!owner || !location) {
      active = '';
      return;
    }
    const key = catalogVisitKey(location);
    let session = sessions.get(key);
    if (!session) session = useCatalogSession(account, location.resource, core, expired);
    sessions.delete(key);
    sessions.set(key, session);
    while (sessions.size > capacity) {
      const oldest = sessions.keys().next().value!;
      sessions.get(oldest)!.dispose();
      sessions.delete(oldest);
    }
    void session.select(location);
    if (active !== key) void session.activate();
    active = key;
    return session;
  }
  return { open, clear };
}
