import { test } from 'node:test';
import assert from 'node:assert/strict';
import { libraryView, saveLibraryView, libraryPage } from '../src/services/playlistLibrary.ts';
test('library preferences retain only view modes; account navigation is always reset', () => {
  const state = libraryView({
    getItem: () =>
      JSON.stringify({
        createdMode: 'list',
        subscribedMode: 'grid',
        createdPage: 99,
        scrollTop: 800,
        focusId: 'private-id',
      }),
  });
  assert.deepEqual(state, {
    createdMode: 'list',
    subscribedMode: 'grid',
    createdPage: 1,
    subscribedPage: 1,
    scrollTop: 0,
    focusId: '',
  });
  state.focusId = 'private-id';
  state.createdPage = 7;
  let saved = '';
  saveLibraryView(state, {
    setItem: (_key, value) => {
      saved = value;
    },
  });
  assert.deepEqual(JSON.parse(saved), { createdMode: 'list', subscribedMode: 'grid' });
  assert.equal(libraryView({ getItem: () => '{invalid' }).createdMode, 'grid');
  assert.equal(libraryPage(9, 21), 2);
  assert.equal(libraryPage(3, 0), 1);
});
