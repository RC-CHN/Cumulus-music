import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useFm } from '../src/composables/useFm.ts';
import type { FmClient, FmView, FmTrash, FmReasons } from '../src/services/fm.ts';
import type { AccountData } from '../src/services/contracts.ts';
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { resolve, promise };
}
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: 'test',
  avatarUrl: null,
});
const view = (revision = '1'): FmView => ({
  source: { kind: 'personal' },
  activeSource: null,
  modePending: false,
  revision,
  items: [],
  current: null,
  active: false,
  pending: false,
  notice: null,
  operations: [],
  blockedArtists: [],
});
const empty: FmTrash = { page: 1, reportedTotal: 0, songs: [], more: false, cursor: '' };
function client(overrides: Partial<FmClient> = {}): FmClient {
  return {
    fmDiscovery: async () => {
      throw new Error('not loaded');
    },
    fmReasons: async (epoch, _source, entryId) => ({
      epoch,
      value: { entryId, songId: '1', options: [] },
    }),
    fmView: async (epoch) => ({ epoch, value: view() }),
    fmCommand: async (epoch) => ({ epoch, value: view('2') }),
    fmTrash: async (epoch) => ({ epoch, value: empty }),
    ...overrides,
  };
}
test('关闭原因窗口和切号都会拒绝迟到的原因列表', async (t) => {
  const first = deferred<AccountData<FmReasons>>(),
    second = deferred<AccountData<FmReasons>>();
  let calls = 0;
  const model = useFm(
    client({ fmReasons: () => (++calls === 1 ? first.promise : second.promise) }),
  );
  t.after(() => model.dispose());
  model.connect(account());
  const a = model.loadReasons('one');
  model.clearReasons();
  first.resolve({ epoch: '1', value: { entryId: 'one', songId: '1', options: [] } });
  await a;
  assert.equal(model.reasons.value, undefined);
  const b = model.loadReasons('two');
  model.connect(account('2'));
  second.resolve({ epoch: '1', value: { entryId: 'two', songId: '2', options: [] } });
  await b;
  assert.equal(model.reasons.value, undefined);
  assert.equal(model.reasonsLoading.value, false);
});
test('切换模式使旧页面和原因响应失效', async (t) => {
  const late = deferred<AccountData<FmView>>();
  const model = useFm(
    client({
      fmView: async (epoch, source) =>
        source.kind === 'personal' ? late.promise : { epoch, value: { ...view('zone'), source } },
    }),
  );
  t.after(() => model.dispose());
  model.connect(account());
  const old = model.load(true);
  await model.select({ kind: 'zone', rule_id: '27001', trial_mode: null }, '摇滚漫游');
  late.resolve({ epoch: '1', value: view('old') });
  await old;
  assert.equal(model.view.value?.source.kind, 'zone');
  assert.equal(model.title.value, '摇滚漫游');
});
test('进入FM页面跟随重启后实际播放的模式', async (t) => {
  const zone = { kind: 'zone' as const, rule_id: '27001', trial_mode: null };
  const model = useFm(
    client({
      fmView: async (epoch, source) => ({
        epoch,
        value: { ...view(), source, activeSource: zone, active: source.kind === 'zone' },
      }),
    }),
  );
  t.after(() => model.dispose());
  model.connect(account());
  await model.load(true);
  assert.deepEqual(model.source.value, zone);
  assert.equal(model.view.value?.active, true);
});
test('切号后旧卡片选择不能触发新账号自动播放', async (t) => {
  const late = deferred<AccountData<FmView>>();
  const model = useFm(
    client({
      fmView: async (epoch) => (epoch === '1' ? late.promise : { epoch, value: view('new') }),
    }),
  );
  t.after(() => model.dispose());
  model.connect(account());
  const old = model.select({ kind: 'personal' }, '旧入口');
  model.connect(account('2'));
  await model.load(true);
  late.resolve({ epoch: '1', value: view('old') });
  assert.equal(await old, false);
  assert.equal(model.view.value?.revision, 'new');
});
test('原因响应必须属于请求的entry', async (t) => {
  const model = useFm(
    client({
      fmReasons: async (epoch) => ({
        epoch,
        value: { entryId: 'other', songId: '2', options: [] },
      }),
    }),
  );
  t.after(() => model.dispose());
  model.connect(account());
  await model.loadReasons('one');
  assert.equal(model.reasons.value, undefined);
});
test('FM迟到的账号读取不覆盖新账号', async (t) => {
  const late = deferred<AccountData<FmView>>();
  const model = useFm(
    client({
      fmView: (epoch) =>
        epoch === '1' ? late.promise : Promise.resolve({ epoch, value: view('new') }),
    }),
  );
  t.after(() => model.dispose());
  model.connect(account());
  const old = model.load(true);
  model.connect(account('2'));
  await model.load();
  late.resolve({ epoch: '1', value: view('old') });
  await old;
  assert.equal(model.view.value?.revision, 'new');
  assert.equal(model.loading.value, false);
});
test('FM写操作使此前轮询失效', async (t) => {
  const late = deferred<AccountData<FmView>>();
  const model = useFm(client({ fmView: () => late.promise }));
  t.after(() => model.dispose());
  model.connect(account());
  const read = model.load();
  await model.command({ kind: 'start', quality: 'standard' });
  late.resolve({ epoch: '1', value: view('old') });
  await read;
  assert.equal(model.view.value?.revision, '2');
});
test('FM写入期间重新打开页面不会启动旧状态读取', async (t) => {
  const late = deferred<AccountData<FmView>>();
  let reads = 0;
  const model = useFm(
    client({
      fmCommand: () => late.promise,
      fmView: async (epoch) => {
        reads++;
        return { epoch, value: view('old') };
      },
    }),
  );
  t.after(() => model.dispose());
  model.connect(account());
  const write = model.command({ kind: 'start', quality: 'standard' });
  model.show(true);
  await model.load(true);
  assert.equal(reads, 0);
  late.resolve({ epoch: '1', value: view('new') });
  await write;
  assert.equal(model.view.value?.revision, 'new');
});
test('FM未知写入只刷新核对入口，不自动重放', async (t) => {
  let writes = 0;
  const v = view();
  v.operations = [
    { id: 'op', kind: 'trash', phase: 'uncertain', desired: true, canApply: false, artistId: null },
  ];
  const model = useFm(
    client({
      fmCommand: async () => {
        writes++;
        throw { code: 'write_uncertain', message: '请核对' };
      },
      fmView: async (epoch) => ({ epoch, value: v }),
    }),
  );
  t.after(() => model.dispose());
  model.connect(account());
  await model.command({ kind: 'trash', entry_id: 'old' });
  assert.equal(writes, 1);
  assert.equal(model.view.value?.operations[0]?.phase, 'uncertain');
  assert.equal(model.error.value, '请核对');
  assert.equal(model.busy.value, false);
});
test('恢复推荐使迟到的旧列表失效，下一页沿用游标', async (t) => {
  const late = deferred<AccountData<FmTrash>>();
  let calls = 0;
  const args: unknown[] = [];
  const model = useFm(
    client({
      fmTrash: async (epoch, page, cursor) => {
        args.push([page, cursor]);
        calls++;
        return calls === 1
          ? late.promise
          : { epoch, value: { ...empty, page, cursor: 99, more: page === 1 } };
      },
    }),
  );
  t.after(() => model.dispose());
  model.connect(account());
  const old = model.loadTrash();
  await model.command({ kind: 'restore_trash', ids: ['1'] });
  await model.loadTrash();
  late.resolve({ epoch: '1', value: { ...empty, cursor: 'old' } });
  await old;
  assert.equal(model.trash.value?.cursor, 99);
  await model.loadTrash(true);
  assert.deepEqual(args.at(-1), [2, 99]);
});
test('会话失效清除FM和减少推荐列表', async (t) => {
  let expired = 0;
  const model = useFm(
    client({
      fmView: async () => {
        throw { code: 'session_expired', message: 'expired' };
      },
    }),
    () => expired++,
  );
  t.after(() => model.dispose());
  model.connect(account());
  await model.loadTrash();
  await model.load();
  assert.equal(expired, 1);
  assert.equal(model.view.value, undefined);
  assert.equal(model.trash.value, undefined);
});
