import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePlaylistEssential } from '../src/composables/usePlaylistEssential.ts';
import type { AccountData, CoreClient, PlaylistEssentialPage } from '../src/services/contracts.ts';

test('an old category or account cannot restore a departed high-quality response', async () => {
  const reads: Array<[string, string, string | undefined]> = [];
  let old!: (value: AccountData<PlaylistEssentialPage>) => void;
  let pending!: (value: AccountData<PlaylistEssentialPage>) => void;
  const row = (id: string) => ({ id, name: id, coverUrl: null, playCount: null });
  const result = (
    epoch: string,
    id: string,
    continuation: string | null,
  ): AccountData<PlaylistEssentialPage> => ({
    epoch,
    value: { items: [row(id)], continuation },
  });
  const core: Pick<CoreClient, 'playlistEssentialCategories' | 'playlistEssentialPage'> = {
    playlistEssentialCategories: async (epoch) => ({ epoch, value: [] }),
    playlistEssentialPage(epoch, category, cursor) {
      reads.push([epoch, category, cursor]);
      if (category === '轻音乐') return new Promise((resolve) => (old = resolve));
      if (cursor) return new Promise((resolve) => (pending = resolve));
      return Promise.resolve(result(epoch, epoch, epoch === 'a' ? 'native-handle' : null));
    },
  };
  const model = usePlaylistEssential(core, () => assert.fail('unexpected expiry'));
  model.enter('a', '轻音乐');
  model.enter('a', '全部');
  old(result('a', 'stale-category', null));
  await Promise.resolve();
  assert.deepEqual(
    model.page.value?.items.map((item) => item.id),
    ['a'],
  );
  const continuation = model.load();
  model.enter('b', '全部');
  pending(result('a', 'old-account', null));
  await continuation;
  assert.deepEqual(
    model.page.value?.items.map((item) => item.id),
    ['b'],
  );
  assert.deepEqual(reads, [
    ['a', '轻音乐', undefined],
    ['a', '全部', undefined],
    ['a', '全部', 'native-handle'],
    ['b', '全部', undefined],
  ]);
});

test('a failed restart never falls back to an already invalid high-quality cursor', async () => {
  const reads: (string | undefined)[] = [];
  const core: Pick<CoreClient, 'playlistEssentialCategories' | 'playlistEssentialPage'> = {
    playlistEssentialCategories: async (epoch) => ({ epoch, value: [] }),
    async playlistEssentialPage(epoch, _category, cursor) {
      reads.push(cursor);
      if (reads.length === 2) throw { code: 'stale', message: '游标已失效' };
      if (reads.length === 3) throw { code: 'network', message: '读取失败' };
      return { epoch, value: { items: [], continuation: reads.length === 1 ? 'old' : null } };
    },
  };
  const model = usePlaylistEssential(core, () => assert.fail('unexpected expiry'));
  model.enter('a', '全部');
  await Promise.resolve();
  await model.load();
  await model.retry();
  assert.equal(model.error.value, '读取失败');
  await model.retry();
  assert.deepEqual(reads, [undefined, 'old', undefined, undefined]);
  assert.equal(model.page.value?.continuation, null);
});
