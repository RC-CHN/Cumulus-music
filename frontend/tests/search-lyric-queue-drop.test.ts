import assert from 'node:assert/strict';
import { test } from 'node:test';
import { lyricQueueDropTickets } from '../src/services/searchLyricQueueDrop.ts';

function row(
  id: string,
  receipt: string,
  right: 'playable' | 'copyright_blocked' = 'playable',
  track: { local?: boolean; missing?: boolean; voiceId?: string } = {},
) {
  return {
    track: { id, local: false, missing: false, ...track },
    lyricQueueDrop: { receipt, right },
  };
}

const hits = [
  row('101', '101-first'),
  row('102', '102-blocked', 'copyright_blocked'),
  row('103', '103-first'),
  row('104', '104-outside'),
  row('103', '103-duplicate'),
];

test('drag payload keeps group gesture order, first displayed representatives, and the exact grabbed hit', () => {
  const selected = new Set(['103', '102', '101']);
  const rows = ['103-first', '102-blocked', '101-first'];
  assert.deepEqual(lyricQueueDropTickets(hits, 4, selected), { anchor: '103-duplicate', rows });
  assert.deepEqual(lyricQueueDropTickets(hits, 1, selected), { anchor: '102-blocked', rows });
  assert.equal(lyricQueueDropTickets(hits, 3, selected), undefined);
});

test('single-hit drag always uses the exact receipt and rejects a blocked singleton', () => {
  assert.deepEqual(lyricQueueDropTickets(hits, 4, new Set(['103'])), {
    anchor: '103-duplicate',
    rows: ['103-duplicate'],
  });
  assert.equal(lyricQueueDropTickets(hits, 1, new Set(['102'])), undefined);
});

test('incomplete or all-blocked lyric selections never fall through to a partial drag', () => {
  const granted = row('1', 'playable');
  const unknown = { ...row('2', 'unknown'), lyricQueueDrop: undefined };
  const later = row('2', 'later');
  assert.equal(lyricQueueDropTickets([granted, unknown, later], 2, new Set(['2', '1'])), undefined);
  assert.equal(lyricQueueDropTickets([granted], 0, new Set(['1', '2'])), undefined);
  assert.equal(
    lyricQueueDropTickets(
      [granted, later, row('1', 'blocked-duplicate', 'copyright_blocked')],
      2,
      new Set(['1', '2']),
    ),
    undefined,
  );
  const blocked = [row('1', 'one', 'copyright_blocked'), row('2', 'two', 'copyright_blocked')];
  assert.equal(lyricQueueDropTickets(blocked, 0, new Set(['1', '2'])), undefined);
  for (const invalid of [{ local: true }, { missing: true }, { voiceId: '8' }])
    assert.equal(
      lyricQueueDropTickets(
        [granted, row('2', 'foreign', 'playable', invalid)],
        0,
        new Set(['1', '2']),
      ),
      undefined,
    );
});
