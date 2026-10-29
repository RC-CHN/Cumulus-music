import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useFonts } from '../src/composables/useFonts.ts';
import { fontStack } from '../src/services/fonts.ts';
import type { FontCatalog } from '../src/services/fonts.ts';
const catalog: FontCatalog = {
  epoch: '1',
  families: Array.from({ length: 12005 }, (_, i) => ({
    name: `Font ${i}`,
    aliases: [`字体 ${i}`],
  })),
  fallbacks: ['Fallback'],
};
test('large font directory is bounded; localized aliases can be searched and preserved', async () => {
  const model = useFonts({ read: async () => catalog }, '1');
  await model.read();
  assert.equal(model.rows.value.length, 20);
  assert.equal(model.pages.value, 601);
  model.page.value = 600;
  assert.equal(model.rows.value.length, 5);
  model.query.value = '  字体 12004 ';
  model.page.value = 0;
  assert.deepEqual(
    model.rows.value.map((v) => v.name),
    ['Font 12004'],
  );
  assert.equal(model.installed('font 12004'), true);
  assert.equal(model.installed('字体 12004'), true);
  assert.equal(model.installed('Missing'), false);
  assert.equal(fontStack(['A"B', 'C\\D', 'A"B', '']), '"A\\"B", "C\\\\D", system-ui, sans-serif');
});
test('closed dialog ignores late native result and failure; stale account cannot populate fonts', async () => {
  let resolve!: (value: FontCatalog) => void;
  let requests = 0;
  const model = useFonts(
    {
      read: () => {
        requests++;
        return new Promise((r) => {
          resolve = r;
        });
      },
    },
    '1',
  );
  const pending = model.read();
  await model.read();
  assert.equal(requests, 1);
  model.dispose();
  resolve(catalog);
  await pending;
  assert.equal(model.catalog.value, undefined);
  await model.read();
  assert.equal(requests, 1);
  const stale = useFonts({ read: async () => catalog }, '2');
  await stale.read();
  assert.equal(stale.catalog.value, undefined);
  assert.ok(stale.error.value);
});
test('native enumeration failure remains explicit until user retries', async () => {
  let requests = 0;
  const model = useFonts(
    {
      read: async () => {
        if (++requests === 1) throw Error('native failure');
        return catalog;
      },
    },
    '1',
  );
  await model.read();
  assert.ok(model.error.value);
  assert.equal(model.loading.value, false);
  assert.equal(requests, 1);
  await model.read();
  assert.equal(requests, 2);
  assert.equal(model.error.value, '');
  assert.equal(model.rows.value.length, 20);
});
