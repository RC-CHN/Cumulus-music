import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useCatalog } from '../src/composables/useCatalog.ts';
import type { CatalogQuery, CatalogView } from '../src/services/catalog.ts';

const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: '合成账号',
  avatarUrl: null,
};
const target: CatalogQuery = { kind: 'artist_introduction', id: '12' };
function view(query = target, cached = false): CatalogView {
  return {
    query,
    fetchedAt: 1,
    cached,
    revision: 'order',
    page: { kind: 'artist_introduction', briefDescription: '简介', sections: [] },
  };
}
const reply = (value: CatalogView | null, epoch = '1') => ({ epoch, value });
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

test('saved page renders while a required remote refresh is pending', async () => {
  const remote = deferred<ReturnType<typeof reply>>();
  const calls: boolean[] = [];
  const model = useCatalog({
    catalogPage: async (_epoch, _query, refresh) => {
      calls.push(refresh);
      return refresh ? remote.promise : reply(view(target, true));
    },
  });
  model.connect(account);
  const pending = model.open(target);
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(calls, [false, true]);
  assert.equal(model.view.value?.cached, true);
  assert.equal(model.loading.value, true);
  await model.open({ ...target }, true);
  assert.deepEqual(calls, [false, true]);
  remote.resolve(reply(view()));
  await pending;
  assert.equal(model.loading.value, false);
  assert.equal(model.view.value?.cached, false);
});

test('a failed refresh keeps the recovery page; retry replaces it without another cache read', async () => {
  const calls: boolean[] = [];
  let fail = true;
  const model = useCatalog({
    catalogPage: async (_e, _q, refresh) => {
      calls.push(refresh);
      if (refresh && fail) throw { code: 'network', message: '暂时离线' };
      return reply(view(target, !refresh));
    },
  });
  model.connect(account);
  await model.open(target);
  assert.equal(model.error.value, '暂时离线');
  assert.equal(model.view.value?.cached, true);
  fail = false;
  await model.refresh();
  assert.deepEqual(calls, [false, true, true]);
  assert.equal(model.error.value, '');
  assert.equal(model.view.value?.cached, false);
});

test('null cache is a miss, while null network result is a read failure', async () => {
  const model = useCatalog({ catalogPage: async () => reply(null) });
  model.connect(account);
  await model.open(target);
  assert.equal(model.view.value, undefined);
  assert.ok(model.error.value);
  assert.equal(model.loading.value, false);
});

test('a damaged or mismatched cache can recover through the network', async () => {
  for (const corrupt of [false, true]) {
    let calls = 0;
    const model = useCatalog({
      catalogPage: async (_e, _q, refresh) => {
        calls++;
        if (!refresh && corrupt) throw { code: 'storage', message: '损坏的缓存' };
        return reply(view(refresh ? target : { ...target, id: '13' }, !refresh));
      },
    });
    model.connect(account);
    await model.open(target);
    assert.equal(calls, 2);
    assert.equal(model.view.value?.query.id, '12');
    assert.equal(model.error.value, '');
  }
});

test('a mismatched remote resource or account never replaces a known page', async () => {
  for (const wrongAccount of [false, true]) {
    const model = useCatalog({
      catalogPage: async (_e, _q, refresh) =>
        refresh
          ? reply(view(wrongAccount ? target : { ...target, id: '13' }), wrongAccount ? '2' : '1')
          : reply(view(target, true)),
    });
    model.connect(account);
    await model.open(target);
    assert.equal(model.view.value?.query.id, '12');
    assert.equal(model.view.value?.cached, true);
    assert.ok(model.error.value);
  }
});

test('account changes and disposal suppress late results and clear displayed metadata', async () => {
  for (const dispose of [false, true]) {
    const late = deferred<ReturnType<typeof reply>>();
    const calls: boolean[] = [];
    const model = useCatalog({
      catalogPage: async (_e, _q, refresh) => {
        calls.push(refresh);
        return late.promise;
      },
    });
    model.connect(account);
    const pending = model.open(target);
    if (dispose) model.dispose();
    else model.connect({ ...account, epoch: '2' });
    late.resolve(reply(view(target, true)));
    await pending;
    assert.equal(model.view.value, undefined);
    assert.equal(model.loading.value, false);
    assert.deepEqual(calls, [false]);
  }
});

test('late navigation cannot overwrite the new resource', async () => {
  const late = deferred<ReturnType<typeof reply>>();
  const model = useCatalog({
    catalogPage: async (_e, q, refresh) =>
      q.id === '12' ? late.promise : reply(view(q, !refresh)),
  });
  model.connect(account);
  const first = model.open(target);
  await model.open({ ...target, id: '13' });
  late.resolve(reply(view(target, true)));
  await first;
  assert.equal(model.view.value?.query.id, '13');
});

test('session expiry during cache read is surfaced without another network request', async () => {
  let calls = 0,
    expired = 0;
  const model = useCatalog(
    {
      catalogPage: async () => {
        calls++;
        throw { code: 'session_expired', message: '登录已失效' };
      },
    },
    () => expired++,
  );
  model.connect(account);
  await model.open(target);
  assert.equal(calls, 1);
  assert.equal(expired, 1);
  assert.equal(model.view.value, undefined);
  assert.equal(model.error.value, '登录已失效');
});
