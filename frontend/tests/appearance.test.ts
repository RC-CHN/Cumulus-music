import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';
import {
  accentSliders,
  readAppearance,
  resolvedAppearance,
  sliderAccent,
  storeAppearance,
} from '../src/services/appearance.ts';

const boot = readFileSync(new URL('../public/appearance.js', import.meta.url), 'utf8');

function firstPaint(read: () => unknown, dark: boolean) {
  const document = {
    documentElement: {
      dataset: {} as Record<string, string>,
      style: new Map<string, string>(),
    },
  };
  runInNewContext(boot, {
    document: {
      documentElement: {
        dataset: document.documentElement.dataset,
        style: { setProperty: (k: string, v: string) => document.documentElement.style.set(k, v) },
      },
    },
    localStorage: { getItem: read },
    matchMedia: () => ({ matches: dark }),
  });
  return document.documentElement;
}

test('first paint agrees with app for legacy, bundled, custom and invalid preferences', () => {
  for (const saved of [
    null,
    'light',
    'dark',
    'system',
    'red',
    'custom',
    'invalid',
    '{"theme":"custom","color":"#AA3377"}',
    '{"theme":"custom","color":"url(https://example.invalid)"}',
    '{"theme":"red","color":"#112233"}',
    '{"theme":"custom","color":"#123"}',
    JSON.stringify({ theme: 'custom', color: '#123abc\n' }),
    '{"theme":"custom","color":"#aa3377","padding":"' + 'x'.repeat(100) + '"}',
  ]) {
    for (const dark of [true, false]) {
      const shown = firstPaint(() => saved, dark);
      const selection = readAppearance(saved);
      assert.equal(shown.dataset.theme, resolvedAppearance(selection.theme, dark));
      assert.equal(
        shown.style.get('--custom-theme-color'),
        selection.theme === 'custom' ? selection.color : undefined,
      );
    }
  }
  assert.deepEqual(readAppearance('{"theme":"custom","color":"#AA3377"}'), {
    theme: 'custom',
    color: '#aa3377',
  });
  assert.deepEqual(readAppearance('{"theme":"custom","color":"url(https://example.invalid)"}'), {
    theme: 'system',
  });
});
test('unavailable preference storage does not block the first paint', () => {
  const shown = firstPaint(() => {
    throw new Error('storage unavailable');
  }, true);
  assert.equal(shown.dataset.theme, 'dark');
});
test('confirmed custom color is one stored value and slider endpoints produce usable colors', () => {
  const custom = { theme: 'custom', color: '#f65888' } as const;
  assert.deepEqual(readAppearance(storeAppearance(custom)), custom);
  assert.equal(storeAppearance({ theme: 'red' }), 'red');
  assert.equal(sliderAccent(0, 100), '#ff1a1a');
  assert.equal(sliderAccent(360, 100), sliderAccent(0, 100));
  assert.equal(sliderAccent(120, 0), '#000000');
  assert.deepEqual(accentSliders('#5590ea'), { hue: 216, brightness: 92 });
});
