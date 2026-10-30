import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { AccountView, CoreClient } from '../src/services/contracts.ts';
import type { CatalogQuery, CatalogView } from '../src/services/catalog.ts';
import { useCatalogWorkspace } from '../src/composables/useCatalogWorkspace.ts';
const account: AccountView = {
  epoch: 'one',
  status: 'authenticated',
  nickname: null,
  avatarUrl: null,
};
const tick = () => new Promise((resolve) => setImmediate(resolve));
function view(query: CatalogQuery): CatalogView {
  let page: CatalogView['page'];
  switch (query.kind) {
    case 'artist':
      page = {
        kind: 'artist',
        artist: {
          id: query.id,
          name: 'artist',
          aliases: [],
          coverUrl: null,
          description: '',
          songCount: 55,
          albumCount: 62,
          mvCount: 2,
        },
        tracks: [],
        more: false,
      };
      break;
    case 'artist_dynamic':
      page = { kind: 'artist_dynamic', followed: false, onlineCount: 0, videoCount: 2 };
      break;
    case 'artist_albums':
      page = { kind: 'artist_albums', offset: query.offset, total: 62, more: true, albums: [] };
      break;
    case 'artist_songs':
      page = { kind: 'artist_songs', offset: query.offset, total: 55, more: true, tracks: [] };
      break;
    case 'album':
      page = {
        kind: 'album',
        album: {
          id: query.id,
          name: 'album',
          aliases: [],
          translations: [],
          description: '',
          artists: [],
          coverUrl: null,
          trackCount: 0,
          publishedAt: null,
          subscribed: false,
          explicit: false,
        },
        tracks: [],
        read: null,
      };
      break;
    default:
      throw Error('unexpected fixture query ' + query.kind);
  }
  return {
    query: { ...query },
    fetchedAt: 1,
    cached: false,
    revision: JSON.stringify(query),
    page,
  };
}
function backend(overrides: Partial<CoreClient> = {}) {
  const calls: { epoch: string; query: CatalogQuery; refresh: boolean }[] = [];
  const core = {
    catalogPage: async (epoch: string, query: CatalogQuery, refresh: boolean) => {
      calls.push({ epoch, query: { ...query }, refresh });
      return { epoch, value: view(query) };
    },
    catalogMembership: async (epoch: string, resource: unknown, action: { kind: string }) => {
      assert.equal(action.kind, 'inspect', 'navigation must never submit/reconcile a write');
      return { epoch, value: { resource, operation: null } };
    },
    commentsWriteOperations: async (epoch: string) => ({ epoch, value: [] }),
    commentsPage: async (epoch: string, resource: unknown, query: { offset: number }) => ({
      epoch,
      value: {
        resource,
        comments: [],
        hotComments: [],
        topComments: [],
        currentComment: null,
        total: 40,
        more: query.offset === 0,
        moreHot: false,
        cached: false,
        operations: [],
        next: query.offset === 0 ? { sort: 'latest', offset: 20, limit: 20, beforeTime: 1 } : null,
      },
    }),
    ...overrides,
  } as CoreClient;
  return { core, calls };
}
const artist = (visit = 1, id = '12') => ({
  resource: { kind: 'artist' as const, id },
  catalogVisit: visit,
});

test('album re-entry preserves comment page, unsent draft and locator without reconnecting its writer', async () => {
  let inspections = 0;
  const fixture = backend({
    commentsWriteOperations: async (epoch) => {
      inspections++;
      return { epoch, value: [] };
    },
  });
  const workspace = useCatalogWorkspace(fixture.core),
    route = {
      resource: { kind: 'album' as const, id: '8' },
      catalogVisit: 1,
      albumTab: 'comments' as const,
    };
  const first = workspace.open(account, route)!;
  await tick();
  await first.comments.next();
  first.commentContext.writer.draft.value = 'unsubmitted text';
  first.commentContext.target.value = '101';
  const count = inspections;
  workspace.open(account, artist(2));
  await tick();
  const returned = workspace.open(account, route)!;
  await tick();
  assert.equal(returned, first);
  assert.equal(returned.comments.query.value.offset, 20);
  assert.equal(returned.commentContext.writer.draft.value, 'unsubmitted text');
  assert.equal(returned.commentContext.target.value, '101');
  assert.equal(inspections, count);
  assert.equal(returned.comments.history.value.length, 1);
  workspace.clear();
  assert.equal(first.commentContext.writer.draft.value, '');
});

test('album replacement of metadata does not reset comment interaction identity or draft', async () => {
  const { core } = backend(),
    workspace = useCatalogWorkspace(core);
  const route = {
    resource: { kind: 'album' as const, id: '8' },
    catalogVisit: 1,
    albumTab: 'comments' as const,
  };
  const session = workspace.open(account, route)!;
  await tick();
  session.commentContext.writer.draft.value = 'draft';
  await session.refresh();
  await session.comments.load();
  assert.equal(session.albumTab.value, 'comments');
  assert.equal(session.commentContext.writer.draft.value, 'draft');
  workspace.clear();
});

test('cross-resource back retains complete artist prefixes and positions while revalidating its header', async () => {
  const { core, calls } = backend(),
    workspace = useCatalogWorkspace(core);
  const route = { ...artist(), artistTab: 'albums' as const },
    first = workspace.open(account, route)!;
  await tick();
  await first.list.more();
  first.positions.set('artist:albums', { inner: 900, outer: 220, focus: 'album:8' });
  workspace.open(account, { resource: { kind: 'album', id: '8' }, catalogVisit: 2 });
  await tick();
  const before = calls.length,
    returned = workspace.open(account, route)!;
  await tick();
  assert.equal(returned, first);
  assert.equal(returned.list.pages.value.length, 2);
  assert.deepEqual(returned.positions.get('artist:albums'), {
    inner: 900,
    outer: 220,
    focus: 'album:8',
  });
  assert.ok(calls.slice(before).some((c) => c.query.kind === 'artist' && c.refresh));
  assert.ok(!calls.slice(before).some((c) => c.query.kind === 'artist_albums'));
  workspace.clear();
});

test('a fresh visit of the same artist owns a separate viewport and loaded prefix', async () => {
  const { core } = backend(),
    workspace = useCatalogWorkspace(core);
  const first = workspace.open(account, { ...artist(1), artistTab: 'albums' })!;
  await tick();
  await first.list.more();
  first.positions.set('artist:albums', { inner: 900, outer: 200 });
  const second = workspace.open(account, { ...artist(2), artistTab: 'albums' })!;
  await tick();
  assert.notEqual(first, second);
  assert.equal(second.positions.size, 0);
  assert.equal(second.list.pages.value.length, 1);
  workspace.clear();
});

test('tab history changes reuse the session and do not revalidate the header or replay commands', async () => {
  const { core, calls } = backend(),
    workspace = useCatalogWorkspace(core);
  const first = workspace.open(account, artist())!;
  await tick();
  const count = calls.filter((c) => c.query.kind === 'artist').length;
  workspace.open(account, { ...artist(), artistTab: 'albums' });
  await tick();
  workspace.open(account, artist());
  await tick();
  assert.equal(calls.filter((c) => c.query.kind === 'artist').length, count);
  assert.equal(workspace.open(account, artist()), first);
  workspace.clear();
});

test('account changes and expiry discard every cached model, draft and viewport', async () => {
  const { core } = backend(),
    workspace = useCatalogWorkspace(core);
  const first = workspace.open(account, artist())!;
  first.commentContext.writer.draft.value = 'private draft';
  first.positions.set('artist:songs', { inner: 5, outer: 6 });
  const second = workspace.open({ ...account, epoch: 'two' }, artist())!;
  await tick();
  assert.notEqual(second, first);
  assert.equal(first.valid(), false);
  assert.equal(first.commentContext.writer.draft.value, '');
  assert.equal(first.positions.size, 0);
  assert.equal(first.detail.view.value, undefined);
  workspace.open({ ...account, epoch: 'two', status: 'expired' }, artist());
  assert.equal(second.valid(), false);
  assert.equal(second.detail.view.value, undefined);
  workspace.clear();
});

test('LRU eviction disposes models and recreates an evicted visit without an old viewport', async () => {
  const { core } = backend(),
    workspace = useCatalogWorkspace(core, () => {}, 2);
  const first = workspace.open(account, artist(1))!;
  await tick();
  workspace.open(account, artist(2));
  await tick();
  workspace.open(account, artist(3));
  await tick();
  assert.equal(first.valid(), false);
  const returned = workspace.open(account, artist(1))!;
  await tick();
  assert.notEqual(returned, first);
  assert.equal(returned.positions.size, 0);
  workspace.clear();
});

test('re-entering with failed dynamic state keeps follow disabled instead of trusting the old value', async () => {
  let fail = false;
  const base = backend();
  const core = {
    ...base.core,
    catalogPage: async (e: string, q: CatalogQuery, r: boolean) => {
      if (fail && q.kind === 'artist_dynamic' && r) throw Error('offline');
      return base.core.catalogPage(e, q, r);
    },
  };
  const workspace = useCatalogWorkspace(core),
    session = workspace.open(account, artist())!;
  await tick();
  assert.equal(session.stateReading.value, false);
  workspace.open(account);
  fail = true;
  workspace.open(account, artist());
  await tick();
  assert.equal(session.stateReading.value, true);
  assert.ok(session.dynamic.error.value);
  await session.collect(); // The fixture rejects every non-inspect membership command.
  workspace.clear();
});

test('disposing a visit fences late account-scoped reads after another identity opens the same route', async () => {
  let resolve!: (v: { epoch: string; value: CatalogView }) => void;
  const held = new Promise<{ epoch: string; value: CatalogView }>((done) => {
    resolve = done;
  });
  const base = backend(),
    core = {
      ...base.core,
      catalogPage: async (e: string, q: CatalogQuery, r: boolean) =>
        e === 'one' && q.kind === 'artist' && r ? held : base.core.catalogPage(e, q, r),
    };
  const workspace = useCatalogWorkspace(core),
    first = workspace.open(account, artist())!;
  await tick();
  const next = workspace.open({ ...account, epoch: 'two' }, artist())!;
  await tick();
  resolve({ epoch: 'one', value: view({ kind: 'artist', id: '12' }) });
  await tick();
  assert.equal(first.detail.view.value, undefined);
  assert.ok(next.detail.view.value);
  assert.equal(first.valid(), false);
  workspace.clear();
});
