import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';
import { createMotionPreferences } from '../src/composables/useMotion.ts';
import { motionPreference, reducedMotion } from '../src/services/motion.ts';

const boot = readFileSync(new URL('../public/appearance.js', import.meta.url), 'utf8');
test('first paint and mounted motion policy agree; explicit enable never overrides system reduction', () => {
  for (const value of [null, 'system', 'disabled', 'false', 'invalid']) {
    for (const system of [false, true]) {
      const document = { documentElement: { dataset: {} as Record<string, string> } };
      runInNewContext(boot, {
        document,
        localStorage: { getItem: () => value },
        matchMedia: (query: string) => ({ matches: query.includes('reduced-motion') && system }),
      });
      const reduced = reducedMotion(motionPreference(value), system);
      assert.equal(document.documentElement.dataset.motion, reduced ? 'reduced' : 'full');
      let applied: boolean | undefined;
      const model = createMotionPreferences({
        read: () => value,
        write: () => {},
        apply: (v) => {
          applied = v;
        },
      });
      model.initialize(system);
      assert.equal(applied, reduced);
      model.select(false);
      assert.equal(applied, system);
    }
  }
});
test('motion persistence failure stays applied and is recovered by a subsequent successful choice', () => {
  let unavailable = true,
    stored: string | null = null,
    applied = false;
  const model = createMotionPreferences({
    read: () => {
      if (unavailable) throw Error('unavailable');
      return stored;
    },
    write: (v) => {
      if (unavailable) throw Error('unavailable');
      stored = v;
    },
    apply: (v) => {
      applied = v;
    },
  });
  model.initialize(false);
  assert.equal(model.saved.value, false);
  model.select(true);
  assert.equal(applied, true);
  assert.equal(stored, null);
  unavailable = false;
  model.select(true);
  assert.equal(model.saved.value, true);
  assert.equal(stored, 'disabled');
  const reopened = createMotionPreferences({
    read: () => stored,
    write: () => {},
    apply: () => {},
  });
  reopened.initialize(false);
  assert.equal(reopened.reduced.value, true);
});
test('OS changes and another window update policy without rewriting saved preferences', () => {
  let writes = 0,
    applied = false;
  const model = createMotionPreferences({
    read: () => 'disabled',
    write: () => {
      writes++;
    },
    apply: (v) => {
      applied = v;
    },
  });
  model.initialize(false);
  model.systemChanged(true);
  model.systemChanged(false);
  assert.equal(applied, true);
  model.storedChanged(null);
  assert.equal(applied, false);
  model.systemChanged(true);
  model.storedChanged('invalid');
  assert.equal(applied, true);
  assert.equal(writes, 0);
});
