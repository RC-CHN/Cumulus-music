import assert from 'node:assert/strict';
import { test } from 'node:test';
import { usePlaylistTagBrowse } from '../src/composables/usePlaylistTagBrowse.ts';
import type { AccountData, PlaylistTagPage, PlaylistTagQuery } from '../src/services/contracts.ts';

test('tag pages append only supplied continuation IDs and retain the list when returning from detail', async () => {
  const requests: PlaylistTagQuery[] = [];
  const item = (id: string) => ({ id, name: id, coverUrl: null });
  const model = usePlaylistTagBrowse({
    playlistTagPage: async (epoch, query) => {
      requests.push(query);
      return {
        epoch,
        value:
          query.kind === 'category'
            ? { items: [item('1')], remainingIds: ['25', '26'] }
            : { items: query.ids.map(item), remainingIds: [] },
      };
    },
  });
  await model.enter('e1', '轻音乐');
  await model.load();
  assert.deepEqual(requests, [
    { kind: 'category', tag: '轻音乐' },
    { kind: 'items', ids: ['25', '26'] },
  ]);
  model.remember('26');
  model.saveScroll('轻音乐', 1240);
  model.enter('e1', null);
  await model.enter('e1', '轻音乐');
  assert.deepEqual(
    model.items.value?.map((item) => item.id),
    ['1', '25', '26'],
  );
  assert.deepEqual(model.restore(), { opener: '26', scroll: 1240 });
  await model.load();
  assert.equal(requests.length, 2);
});

test('an old tag read cannot publish into a different account or tag', async () => {
  let resolve!: (value: AccountData<PlaylistTagPage>) => void;
  const model = usePlaylistTagBrowse({
    playlistTagPage: (epoch) =>
      epoch === 'e1'
        ? new Promise((done) => {
            resolve = done;
          })
        : Promise.resolve({ epoch, value: { items: [], remainingIds: [] } }),
  });
  const read = model.enter('e1', '轻音乐');
  await model.enter('e2', '古典');
  resolve({
    epoch: 'e1',
    value: { items: [{ id: '1', name: '旧歌单', coverUrl: null }], remainingIds: [] },
  });
  await read;
  assert.deepEqual(model.items.value, []);
});

test('a bare square link retains only a confirmed same-account choice category', async () => {
  const model = usePlaylistTagBrowse({
    playlistTagPage: async (epoch, query) => {
      if (query.kind === 'category' && query.tag === '失败') throw Error('not loaded');
      return { epoch, value: { items: [], remainingIds: [] } };
    },
  });
  model.enter('one', null);
  assert.equal(model.bareChoiceTag('one'), '推荐');
  model.rememberChoiceTag('one', '官方');
  assert.equal(model.bareChoiceTag('one'), '推荐');
  await model.enter('one', '官方');
  model.enter('one', null);
  await model.enter('one', '轻音乐');
  assert.equal(model.bareChoiceTag('one'), '官方', 'non-choice tag read does not replace it');
  model.rememberChoiceTag('one', '失败');
  await model.enter('one', '失败');
  assert.equal(model.bareChoiceTag('one'), '推荐');
  model.rememberChoiceTag('one', '官方');
  assert.equal(model.bareChoiceTag('other'), '推荐');
  model.enter('other', null);
  await model.enter('other', '官方');
  assert.equal(model.bareChoiceTag('other'), '推荐', 'switching accounts also forgets the choice');
});
