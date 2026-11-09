import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  createInterfaceFontCatalog,
  createInterfaceFontPreference,
} from '../src/composables/useInterfaceFont.ts';
import { interfaceFontCss, interfaceFontPage, type InterfaceFont } from '../src/services/fonts.ts';

test('the main interface applies the saved native font; rejected selections keep the accepted style', async () => {
  const saved: InterfaceFont = { family: '楷体', sizePx: 14 };
  const applied: InterfaceFont[] = [];
  const writes: (string | null)[] = [];
  const model = createInterfaceFontPreference(
    {
      read: async () => saved,
      write: async (family) => {
        writes.push(family);
        if (family) throw Error('local store unavailable');
        return { family, sizePx: 12 };
      },
    },
    (value) => applied.push(value),
  );
  await model.load();
  await model.select('楷体');
  await model.select('仿宋');
  assert.deepEqual(applied, [saved]);
  assert.equal(model.current.value.family, '楷体');
  assert.equal(model.writeError.value, true);
  await model.select(null);
  assert.deepEqual(writes, ['仿宋', null]);
  assert.equal(model.writeError.value, false);
  assert.deepEqual(interfaceFontCss(model.current.value), { family: '', size: '12px' });
  assert.deepEqual(interfaceFontCss({ family: '体"\\; url(test)', sizePx: 14 }), {
    family: '"体\\"\\\\; url(test)", var(--interface-font-fallback)',
    size: '14px',
  });
});

test('a late catalog from a retired settings visit or account cannot replace the current choices', async () => {
  const pending: ((value: { epoch: string; choices: string[] }) => void)[] = [];
  const model = createInterfaceFontCatalog({
    catalog: () => new Promise((resolve) => pending.push(resolve)),
  });
  const old = model.open('one');
  const next = model.open('two');
  pending[1]!({ epoch: 'two', choices: ['楷体'] });
  await next;
  pending[0]!({ epoch: 'one', choices: ['仿宋'] });
  await old;
  assert.deepEqual(model.choices.value, ['楷体']);
  const reopen = model.open('two');
  model.dispose();
  pending[2]!({ epoch: 'two', choices: ['宋体'] });
  await reopen;
  assert.deepEqual(model.choices.value, []);
});

test('an oversized catalog has bounded pages and retains the saved choice when its page is not shown', () => {
  const choices = Array.from({ length: 500 }, (_, index) => `字体${index}`);
  assert.equal(interfaceFontPage(choices, null, 0).length, 128);
  assert.deepEqual(interfaceFontPage(choices, '字体256', 0).slice(0, 3), [
    '字体256',
    '字体0',
    '字体1',
  ]);
  assert.deepEqual(interfaceFontPage(choices, '字体256', 2), choices.slice(256, 384));
});
