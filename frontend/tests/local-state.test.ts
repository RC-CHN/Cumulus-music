import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useLocal } from '../src/composables/useLocal.ts';
import { usePlayer } from '../src/composables/usePlayer.ts';
import type { CoreClient } from '../src/services/contracts.ts';
import type { LocalClient, LocalPage, LocalQuery } from '../src/services/local.ts';
const turn = () => new Promise((r) => setImmediate(r));
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { resolve, promise };
}
function page(revision = 'a', total = 0): LocalPage {
  return {
    revision,
    total,
    playable: total,
    tracks: [],
    groups: [],
    roots: [],
    offset: 0,
    more: false,
  };
}
function fixture(overrides: Partial<LocalClient> = {}) {
  return {
    localPage: async () => page(),
    localCommand: async () => {},
    pickLocalDirectory: async () => null,
    ...overrides,
  } satisfies LocalClient;
}
test('late page cannot overwrite a newer filter or reopen hidden polling', async () => {
  const delayed = deferred<LocalPage>();
  let calls = 0;
  const model = useLocal(
    fixture({ localPage: async () => (++calls === 1 ? delayed.promise : page('new', 3)) }),
  );
  model.show(true);
  model.filter({ keyword: 'chosen' });
  await turn();
  delayed.resolve(page('old', 8));
  await turn();
  assert.equal(model.view.value?.revision, 'new');
  assert.equal(model.query.value.keyword, 'chosen');
  model.show(false);
  model.dispose();
});
test('changed index resets pagination instead of combining incompatible snapshots', async () => {
  const queries: LocalQuery[] = [];
  const model = useLocal(
    fixture({
      localPage: async (q) => {
        queries.push(q);
        if (q.revision) throw { code: 'snapshot_changed', message: 'changed' };
        return page('new', 205);
      },
    }),
  );
  await model.refresh();
  model.page(200);
  await turn();
  await turn();
  assert.equal(model.query.value.offset, 0);
  assert.equal(model.query.value.revision, null);
  assert.equal(queries[1]?.revision, 'new');
  assert.equal(queries[2]?.offset, 0);
  assert.match(model.notice.value, /已更新/);
  model.dispose();
});
test('directory picker cancellation and disposal never enqueue a watch', async () => {
  const pending = deferred<string | null>();
  let writes = 0;
  const model = useLocal(
    fixture({
      pickLocalDirectory: () => pending.promise,
      localCommand: async () => {
        writes++;
      },
    }),
  );
  const operation = model.choose();
  model.dispose();
  pending.resolve('/fixture');
  await operation;
  assert.equal(writes, 0);
  const cancel = useLocal(
    fixture({
      localCommand: async () => {
        writes++;
      },
    }),
  );
  await cancel.choose();
  assert.equal(writes, 0);
  cancel.dispose();
});
test('guest player connects while stale account streams still disconnect', async () => {
  let stopped = 0;
  const pending = deferred<() => void>();
  let calls = 0;
  const core = {
    watchPlayer: async () => {
      calls++;
      return calls === 1
        ? pending.promise
        : () => {
            stopped++;
          };
    },
  } as unknown as CoreClient;
  const model = usePlayer(core);
  const old = model.connect({ epoch: '0', status: 'unknown', nickname: null, avatarUrl: null });
  await model.connect({ epoch: '1', status: 'expired', nickname: null, avatarUrl: null });
  pending.resolve(() => {
    stopped++;
  });
  await old;
  assert.equal(stopped, 1);
  assert.equal(model.ready.value, true);
  model.dispose();
  assert.equal(stopped, 2);
});
