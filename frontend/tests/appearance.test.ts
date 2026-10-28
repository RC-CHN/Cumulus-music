import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';
import { appearancePreference, resolvedAppearance } from '../src/services/appearance.ts';

const boot = readFileSync(new URL('../public/appearance.js', import.meta.url), 'utf8');
test('first-paint theme agrees with the app for saved, missing and invalid preferences', () => {
  for (const saved of [null, 'light', 'dark', 'system', 'invalid']) {
    for (const dark of [true, false]) {
      const document = { documentElement: { dataset: {} as Record<string, string> } };
      runInNewContext(boot, {
        document,
        localStorage: { getItem: () => saved },
        matchMedia: () => ({ matches: dark }),
      });
      assert.equal(
        document.documentElement.dataset.theme,
        resolvedAppearance(appearancePreference(saved), dark),
      );
    }
  }
});
test('unavailable preference storage does not block the first paint', () => {
  const document = { documentElement: { dataset: {} as Record<string, string> } };
  runInNewContext(boot, {
    document,
    localStorage: {
      getItem: () => {
        throw new Error('storage unavailable');
      },
    },
    matchMedia: () => ({ matches: true }),
  });
  assert.equal(document.documentElement.dataset.theme, 'dark');
});
