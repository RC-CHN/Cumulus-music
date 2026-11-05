import assert from 'node:assert/strict';
import test from 'node:test';
import {
  confirmLyricContributorTarget,
  lyricContributorRows,
} from '../src/services/lyricContributors.ts';

const playback = { epoch: '7', playId: 'loaded-song' };
const document = {
  ...playback,
  lyricContributor: { nickname: '合成 <滚动歌词作者>', userId: '9007199254740993' },
  translationContributor: { nickname: '合成译者', userId: '88' },
};
const target = lyricContributorRows(document, playback)[0].target!;

test('credits keep the official order and link only a valid current profile ID', () => {
  assert.deepEqual(
    lyricContributorRows(document, playback).map(({ label, nickname, target }) => [
      label,
      nickname,
      target?.userId,
    ]),
    [
      ['滚动歌词贡献者：', '合成 <滚动歌词作者>', '9007199254740993'],
      ['翻译贡献者：', '合成译者', '88'],
    ],
  );
  for (const userId of [null, '0', '../88', '8'.repeat(129)]) {
    const [row] = lyricContributorRows(
      {
        ...document,
        lyricContributor: { nickname: '不可链接作者', userId },
      },
      playback,
    );
    assert.equal(row.nickname, '不可链接作者');
    assert.equal(row.target, undefined);
  }
  assert.deepEqual(lyricContributorRows(document, { ...playback, epoch: '8' }), []);
  assert.deepEqual(lyricContributorRows(document, { ...playback, playId: 'new-song' }), []);
});

test('a displayed credit needs the same native playback and current cached lyric before navigation', async () => {
  const active = () => document;
  const sync = async () => playback;
  assert.equal(
    await confirmLyricContributorTarget(target, active, sync, async () => document),
    true,
  );
  assert.equal(
    await confirmLyricContributorTarget(target, active, sync, async () => ({
      ...document,
      lyricContributor: null,
    })),
    false,
  );

  let cachedReads = 0;
  assert.equal(
    await confirmLyricContributorTarget(
      target,
      active,
      async () => ({ ...playback, playId: 'already-changed' }),
      async () => {
        cachedReads++;
        return document;
      },
    ),
    false,
  );
  assert.equal(cachedReads, 0);

  let display: typeof document | undefined = document;
  assert.equal(
    await confirmLyricContributorTarget(
      target,
      () => display,
      sync,
      async () => {
        display = undefined;
        return document;
      },
    ),
    false,
  );
});
