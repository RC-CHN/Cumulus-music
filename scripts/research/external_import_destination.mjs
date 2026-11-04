// Execute only the fixed official H5 footer and link/text submit callbacks in a
// closed VM. The page, bundled dependencies and actual request client never run.
// Usage: node scripts/research/external_import_destination.mjs /path/to/index.8910399d.js
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { runInNewContext } from 'node:vm';

const source = readFileSync(process.argv[2], 'utf8');
const sha256 = (data) => createHash('sha256').update(data).digest('hex');
assert.equal(sha256(source), '39ecd509b89e4d9dc3dbe866310536c952184e6e23bb65f5697132394d114095');
const require = createRequire(new URL('../../frontend/package.json', import.meta.url));
const formatted = await require('prettier').format(source, { parser: 'babel' });
assert.equal(sha256(formatted), 'e7b057ea9b3785efaeb98a49a4691a4b5b471042fd8ad38bd36f6bebaa8762f0');
const lines = formatted.split('\n');
const take = (first, last) => lines.slice(first - 1, last).join('\n').trim();
const evaluate = (source, context) => runInNewContext(source, context, { timeout: 1000 });

const storage = new Map();
const localStorage = {
  getItem: (key) => storage.get(key),
  setItem: (key, value) => storage.set(key, value),
};
const store = evaluate(`(${take(7783, 7789)})`, { localStorage });
const key = 'module_sort_import_to_starPl_checked';
const date = '2026/11/4 12:34:56';
const link = 'https://y.qq.com/synthetic/playlist?list=1';
const raw = '合成歌手 A&B=#?+ 50%\n合成曲目 🎵';
const report = {};
for (const lane of ['link', 'text']) {
  storage.clear();
  let selected = store(key) || false;
  assert.equal(selected, false);
  const select = (value) => { selected = value; };
  const captures = [];
  const calls = [];
  const submit = () => {
    const common = {
      F: { c: true },
      B: {
        e: async (payload) => {
          captures.push(JSON.parse(JSON.stringify(payload)));
          calls.push('create');
          return { taskId: 'synthetic' };
        },
      },
      G: { a: { fail: (message) => { throw Error(message); } } },
      T: { a: {
        importPlaylistTask: async (payload) => {
          assert.equal(payload.taskId, 'synthetic');
          calls.push('bridge');
          return { code: 200 };
        },
        open: ({ url }) => {
          assert.equal(url, 'orpheus://nm/base/startExternalPlaylist');
          calls.push('return');
        },
      } },
      window: {},
      j: () => undefined, // The official fixed entry has only appBizName.
      pt: { a: () => date },
    };
    if (lane === 'link') {
      const callback = take(3619, 3687).replace(/,$/, '');
      evaluate(`(${callback})({})`, {
        ...common, y: false, d: { current: false }, c: [link], h: selected,
        x: { a: (url) => ({ url }) },
      });
    } else {
      const callback = take(4591, 4653).replace(/,$/, '');
      evaluate(`(${callback})({})`, {
        ...common, h: false, p: { current: false }, l: raw, s: selected,
      });
    }
  };
  const footer = () => {
    const props = lane === 'link'
      ? evaluate(`({${take(3823, 3829)}})`, { h: selected, m: select, t: submit, y: false, f: false })
      : evaluate(`({${take(4693, 4699)}})`, { s: selected, f: select, t: submit, h: false, i: false });
    const definition = take(2512, 2564).replace(/^et = /, '').replace(/;$/, '');
    return evaluate(`(${definition})(props)`, {
      props,
      H: () => (callback) => callback, // Only the 100 ms debounce is bypassed.
      I: { a: { createElement: (tag, attrs, ...children) => ({ tag, attrs, children }) } },
      B: { p: store },
      X: { a: 'checked' },
      z: { a: 'unchecked' },
    }).children[0].children.filter(Boolean);
  };
  let [start, toggle] = footer();
  assert.equal(start.children[0], '开始导入');
  assert.equal(toggle.children[1].children[0], '导入我喜欢的音乐');
  assert.equal(toggle.children[0].attrs.src, 'unchecked');
  start.attrs.onClick({});
  await new Promise(setImmediate);
  toggle.attrs.onClick();
  assert.equal(store(key), 'true');
  [start, toggle] = footer();
  assert.equal(toggle.children[0].attrs.src, 'checked');
  start.attrs.onClick({});
  await new Promise(setImmediate);
  toggle.attrs.onClick();
  assert.equal(store(key), 'false');
  [start, toggle] = footer();
  assert.equal(toggle.children[0].attrs.src, 'unchecked');
  start.attrs.onClick({});
  await new Promise(setImmediate);
  assert.deepEqual(captures.map((payload) => payload.importStarPlaylist), [false, 'true', 'false']);
  assert.deepEqual(calls, Array.from({ length: 3 }, () => ['create', 'bridge', 'return']).flat());
  for (const payload of captures) {
    assert.deepEqual(Object.keys(payload).sort(), ['importStarPlaylist', 'playlistName', 'songs', 'taskIdForLog']);
    assert.deepEqual(JSON.parse(payload.songs), [{
      name: `导入音乐 ${date}`, type: '', url: encodeURI(lane === 'link' ? link : `rpc://playlist/import?text=${raw}`),
    }]);
  }
  report[lane] = { destination: captures.map((payload) => payload.importStarPlaylist), calls };
}
console.log(JSON.stringify(report));
