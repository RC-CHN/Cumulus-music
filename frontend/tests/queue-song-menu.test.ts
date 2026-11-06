import assert from 'node:assert/strict';
import test from 'node:test';
import type { QueueCatalogMenu } from '../src/services/player.ts';
import { queueSongMenuItems } from '../src/services/trackMenu.ts';

const completed: QueueCatalogMenu = {
  epoch: '1',
  revision: '3',
  entryId: 'song-entry',
  track: {
    id: '14',
    name: '合成歌曲',
    artists: ['仅有署名'],
    artistRefs: [{ id: null, name: '仅有署名' }],
    album: '',
    albumId: null,
    coverUrl: null,
    durationMs: 1000,
    fee: 0,
    local: false,
    missing: false,
  },
  sourceLabel: '搜索',
  origin: null,
  commentCount: null,
  downloaded: true,
  downloadTaskId: 'verified-task',
};

test('a completed song reveals only an authorized folder and never offers another download', () => {
  const menu = queueSongMenuItems(completed, false, true, false);
  const item = (key: string) => menu.find((value) => value.key === key);
  assert.equal(item('play')?.label, '暂停');
  assert.equal(item('download'), undefined);
  assert.equal(item('directory')?.label, '打开文件所在目录');
  assert.equal(
    queueSongMenuItems({ ...completed, downloadTaskId: null }, false, false, false).some(
      ({ key }) => key === 'download' || key === 'directory',
    ),
    false,
  );
  assert.equal(item('artist')?.disabled, true);
  assert.equal(item('album')?.label, '专辑: 未知');
  assert.equal(item('album')?.disabled, true);
  assert.equal(item('source')?.label, '来自：搜索');
  assert.equal(item('source')?.disabled, true);
});
