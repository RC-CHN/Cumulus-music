import { test } from 'node:test';
import assert from 'node:assert/strict';
import { downloadMenuItems } from '../src/services/downloadMenu.ts';
import type { DownloadTask } from '../src/services/downloads.ts';
const task = {
  id: 'task',
  source: 'song',
  status: 'complete',
  path: '/synthetic.wav',
  track: { id: '9', name: 'Synthetic', artists: [], missing: false },
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
test('voice audio id is never treated as a song or program identifier', () => {
  const voice = { ...task, source: 'voice' } as DownloadTask;
  assert.equal(
    downloadMenuItems(voice, state).some((i) =>
      ['collect', 'share', 'copy', 'comments'].includes(i.key),
    ),
    false,
  );
  voice.track = { ...voice.track, voiceId: '77' };
  assert.equal(downloadMenuItems(voice, state).find((i) => i.key === 'comments')?.disabled, false);
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
