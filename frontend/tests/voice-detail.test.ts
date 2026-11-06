import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  formatVoiceReleaseDate,
  playbackVoiceDetail,
  voiceCreatorTarget,
  voiceCreatorTargetIsCurrent,
} from '../src/services/voiceDetail.ts';
import type { PlayerView } from '../src/services/player.ts';

function view(extra: Partial<PlayerView> = {}): PlayerView {
  return {
    epoch: '1',
    playId: 'play-a',
    resourceKind: 'voice',
    track: {
      id: '1601',
      voiceId: '601',
      name: 'Episode 601',
      artists: ['Song artist'],
      artistRefs: [{ id: '99', name: 'Wrong artist identity' }],
      album: 'Wrong album',
      local: false,
    },
    voiceDetail: {
      id: '601',
      creator: { id: '70', name: 'Official host' },
      releaseTime: new Date(2020, 1, 3, 12, 34).getTime(),
    },
    ...extra,
  } as PlayerView;
}

test('voice metadata uses native host identity and local YYYY-MM-DD, including downloaded voice', () => {
  const expected = {
    id: '601',
    title: 'Episode 601',
    creator: { id: '70', name: 'Official host' },
    releaseDate: '2020-02-03',
  };
  assert.deepEqual(playbackVoiceDetail(view()), expected);
  assert.deepEqual(playbackVoiceDetail(view({ resourceKind: 'downloaded' })), expected);
  assert.equal(formatVoiceReleaseDate(new Date(2021, 0, 2, 0, 30).getTime()), '2021-01-02');
  assert.equal(formatVoiceReleaseDate(new Date(2000, 1, 29, 23, 59).getTime()), '2000-02-29');
});

test('unknown, fractional and invalid date metadata cannot fabricate a date', () => {
  for (const value of [
    undefined,
    null,
    0,
    -1,
    0.5,
    Infinity,
    NaN,
    8.64e15 + 1,
    Number.MAX_SAFE_INTEGER,
  ])
    assert.equal(formatVoiceReleaseDate(value), null);
  assert.equal(playbackVoiceDetail(view({ voiceDetail: null }))?.releaseDate, null);
});

test('missing or invalid creator links preserve the supplied name without borrowing a song artist', () => {
  for (const id of [null, '', '0', '0070', '/user/70', 'https://invalid.example']) {
    const current = view({
      voiceDetail: {
        id: '601',
        creator: { id, name: 'Host without a valid link' },
        releaseTime: null,
      },
    });
    assert.deepEqual(playbackVoiceDetail(current)?.creator, {
      id: null,
      name: 'Host without a valid link',
    });
    assert.equal(voiceCreatorTarget(current), null);
  }
  const empty = view({ voiceDetail: { id: '601', creator: null, releaseTime: null } });
  assert.equal(playbackVoiceDetail(empty)?.creator, null);
  assert.equal(voiceCreatorTarget(empty), null);
});

test('another episode metadata is ignored while authoritative voice layout and title remain', () => {
  const current = view({
    voiceDetail: {
      id: '602',
      creator: { id: '71', name: 'Other host' },
      releaseTime: new Date(2022, 3, 4).getTime(),
    },
  });
  assert.deepEqual(playbackVoiceDetail(current), {
    id: '601',
    title: 'Episode 601',
    creator: null,
    releaseDate: null,
  });
  assert.equal(voiceCreatorTarget(current), null);
  assert.deepEqual(
    playbackVoiceDetail(view({ track: { ...current.track!, voiceId: undefined } })),
    { id: null, title: 'Episode 601', creator: null, releaseDate: null },
  );
});

test('stray voice metadata never replaces ordinary catalogue/local/cloud/downloaded song credits', () => {
  for (const resourceKind of ['catalog', 'local', 'cloud', 'downloaded'] as const) {
    const current = view({ resourceKind, track: { ...view().track!, voiceId: undefined } });
    assert.equal(playbackVoiceDetail(current), null);
    assert.equal(voiceCreatorTarget(current), null);
  }
});

test('the description keeps the native episode text and clears when the play or episode changes', () => {
  const description = '  第一段\n\n第二段 <文字>\t尾声';
  const current = view();
  const detail = { ...current.voiceDetail!, description };
  assert.equal(playbackVoiceDetail(view({ voiceDetail: detail }))?.description, description);
  assert.equal(
    playbackVoiceDetail(view({ voiceDetail: detail, resourceKind: 'downloaded' }))?.description,
    description,
  );
  for (const replacement of [
    view({ voiceDetail: detail, playId: null }),
    view({ voiceDetail: detail, track: { ...current.track!, voiceId: '602' } }),
    view({
      voiceDetail: detail,
      resourceKind: 'catalog',
      track: { ...current.track!, voiceId: undefined },
    }),
  ]) {
    assert.equal(playbackVoiceDetail(replacement)?.description, undefined);
  }
});

test('creator navigation is bound to the native account, play, voice and user identity', () => {
  const current = view();
  const target = voiceCreatorTarget(current)!;
  assert.deepEqual(target, { epoch: '1', playId: 'play-a', voiceId: '601', userId: '70' });
  assert.equal(voiceCreatorTargetIsCurrent(target, current), true);
  for (const replacement of [
    view({ epoch: '2' }),
    view({ playId: 'play-b' }),
    view({ playId: null }),
    view({ track: { ...current.track!, voiceId: '602' } }),
    view({ voiceDetail: { ...current.voiceDetail!, creator: { id: '71', name: 'New host' } } }),
  ]) {
    assert.equal(voiceCreatorTargetIsCurrent(target, replacement), false);
  }
});
