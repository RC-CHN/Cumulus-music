import assert from 'node:assert/strict';
import test from 'node:test';
import type { QueueCatalogMenu, QueueVoiceMenu } from '../src/services/player.ts';
import {
  queueFileMenuItems,
  queueSongMenuItems,
  queueVoiceMenuItems,
} from '../src/services/trackMenu.ts';

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
  directoryReceipt: 'opaque-native-receipt',
};

test('a completed song reveals only an authorized folder and never offers another download', () => {
  const menu = queueSongMenuItems(completed, false, true, false);
  const item = (key: string) => menu.find((value) => value.key === key);
  assert.equal(item('play')?.label, '暂停');
  assert.equal(item('download'), undefined);
  assert.equal(item('directory')?.label, '打开文件所在目录');
  assert.equal(
    queueSongMenuItems({ ...completed, directoryReceipt: null }, false, false, false).some(
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

test('a verified mapped file offers original location without pretending it is a download', () => {
  const menu = queueSongMenuItems({ ...completed, downloaded: false }, false, false, false);
  assert.ok(menu.some(({ key }) => key === 'directory'));
  assert.ok(menu.some(({ key }) => key === 'download'));
});

test('voice groups keep queue controls without single-episode actions or catalog collection', () => {
  const voice: QueueVoiceMenu = {
    epoch: '1',
    revision: '3',
    entryId: 'voice-entry',
    track: { ...completed.track, voiceId: '901' },
    voiceId: '901',
    sourceLabel: '合成播客',
    radioId: '71',
    creator: { id: '99', name: '合成主播' },
    commentCount: 54,
    downloaded: false,
    directoryAvailable: false,
  };
  const group = queueVoiceMenuItems(voice, true, false, false);
  assert.deepEqual(
    group.filter(({ separator }) => !separator).map(({ key }) => key),
    ['play', 'source', 'remove'],
  );
  assert.equal(group.find(({ key }) => key === 'source')?.disabled, false);
  const single = queueVoiceMenuItems(
    { ...voice, radioId: null, downloaded: null },
    false,
    true,
    true,
  );
  assert.equal(single.find(({ key }) => key === 'comments')?.label, '查看评论(54)');
  assert.equal(single.find(({ key }) => key === 'source')?.disabled, true);
  assert.equal(single.find(({ key }) => key === 'play')?.label, '暂停');
  assert.equal(single.find(({ key }) => key === 'remove')?.disabled, true);
  assert.equal(
    single.some(({ key }) => key === 'directory' || key === 'download'),
    false,
  );
});

test('file menus use only their typed native origin and preserve file actions', () => {
  const info = {
    epoch: '1',
    revision: '3',
    entryId: 'file-entry',
    track: { ...completed.track, id: 'a'.repeat(64), local: true },
  };
  const local = {
    kind: 'local' as const,
    tracks: [info.track],
    anchor: {} as HTMLElement,
    live: () => true,
    play() {},
    remove() {},
    report() {},
    collect() {},
    collectionLocked: () => false,
  };
  const cases = [
    [
      {
        ...local,
        info: { ...info, origin: { kind: 'playlist', id: '73' }, sourceLabel: '合成混合歌单' },
      },
      '来自：合成混合歌单',
    ],
    [{ ...local, info: { ...info, origin: { kind: 'playlist', id: '73' } } }, '来自：歌单'],
    [
      { ...local, info: { ...info, origin: { kind: 'local' }, sourceLabel: '不采用此字段' } },
      '来自：本地音乐',
    ],
    [{ ...local, info: { ...info, origin: { kind: 'downloads' } } }, undefined],
    [{ ...local, info }, undefined],
    [
      {
        ...local,
        kind: 'downloaded',
        downloadKind: 'song',
        info: { ...info, origin: { kind: 'downloads' } },
      },
      '来自：下载的单曲',
    ],
    [
      {
        ...local,
        kind: 'downloaded',
        downloadKind: 'voice',
        info: { ...info, origin: { kind: 'downloads' } },
      },
      '来自：下载的声音',
    ],
    [
      {
        ...local,
        kind: 'downloaded',
        downloadKind: 'song',
        info: { ...info, origin: { kind: 'playlist', id: '73' } },
      },
      undefined,
    ],
  ] as const;
  for (const [row, label] of cases) {
    const menu = queueFileMenuItems(row, true, false);
    assert.equal(menu.find(({ key }) => key === 'source')?.label, label);
    assert.ok(menu.some(({ key }) => key === 'directory'));
    assert.ok(menu.some(({ key }) => key === 'remove'));
    assert.equal(menu.find(({ key }) => key === 'play')?.label, '暂停');
  }
});
