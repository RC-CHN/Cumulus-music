import { test } from 'node:test';
import assert from 'node:assert/strict';
import { formatPlaybackCommentCount } from '../src/services/playbackCommentCount.ts';

test('the current comment badge follows the recovered display ranges', () => {
  const counts = [0, 1, 999, 1_000, 9_999, 10_000, 99_999, 100_000, 999_999, 1_000_000];
  assert.deepEqual(counts.map(formatPlaybackCommentCount), [
    '',
    '1',
    '999',
    '999+',
    '999+',
    '1w+',
    '1w+',
    '10w+',
    '10w+',
    '100w+',
  ]);
});
