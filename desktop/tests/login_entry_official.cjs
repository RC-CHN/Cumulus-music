// Optional source comparison. Official unpacked inputs are intentionally not in CI.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const root = path.resolve(process.argv[2] || '.');
const modules = path.join(root, 'references/netease-3.1.42.205594-analysis/recovered-js/modules');
const inputs = ['hybrid_app.chunk.1482700000000.js/541.js', 'hybrid_app.chunk.1482700000000.js/278.js'];
const dependencies = { 4: {}, 18: { a: {} }, 16: { b: {} }, 2: {} };
function factory(file) {
  const output = {};
  const require = (id) => {
    assert.ok(id in dependencies, `unexpected official dependency ${id}`);
    return dependencies[id];
  };
  require.d = (target, name, get) => Object.defineProperty(target, name, { get });
  vm.runInNewContext(fs.readFileSync(path.join(modules, file), 'utf8'), {})(null, output, require);
  return output;
}
const countries = factory(inputs[0]);
dependencies[541] = countries;
const legal = factory(inputs[1]);
const rows = Array.from(countries.b(), (row) => ({
  alpha: row.alpha, name: row.ch, englishName: row.en, calling: row.calling,
  allowedLengths: Array.from([countries.d(row.calling)].flat().filter(Boolean)),
}));
const cases = [];
for (const row of rows) {
  for (const length of [0, 1, 4, 5, 7, 8, 9, 10, 11, 12, 15, 16, 32]) {
    const phone = '1'.repeat(length);
    cases.push({ country: row.calling, phone, valid: legal.c(phone, row.calling) });
  }
}
for (const phone of ['１２３４５６７８９０１', '١٢٣٤٥٦٧٨٩٠١', '138 00000000', '+13800000000', '1\n'])
  cases.push({ country: '86', phone, valid: legal.c(phone, '86') });
const value = { cases };
const worktree = path.resolve(__dirname, '../..');
const table = path.join(worktree, 'core/src/account/login/countries.json');
const vectors = path.join(worktree, 'core/tests/fixtures/login-entry-official.json');
if (process.argv.includes('--write-vectors')) {
  fs.writeFileSync(table, JSON.stringify(rows, null, 2) + '\n');
  fs.writeFileSync(vectors, JSON.stringify(value, null, 2) + '\n');
} else {
  assert.equal(JSON.stringify(JSON.parse(fs.readFileSync(table))), JSON.stringify(rows));
  assert.equal(JSON.stringify(JSON.parse(fs.readFileSync(vectors))), JSON.stringify(value));
}
console.log(JSON.stringify({ ok: true, countryCount: rows.length, cases: cases.length,
  inputs: inputs.map((file) => ({ file, sha256: crypto.createHash('sha256').update(fs.readFileSync(path.join(modules, file))).digest('hex') })),
  scope: 'Executes only country metadata and official number predicate, not the full login SDK or any real request.' }, null, 2));
