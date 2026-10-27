import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSceneFm } from '../src/composables/useSceneFm.ts';
import type { SceneFmClient, SceneFmCommand, SceneFmView } from '../src/services/sceneFm.ts';
import type { AccountData } from '../src/services/contracts.ts';
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: 'test',
  avatarUrl: null,
});
const view = (revision = '1', extra: Partial<SceneFmView> = {}): SceneFmView => ({
  revision,
  activeId: 'OP_7',
  scenes: [{ id: 'OP_7', name: '新闻资讯', label: '', count: 2 }],
  episode: null,
  active: false,
  playing: false,
  pending: false,
  autoplay: false,
  ...extra,
});
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
test('场景FM迟到缓存不能跨账号显示或触发自动播放', async () => {
  const late = deferred<AccountData<SceneFmView>>();
  let commands = 0;
  const core: SceneFmClient = {
    sceneFmView: async (epoch) => (epoch === '1' ? late.promise : { epoch, value: view() }),
    sceneFmCommand: async (epoch) => {
      commands++;
      return { epoch, value: view() };
    },
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  const old = m.show(true);
  m.connect(account('2'));
  await m.load();
  late.resolve({ epoch: '1', value: view('100', { autoplay: true }) });
  await old;
  assert.equal(commands, 0);
  assert.equal(m.view.value?.revision, '1');
  m.dispose();
});
test('退出页面时未完成读取不能再自动播放', async () => {
  const late = deferred<AccountData<SceneFmView>>();
  let commands = 0;
  const core: SceneFmClient = {
    sceneFmView: async () => late.promise,
    sceneFmCommand: async (epoch) => {
      commands++;
      return { epoch, value: view() };
    },
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  const pending = m.show(true);
  await m.show(false);
  late.resolve({ epoch: '1', value: view('1', { autoplay: true }) });
  await pending;
  assert.equal(commands, 0);
  assert.equal(m.loading.value, false);
  m.dispose();
});
test('有缓存和未决选择时不自动重发；错误后只回读并保留恢复操作', async () => {
  const calls: boolean[] = [],
    commands: SceneFmCommand[] = [];
  let state = view('2', { pending: true, autoplay: true });
  const core: SceneFmClient = {
    sceneFmView: async (epoch, refresh) => {
      calls.push(refresh);
      return { epoch, value: state };
    },
    sceneFmCommand: async (epoch, command) => {
      commands.push(command);
      if (command.kind === 'play') throw { code: 'unavailable', message: '暂不可用' };
      state = view('3');
      return { epoch, value: state };
    },
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  await m.show(true);
  assert.deepEqual(calls, [false]);
  assert.equal(commands.length, 0);
  await m.act('play');
  assert.equal(m.view.value?.pending, true);
  assert.equal(m.error.value, '暂不可用');
  assert.deepEqual(calls, [false, false]);
  assert.equal(commands.length, 1);
  await m.act('cancel');
  assert.equal(m.view.value?.pending, false);
  assert.equal(commands[1]?.kind, 'cancel');
  m.dispose();
});
test('命令完成后迟到轮询和旧revision都不能覆盖新场景', async () => {
  const late = deferred<AccountData<SceneFmView>>();
  let reads = 0;
  const core: SceneFmClient = {
    sceneFmView: async (epoch) => {
      reads++;
      return reads === 2 ? late.promise : { epoch, value: view() };
    },
    sceneFmCommand: async (epoch) => ({ epoch, value: view('5', { activeId: 'OP_76001' }) }),
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  await m.show(true);
  const old = m.poll();
  await m.select('OP_76001');
  late.resolve({ epoch: '1', value: view('2') });
  await old;
  assert.equal(m.view.value?.activeId, 'OP_76001');
  await m.poll();
  assert.equal(m.view.value?.revision, '5');
  m.dispose();
});
test('自动播放只在有效页面请求中调用；设置变更不会立刻强制播放', async () => {
  const commands: SceneFmCommand[] = [];
  const core: SceneFmClient = {
    sceneFmView: async (epoch) => ({ epoch, value: view('1', { autoplay: true }) }),
    sceneFmCommand: async (epoch, command) => {
      commands.push(command);
      return { epoch, value: view('2', { autoplay: true }) };
    },
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  await m.show(true);
  assert.deepEqual(commands, [{ kind: 'automatic', revision: '1' }]);
  await m.command({ kind: 'autoplay', enabled: false });
  assert.equal(commands.length, 2);
  m.dispose();
});
test('慢回读只有一个在途请求，定时轮询不能让它永远失效', async () => {
  const late = deferred<AccountData<SceneFmView>>();
  let calls = 0;
  const core: SceneFmClient = {
    sceneFmView: async (epoch) => {
      calls++;
      return calls === 1 ? { epoch, value: view() } : late.promise;
    },
    sceneFmCommand: async (epoch) => ({ epoch, value: view() }),
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  await m.show(true);
  const old = m.poll();
  await m.poll();
  await m.poll();
  assert.equal(calls, 2);
  late.resolve({ epoch: '1', value: view('4', { playing: true, active: true }) });
  await old;
  assert.equal(m.view.value?.revision, '4');
  assert.equal(m.view.value?.playing, true);
  m.dispose();
});
