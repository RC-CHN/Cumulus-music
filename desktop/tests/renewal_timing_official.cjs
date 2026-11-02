// Optional local oracle: execute the recovered timestamp and daily-gate
// expressions. No network, cookies, SDK emulation or production test clocks.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const parser = require('../../frontend/node_modules/@babel/parser');
const path = 'references/netease-3.1.42.205594-analysis/recovered-js/modules/hybrid_vendors_app_subApp.chunk.1482700000000.js/1368.js';
const source = fs.readFileSync(path, 'utf8');
const tree = parser.parse(source);
function walk(node, visitor) {
  if (!node || typeof node !== 'object') return;
  visitor(node);
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) value.forEach(child => walk(child, visitor));
    else if (value && typeof value === 'object') walk(value, visitor);
  }
}
function method(name, marker) {
  const found = [];
  walk(tree, node => {
    if (node.type === 'ObjectExpression' && node.properties.some(p => p.key?.name === 'key' && p.value?.value === name)) {
      const fn = node.properties.find(p => p.key?.name === 'value').value;
      if (!marker || source.slice(fn.start, fn.end).includes(marker)) found.push(fn);
    }
  });
  assert.equal(found.length, 1, name);
  return found[0];
}
let stamp, gate, prepare;
walk(method('updateLoginInfo', 'jsk_token_last_refresh_time'), node => {
  if (node.type === 'LogicalExpression' && node.operator === '&&' && node.right.type === 'CallExpression' && node.right.arguments[0]?.value === 'jsk_token_last_refresh_time') stamp = node;
});
walk(method('refreshToken', 'getTokenRefreshTime'), node => {
  if (node.type === 'LogicalExpression' && node.operator === '&&' && node.left.name === 'l' && node.right.type === 'BinaryExpression' && node.right.operator === '<' && node.right.left?.operator === '-' && node.right.right.name === 'v') gate = node;
});
walk(method('prepareAndSaveLoginInfo'), node => {
  if (node.type === 'CallExpression' && node.callee.property?.name === 'updateLoginInfo' && node.arguments.length === 4) prepare = node;
});
assert(stamp && gate && prepare);
assert.equal(source.slice(prepare.arguments[3].start, prepare.arguments[3].end), 'Date.now()');
const stampExpression = source.slice(stamp.start, stamp.end);
const gateExpression = source.slice(gate.start, gate.end);
const now = 1730000000000;
const writes = [];
const update = vm.runInNewContext(`(function(...a) { let i; return (${stampExpression}); })`);
const host = {getPlatform: () => ({setItem: (key, value) => writes.push([key, value])})};
update.call(host, {}, {}, 'login', now);
assert.deepEqual(writes, [['jsk_token_last_refresh_time', JSON.stringify(now)]]);
update.call(host, {}, {}, 'restore');
update.call(host, {}, {}, 'restore', 0);
assert.equal(writes.length, 1);
const results = [-1, 0, 86399999, 86400000, 86400001].map(delta => ({deltaMs:delta, skip:Boolean(vm.runInNewContext(gateExpression, {l:now, h:now+delta, v:86400000}))}));
assert.deepEqual(results.map(r => r.skip), [true, true, true, false, false]);
console.log(JSON.stringify({ok:true, source:path, sha256:crypto.createHash('sha256').update(source).digest('hex'), loginSuppliesNativeTime:true, restoresDoNotResetTime:true, stampExpression, gateExpression, productionIntervalMs:86400000, results}, null, 2));
