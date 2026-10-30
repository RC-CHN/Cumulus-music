import { test } from 'node:test';
import assert from 'node:assert/strict';
import { sidebarPreferences } from '../src/services/sidebar.ts';
test('sidebar folding accepts only explicit false and discards account navigation', () => {
  assert.deepEqual(sidebarPreferences({ getItem: () => '{bad' }), {
    created: true,
    subscribed: true,
  });
  assert.deepEqual(
    sidebarPreferences({
      getItem: () =>
        JSON.stringify({
          created: false,
          subscribed: true,
          selectedId: 'private',
          scrollTop: 5000,
        }),
    }),
    { created: false, subscribed: true },
  );
  assert.deepEqual(
    sidebarPreferences({ getItem: () => JSON.stringify({ created: 'false', subscribed: null }) }),
    { created: true, subscribed: true },
  );
});
