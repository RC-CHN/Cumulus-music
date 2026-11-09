import assert from 'node:assert/strict';
import { test } from 'node:test';
import { podcastProgramPublicLink, podcastPublicLink } from '../src/services/podcast.ts';

test('episode sharing uses the program route and leaves attribution and raw URLs out', () => {
  assert.equal(
    podcastProgramPublicLink('9007199254740993'),
    'https://music.163.com/dj?id=9007199254740993',
  );
  assert.equal(podcastPublicLink('81'), 'https://music.163.com/djradio?id=81');
  for (const value of [
    '0',
    '',
    '81&userid=9',
    '81#private',
    'https://music.163.com/dj?id=81',
    ' 81',
    '81\n',
  ])
    assert.equal(podcastProgramPublicLink(value), null);
});
