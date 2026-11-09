import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  downloadMenuCommentResource,
  downloadMenuItems,
  downloadMenuShareTrack,
} from '../src/services/downloadMenu.ts';
import type { DownloadTask } from '../src/services/downloads.ts';
const task = {
  id: 'task',
  source: 'song',
  status: 'complete',
  path: '/synthetic.wav',
  track: { id: '9', name: 'Synthetic', artists: [], missing: false },
  commentTrackId: '9',
  linkTrackId: '9',
  collectionAvailable: true,
} as DownloadTask;
const state = { locked: false, playable: true, queueable: true };
test('downloaded song menu keeps recovered resource order and separates file additions', () => {
  const items = downloadMenuItems(task, state).filter((i) => !i.separator);
  assert.deepEqual(
    items.slice(0, 8).map((i) => i.key),
    ['play', 'next', 'comments', 'collect', 'share', 'copy', 'open', 'delete'],
  );
  assert.equal(
    items.some((i) => i.key === 'download'),
    false,
  );
  assert.deepEqual(
    items.slice(8).map((i) => i.key),
    ['rename', 'move', 'forget', 'manage'],
  );
});
test('collection is available only for a native authorized song or matched cloud task', () => {
  const collect = (row: DownloadTask) =>
    downloadMenuItems(row, state).some((item) => item.key === 'collect');
  assert.equal(collect({ ...task, source: 'cloud', collectionAvailable: true }), true);
  assert.equal(collect({ ...task, source: 'cloud', collectionAvailable: false }), false);
  assert.equal(collect({ ...task, source: 'song', collectionAvailable: false }), false);
  assert.equal(collect({ ...task, source: 'voice', collectionAvailable: true }), false);
});
test('a completed cloud file needs its native catalogue identity for the comments action', () => {
  const cloud = { ...task, source: 'cloud', commentTrackId: undefined } as DownloadTask;
  assert.equal(downloadMenuCommentResource(cloud), undefined);
  assert.equal(
    downloadMenuItems(cloud, state).some((item) => item.key === 'comments'),
    false,
  );
  const matched = { ...cloud, commentTrackId: '71' };
  assert.deepEqual(downloadMenuCommentResource(matched), { kind: 'track', id: '71' });
  assert.equal(
    downloadMenuItems(matched, { ...state, commentCount: 12 }).find(
      (item) => item.key === 'comments',
    )?.label,
    '查看评论(12)',
  );
});
test('public actions use only the native link target, independently of comments', () => {
  const removed = { ...task, linkTrackId: undefined };
  assert.deepEqual(downloadMenuCommentResource(removed), { kind: 'track', id: '9' });
  assert.equal(downloadMenuShareTrack(removed), undefined);
  const cloud = { ...removed, source: 'cloud', commentTrackId: undefined } as DownloadTask;
  assert.equal(
    downloadMenuItems(cloud, state).some((item) => ['share', 'copy'].includes(item.key)),
    false,
  );
  assert.equal(downloadMenuShareTrack({ ...cloud, linkTrackId: '71' })?.id, '71');
  assert.equal(
    downloadMenuShareTrack({ ...cloud, linkTrackId: '71?credential=synthetic' }),
    undefined,
  );
});
test('voice audio id is never treated as a song or program identifier', () => {
  const voice = { ...task, source: 'voice' } as DownloadTask;
  assert.equal(
    downloadMenuItems(voice, state).some((i) =>
      ['collect', 'share', 'copy', 'comments'].includes(i.key),
    ),
    false,
  );
  voice.track = { ...voice.track, voiceId: '77' };
  assert.equal(downloadMenuCommentResource(voice), undefined);
  const completedEpisode = { ...voice, commentVoiceId: '78' };
  assert.deepEqual(downloadMenuCommentResource(completedEpisode), { kind: 'voice', id: '78' });
  assert.equal(
    downloadMenuItems(completedEpisode, { ...state, commentCount: 24 }).find(
      (i) => i.key === 'comments',
    )?.label,
    '查看评论(24)',
  );
});
test('live status and file recovery control permitted actions', () => {
  const items = downloadMenuItems({ ...task, cleanupPending: true }, state);
  assert.equal(items.find((i) => i.key === 'play')?.disabled, false);
  for (const key of ['delete', 'rename', 'move', 'forget'])
    assert.equal(items.find((i) => i.key === key)?.disabled, true);
  assert.equal(
    downloadMenuItems({ ...task, fileOperation: 'working' }, state)
      .filter((i) => !i.separator)
      .every(
        (i) =>
          i.disabled ||
          i.key === 'manage' ||
          i.key === 'copy' ||
          i.key === 'share' ||
          i.key === 'comments',
      ),
    true,
  );
});
test('paused and failed tasks resume, running tasks pause and cancellation cannot be repeated', () => {
  for (const status of ['paused', 'error'] as const)
    assert.equal(downloadMenuItems({ ...task, status }, state)[0]?.label, '继续下载');
  assert.equal(
    downloadMenuItems({ ...task, status: 'error', needsTransferRestart: true }, state)[0]?.label,
    '从头下载',
  );
  assert.equal(downloadMenuItems({ ...task, status: 'downloading' }, state)[0]?.label, '暂停下载');
  assert.equal(
    downloadMenuItems({ ...task, status: 'cancelling' }, state).every((i) => i.disabled),
    true,
  );
});
test('a read failure locks destructive and playback choices, FM only locks insertion', () => {
  assert.equal(
    downloadMenuItems(task, { ...state, locked: true })
      .filter((i) => !i.separator)
      .every((i) => i.disabled),
    true,
  );
  const items = downloadMenuItems(task, { ...state, queueable: false });
  assert.equal(items.find((i) => i.key === 'next')?.disabled, true);
  assert.equal(items.find((i) => i.key === 'play')?.disabled, false);
});

test('a completed row with no published path cannot play or enter the queue', () => {
  const actions = downloadMenuItems({ ...task, path: null }, state);
  for (const key of ['play', 'next'])
    assert.equal(actions.find((item) => item.key === key)?.disabled, true);
});
