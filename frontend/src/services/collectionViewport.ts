import type { AccountView } from './contracts.ts';
import type { CollectionScope, CollectionSnapshot } from './collections.ts';

export interface CollectionPosition {
  scrollTop: number;
  focus?: string;
}
interface SavedScope {
  data: CollectionSnapshot;
  position: CollectionPosition;
}
interface Visit {
  albumKeyword: string;
  followKeyword: string;
  scopes: Map<CollectionScope, SavedScope>;
}
/** Bounded application memory. History stores IDs, never DOM or account credentials. */
export function createCollectionViewportMemory(capacity = 8) {
  if (!Number.isSafeInteger(capacity) || capacity < 1)
    throw Error('invalid collection memory capacity');
  const visits = new Map<number, Visit>();
  const revisions = new Map<CollectionScope, number>();
  let owner = '';
  function find(visit: number, epoch: string) {
    if (!owner || owner !== epoch) return;
    const value = visits.get(visit);
    if (value) {
      visits.delete(visit);
      visits.set(visit, value);
    }
    return value;
  }
  return {
    connect(account: AccountView) {
      const next = account.status === 'authenticated' ? account.epoch : '';
      if (!next || next !== owner) {
        visits.clear();
        revisions.clear();
      }
      owner = next;
    },
    get(visit: number, scope: CollectionScope, epoch: string) {
      return find(visit, epoch)?.scopes.get(scope);
    },
    keyword(visit: number, scope: CollectionScope, epoch: string) {
      const value = find(visit, epoch);
      return (scope === 'followed_artists' ? value?.followKeyword : value?.albumKeyword) ?? '';
    },
    revision(scope: CollectionScope) {
      return revisions.get(scope) ?? 0;
    },
    invalidate(scope: CollectionScope, epoch: string) {
      if (!owner || epoch !== owner) return;
      revisions.set(scope, (revisions.get(scope) ?? 0) + 1);
      for (const visit of visits.values()) visit.scopes.delete(scope);
    },
    set(
      visit: number,
      data: CollectionSnapshot,
      keyword: string,
      position: CollectionPosition,
      revision = revisions.get(data.scope) ?? 0,
    ) {
      if (!owner || data.epoch !== owner || !Number.isSafeInteger(visit) || visit < 0) return;
      if (revision !== (revisions.get(data.scope) ?? 0)) return;
      const value = find(visit, owner) ?? {
        albumKeyword: '',
        followKeyword: '',
        scopes: new Map(),
      };
      if (data.scope === 'followed_artists') value.followKeyword = keyword;
      else value.albumKeyword = keyword;
      value.scopes.set(data.scope, {
        data,
        position: { scrollTop: Math.max(0, position.scrollTop), focus: position.focus },
      });
      visits.set(visit, value);
      while (visits.size > capacity) visits.delete(visits.keys().next().value!);
    },
    clear() {
      visits.clear();
    },
  };
}
export type CollectionViewportMemory = ReturnType<typeof createCollectionViewportMemory>;
