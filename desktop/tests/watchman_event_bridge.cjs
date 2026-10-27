// Native bridge behavior against the actual pinned SDK capability gate.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const crypto = require('node:crypto');
const vm = require('node:vm');
const test = require('node:test');
const bridge = fs.readFileSync('desktop/src/watchman/events.js', 'utf8');
const hash = value => crypto.createHash('sha256').update(value).digest('hex');
const sources = [
  ['watchman-remote', 'af456401a96e7cab2fc0ad1334e1014ed58d5c694908d2d73ec1ef426463f787', '8edd86c297d2aa0e28d80084482e511c990089cdd2b5ca8b158c2f88f1776359', 'ka', 'u'],
  ['watchman-remote-602a5ad7-20261027', '343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5', '54ddd682e2fddbeec8a86550af1224d98f3b1e305e6198b8c68e8116d00d925c', 'ma', 'w'],
].map(([directory, rawHash, readableHash, flag, nav]) => {
  const root = 'references/netease-3.1.42.205594-analysis/protocol/' + directory + '/';
  const source = fs.readFileSync(root + 'watchman.readable.js', 'utf8');
  assert.equal(hash(fs.readFileSync(root + 'watchman.min.js')), rawHash);
  assert.equal(hash(source), readableHash);
  const start = source.indexOf(`          ${flag} = !!${nav}.Mc();`);
  assert.ok(start > 0);
  const begin = source.lastIndexOf('        try {', start);
  const end = source.indexOf('\n        }', source.indexOf('catch (', start));
  return { flag, nav, code: source.slice(begin, end + 10) };
});
function host(navigator, invoke = async () => {}) {
  const bindings = new Map();
  const target = label => ({
    addEventListener(name, fn, capture) { assert.equal(capture, true); bindings.set(label + ':' + name, fn); },
    removeEventListener(name) { bindings.delete(label + ':' + name); },
  });
  const window = { ...target('window'), __TAURI_INTERNALS__: { invoke } };
  const context = vm.createContext({ window, document: target('document'), navigator });
  vm.runInContext(`(${bridge})(17);`, context);
  return { window, bindings };
}
for (const [name, method] of [
  ['standard getBattery alone stays disabled', undefined],
  ['official gate false stays disabled', () => false],
  ['official gate throwing stays disabled', () => { throw Error('unavailable'); }],
  ['official gate true enables battery', () => true],
]) {
  test(name, async () => {
    const navigator = method ? { Mc: method } : {};
    let calls = 0;
    const sent = [];
    navigator.getBattery = async () => {
      calls++;
      return { charging: true, level: 0.5, chargingTime: 60, dischargingTime: Infinity,
        addEventListener() {}, removeEventListener() {} };
    };
    const expected = sources.map(({ code, nav, flag }) => {
      const context = vm.createContext({ [nav]: navigator });
      vm.runInContext(code, context);
      return Boolean(context[flag]);
    });
    assert.equal(expected[0], expected[1]);
    const { window } = host(navigator, async (_, args) => sent.push(args.frame));
    await Promise.resolve();
    await window.__cumulusWatchmanEvents.flushed();
    assert.equal(calls, Number(expected[0]));
    assert.equal(sent.length, Number(expected[0]));
    if (sent.length) {
      assert.equal(sent[0].payload.kind, 'battery');
      assert.equal(sent[0].payload.initial, true);
      assert.equal(sent[0].payload.sample.discharging_time, null);
    }
    window.__cumulusWatchmanEvents.stop();
  });
}
test('backlog overflow is bounded and closes collection after ordered delivery', async () => {
  const sent = [];
  const { window, bindings } = host({}, async (_, args) => sent.push(args.frame));
  const observe = bindings.get('document:keydown');
  for (let i = 0; i < 400; i++) observe({ type: 'keydown', isTrusted: true,
    target: { id: 'isolated-input', value: 'private input must not cross IPC' }, key: 'x', keyCode: 88 });
  await window.__cumulusWatchmanEvents.flushed();
  assert.equal(sent.length, 257);
  assert.equal(sent.at(-1).payload.kind, 'overflow');
  assert.equal(bindings.size, 0);
  sent.forEach((frame, i) => { assert.equal(frame.sequence, i + 1); assert.equal(frame.generation, 17); });
  const encoded = JSON.stringify(sent);
  assert.ok(!encoded.includes('private input'));
  assert.ok(!encoded.includes('keyCode'));
  assert.ok(!encoded.includes('"key"'));
});
test('stop fences a late battery capability promise', async () => {
  let resolve;
  const sent = [];
  const { window } = host({ Mc: () => true, getBattery: () => new Promise(done => { resolve = done; }) },
    async (_, args) => sent.push(args.frame));
  window.__cumulusWatchmanEvents.stop();
  resolve({ addEventListener() { throw Error('late subscription'); } });
  await Promise.resolve();
  await window.__cumulusWatchmanEvents.flushed();
  assert.equal(sent.length, 0);
});
