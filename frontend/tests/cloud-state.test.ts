import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { nextCloudSort, type CloudQuery } from '../src/services/cloud.ts';
import { useCloud } from '../src/composables/useCloud.ts';
import type { AccountView } from '../src/services/contracts.ts';
import type { CloudClient, CloudPage, CloudResult } from '../src/services/cloud.ts';

test('cloud title five-state and other three-state cycles match the executed official hooks', () => {
  const oracle = JSON.parse(
    readFileSync(new URL('../../core/tests/fixtures/cloud_sort.json', import.meta.url), 'utf8'),
  );
  let state: Pick<CloudQuery, 'sort' | 'descending'> = { sort: 'uploaded', descending: true };
  const fields: Record<string, CloudQuery['sort']> = {
    trackName: 'title',
    artist: 'artist',
    albumName: 'album',
    uploadTime: 'uploaded',
    type: 'format',
    size: 'size',
  };
  for (const step of oracle.transitions) {
    state = nextCloudSort(state, step.column);
    assert.deepEqual(state, {
      sort: step.projected.off ? 'natural' : fields[step.projected.id],
      descending: step.projected.flag === 'reverse',
    });
  }
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
const account = (epoch = '1'): AccountView => ({
  epoch,
  status: 'authenticated',
  nickname: null,
  avatarUrl: null,
});
function page(epoch = '1', total = 0, offset = 0): CloudPage {
  return {
    epoch,
    total,
    offset,
    revision: 'snapshot',
    updatedMs: 1,
    size: 1024,
    maxSize: 1048576,
    uploadedCount: total,
    uploadingCount: 0,
    tracks: [],
    uploads: [],
    deletions: [],
    workerError: null,
  };
}
const turn = () => new Promise((r) => setImmediate(r));

test('cloud account switch discards pending reads, writes and file selections without refreshing the previous account', async () => {
  const read = deferred<CloudPage>(),
    write = deferred<CloudResult>(),
    files = deferred<string[]>();
  const requests: string[] = [];
  const model = useCloud({
    cloudPage: async (e, _q, remote) => {
      requests.push(`${e}:${remote}`);
      return e === '1' ? read.promise : page(e);
    },
    cloudCommand: () => write.promise,
    pickCloudFiles: () => files.promise,
  } as CloudClient);
  await model.connect(account());
  const showing = model.show(true);
  const writing = model.command({ kind: 'pause_all' });
  const picking = model.pick('audio');
  await model.connect(account('2'));
  read.resolve(page('1', 500));
  write.resolve({ epoch: '1', accepted: ['old'], failed: [], deletion: null });
  files.resolve(['/private/old-account.mp3']);
  await showing;
  assert.equal(await writing, undefined);
  assert.deepEqual(await picking, []);
  assert.equal(model.view.value?.epoch, '2');
  assert.equal(model.notice.value, '');
  assert.equal(model.busy.value, false);
  assert.deepEqual(requests, ['1:false', '2:false', '2:true']);
  model.dispose();
});

test('cloud cached results survive refresh failure and only a successful remote read clears the error', async () => {
  let fail = true,
    expired = 0;
  const model = useCloud(
    {
      cloudPage: async (e, _q, remote) => {
        if (remote && fail) throw { code: 'session_expired', message: '登录已过期' };
        return page(e, 49);
      },
    } as CloudClient,
    () => expired++,
  );
  await model.connect(account());
  await model.show(true);
  assert.equal(model.view.value?.total, 49);
  assert.equal(model.readError.value, '登录已过期');
  assert.equal(expired, 1);
  await model.refresh();
  assert.equal(model.readError.value, '登录已过期');
  fail = false;
  await model.refresh(true);
  assert.equal(model.readError.value, '');
  model.dispose();
});

test('cloud latest query wins and deletion of the last page falls back within the official 200 row page size', async () => {
  const old = deferred<CloudPage>(),
    offsets: number[] = [];
  const model = useCloud({
    cloudPage: async (e, q) => {
      offsets.push(q.offset);
      if (q.keyword === 'old') return old.promise;
      return page(e, 205, q.offset);
    },
  } as CloudClient);
  await model.connect(account());
  model.filter({ keyword: 'old' });
  model.filter({ keyword: 'new', offset: 400 });
  await turn();
  assert.equal(model.query.value.offset, 200);
  assert.equal(model.view.value?.total, 205);
  old.resolve(page('1', 999));
  await turn();
  assert.equal(model.view.value?.total, 205);
  assert.deepEqual(offsets, [0, 400, 200]);
  model.dispose();
});

test('cloud uncertain deletion is presented as uncertain and never retried by refresh or duplicate clicks', async () => {
  const result = deferred<CloudResult>();
  const commands: string[] = [];
  const model = useCloud({
    cloudPage: async (e) => page(e, 49),
    cloudCommand: (e, input) => {
      commands.push(input.kind);
      if (input.kind === 'delete') return result.promise;
      return Promise.resolve({
        epoch: e,
        accepted: ['op'],
        failed: [],
        deletion: { id: 'op', phase: 'confirmed', total: 1, removed: 1, remaining: 0 },
      });
    },
  } as CloudClient);
  await model.connect(account());
  const pending = model.command({ kind: 'delete', ids: ['new'], revision: 'snapshot' });
  assert.equal(
    await model.command({ kind: 'delete', ids: ['new'], revision: 'snapshot' }),
    undefined,
  );
  result.resolve({
    epoch: '1',
    accepted: [],
    failed: [],
    deletion: { id: 'op', phase: 'uncertain', total: 1, removed: 0, remaining: 1 },
  });
  await pending;
  assert.match(model.notice.value, /尚未确认/);
  await model.refresh();
  await model.refresh(true);
  assert.deepEqual(commands, ['delete']);
  await model.command({ kind: 'reconcile_delete', id: 'op' });
  assert.match(model.notice.value, /已确认删除 1/);
  assert.deepEqual(commands, ['delete', 'reconcile_delete']);
  model.dispose();
});

test('cloud navigation and disposal do not cancel native uploads or trigger late remote reads', async () => {
  const pending = deferred<CloudPage>();
  let reads = 0,
    writes = 0;
  const model = useCloud({
    cloudPage: () => {
      reads++;
      return pending.promise;
    },
    cloudCommand: async () => {
      writes++;
      throw Error('unexpected');
    },
  } as CloudClient);
  await model.connect(account());
  const showing = model.show(true);
  await model.show(false);
  model.dispose();
  pending.resolve(page('1', 49));
  await showing;
  assert.equal(reads, 1);
  assert.equal(writes, 0);
  assert.equal(model.view.value, undefined);
});
