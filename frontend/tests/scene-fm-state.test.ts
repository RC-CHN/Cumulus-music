import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSceneFm } from '../src/composables/useSceneFm.ts';
import type { SceneFmClient, SceneFmCommand, SceneFmView } from '../src/services/sceneFm.ts';
import type { AccountData } from '../src/services/contracts.ts';
const homeOperations = {
  async sceneFmHomeEnter(epoch: string) {
    return { epoch, value: { visit: `visit-${epoch}` } };
  },
  async sceneFmHomeLeave() {},
} satisfies Pick<SceneFmClient, 'sceneFmHomeEnter' | 'sceneFmHomeLeave'>;
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
    ...homeOperations,
    sceneFmView: async (epoch) => (epoch === '1' ? late.promise : { epoch, value: view() }),
    sceneFmCommand: async (epoch) => {
      commands++;
      return { epoch, value: view() };
    },
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  const old = m.show('classic_home');
  m.connect(account('2'));
  await m.show('classic_home');
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
    ...homeOperations,
    sceneFmView: async () => late.promise,
    sceneFmCommand: async (epoch) => {
      commands++;
      return { epoch, value: view() };
    },
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  const pending = m.show('classic_home');
  await m.show(null);
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
    ...homeOperations,
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
  await m.show('classic_home');
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
    ...homeOperations,
    sceneFmView: async (epoch) => {
      reads++;
      return reads === 2 ? late.promise : { epoch, value: view() };
    },
    sceneFmCommand: async (epoch) => ({ epoch, value: view('5', { activeId: 'OP_76001' }) }),
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  await m.show('classic_home');
  const old = m.poll();
  await m.select('OP_76001');
  late.resolve({ epoch: '1', value: view('2') });
  await old;
  assert.equal(m.view.value?.activeId, 'OP_76001');
  await m.poll();
  assert.equal(m.view.value?.revision, '5');
  m.dispose();
});
test('只有已确认的经典播客页会打开一次原生访问并请求自动播放', async () => {
  const commands: SceneFmCommand[] = [];
  const visits: string[] = [];
  const leaves: string[] = [];
  const core: SceneFmClient = {
    async sceneFmHomeEnter(epoch) {
      visits.push(epoch);
      return homeOperations.sceneFmHomeEnter(epoch);
    },
    async sceneFmHomeLeave(epoch, visit) {
      leaves.push(`${epoch}:${visit}`);
    },
    sceneFmView: async (epoch) => ({ epoch, value: view('1', { autoplay: true }) }),
    sceneFmCommand: async (epoch, command) => {
      commands.push(command);
      return { epoch, value: view('2', { autoplay: true, active: true, playing: true }) };
    },
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  for (const surface of ['fallback_home', 'standalone'] as const) {
    await m.show(surface);
    await m.load(true);
  }
  assert.deepEqual(visits, []);
  assert.deepEqual(commands, []);
  await m.show('classic_home');
  await m.show('classic_home');
  await m.load(true);
  assert.deepEqual(visits, ['1']);
  assert.deepEqual(commands, [{ kind: 'home_automatic', revision: '1', visit: 'visit-1' }]);
  await m.show(null);
  assert.deepEqual(leaves, ['1:visit-1']);
  m.dispose();
});
test('离开后收到的原生访问凭据仅释放，不读取节目或自动播放', async () => {
  const entered = deferred<AccountData<{ visit: string } | null>>();
  const leaves: string[] = [];
  let reads = 0;
  let commands = 0;
  const core: SceneFmClient = {
    sceneFmHomeEnter: () => entered.promise,
    async sceneFmHomeLeave(epoch, visit) {
      leaves.push(`${epoch}:${visit}`);
    },
    sceneFmView: async (epoch) => {
      reads++;
      return { epoch, value: view() };
    },
    sceneFmCommand: async (epoch) => {
      commands++;
      return { epoch, value: view() };
    },
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  const showing = m.show('classic_home');
  await m.show(null);
  entered.resolve({ epoch: '1', value: { visit: 'late' } });
  await showing;
  assert.deepEqual(leaves, ['1:late']);
  assert.equal(reads, 0);
  assert.equal(commands, 0);
  m.dispose();
});
test('首次目录失败后显式刷新复用经典播客访问，但不重播丢失回执的自动请求', async () => {
  const events: string[] = [];
  let directory = false;
  const core: SceneFmClient = {
    ...homeOperations,
    async sceneFmHomeEnter(epoch) {
      events.push('enter');
      return homeOperations.sceneFmHomeEnter(epoch);
    },
    async sceneFmView(epoch, refresh) {
      events.push(refresh ? 'directory' : 'cache');
      if (refresh && !directory) throw { code: 'connection', message: '目录失败' };
      return { epoch, value: view('1', { revision: directory ? '1' : null, autoplay: true }) };
    },
    async sceneFmCommand(_epoch, command) {
      assert.deepEqual(command, { kind: 'home_automatic', revision: '1', visit: 'visit-1' });
      events.push('automatic');
      throw { code: 'connection', message: '未收到自动播放回执' };
    },
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  await m.show('classic_home');
  assert.equal(m.error.value, '目录失败');
  directory = true;
  await m.load(true);
  assert.equal(m.error.value, '未收到自动播放回执');
  await m.load(true);
  assert.deepEqual(events, [
    'enter',
    'cache',
    'directory',
    'cache',
    'directory',
    'automatic',
    'cache',
    'directory',
  ]);
  m.dispose();
});
test('慢回读只有一个在途请求，定时轮询不能让它永远失效', async () => {
  const late = deferred<AccountData<SceneFmView>>();
  let calls = 0;
  const core: SceneFmClient = {
    ...homeOperations,
    sceneFmView: async (epoch) => {
      calls++;
      return calls === 1 ? { epoch, value: view() } : late.promise;
    },
    sceneFmCommand: async (epoch) => ({ epoch, value: view() }),
  };
  const m = useSceneFm(core, () => {});
  m.connect(account());
  await m.show('classic_home');
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

test('标题始终提交原生 open_voice，不用过期 active/playing 选择 toggle 或 play', async () => {
  const commands: SceneFmCommand[] = [];
  const episode = {
    id: '601',
    name: '合成节目',
    coverUrl: null,
    creator: '',
    radioId: null,
    radioName: '',
    durationMs: 120000,
    feeType: 0,
    liked: false,
    unavailable: false,
  };
  for (const stale of [
    { active: false, playing: false, pending: true },
    { active: true, playing: false, pending: false },
    { active: true, playing: true, pending: false },
  ]) {
    const core: SceneFmClient = {
      ...homeOperations,
      sceneFmView: async (epoch) => ({ epoch, value: view('7', { episode, ...stale }) }),
      sceneFmCommand: async (epoch, command) => {
        commands.push(command);
        return { epoch, value: view('8', { episode, active: true, playing: true }) };
      },
    };
    const model = useSceneFm(core, () => {});
    model.connect(account());
    await model.show('standalone');
    const result = await model.openVoice();
    assert.equal(result?.episode?.id, '601');
    assert.equal(result?.playing, true);
    assert.deepEqual(commands.at(-1), { kind: 'open_voice', revision: '7', voice_id: '601' });
    model.dispose();
  }
  assert.equal(commands.length, 3);
});

test('标题等待原生命令期间换号，旧回执不能打开新账号的节目', async () => {
  const reply = deferred<AccountData<SceneFmView>>();
  const episode = {
    id: '601',
    name: '合成节目',
    coverUrl: null,
    creator: '',
    radioId: null,
    radioName: '',
    durationMs: 120000,
    feeType: 0,
    liked: false,
    unavailable: false,
  };
  const core: SceneFmClient = {
    ...homeOperations,
    sceneFmView: async (epoch) => ({ epoch, value: view('7', { episode }) }),
    sceneFmCommand: () => reply.promise,
  };
  const model = useSceneFm(core, () => {});
  model.connect(account());
  await model.show('standalone');
  const pending = model.openVoice();
  model.connect(account('2'));
  reply.resolve({ epoch: '1', value: view('8', { episode, active: true, playing: true }) });
  assert.equal(await pending, undefined);
  assert.equal(model.view.value, undefined);
  model.dispose();
});
