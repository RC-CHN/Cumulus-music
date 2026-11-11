import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  preparePlaylistSongComment,
  type PlaylistSongCommentScope,
} from '../src/services/playlistSongComments.ts';

const eligible = (live = () => true): PlaylistSongCommentScope => ({
  epoch: '17',
  menu: { kind: 'eligible', receipt: 'frozen-native-row-receipt' },
  live,
});

test('menu preparation reads only the exact native playlist row receipt', async () => {
  const args: unknown[][] = [];
  const count = await preparePlaylistSongComment(
    {
      playlistSongRowCommentCount: async (...input) => {
        args.push(input);
        return { epoch: '17', value: 43 };
      },
    },
    eligible(),
    new AbortController().signal,
  );
  assert.deepEqual(args, [['17', 'frozen-native-row-receipt']]);
  assert.deepEqual(count, { count: 43 });
});

test('zero, absent counts and read failure allow the ordinary unnumbered entry', async () => {
  const signal = new AbortController().signal;
  for (const value of [null, 0, -1, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
    assert.deepEqual(
      await preparePlaylistSongComment(
        {
          playlistSongRowCommentCount: async () => ({ epoch: '17', value }),
        },
        eligible(),
        signal,
      ),
      { count: null },
    );
  }
  for (const failure of [
    new Error('transport down'),
    { code: 'queue_changed', message: 'optional count receipt was replaced' },
    { code: 'snapshot_changed', message: 'list refresh failed after invalidating receipt' },
  ]) {
    assert.deepEqual(
      await preparePlaylistSongComment(
        {
          playlistSongRowCommentCount: async () => {
            throw failure;
          },
        },
        eligible(),
        signal,
      ),
      { count: null },
    );
  }
});

test('invalidated or hidden rows do not request a count or open a prepared menu', async () => {
  const forbidden = {
    playlistSongRowCommentCount: async () => {
      assert.fail('ineligible native request');
    },
  };
  const controller = new AbortController();
  assert.equal(
    await preparePlaylistSongComment(
      forbidden,
      { ...eligible(), menu: { kind: 'hidden' } },
      controller.signal,
    ),
    undefined,
  );
  assert.equal(
    await preparePlaylistSongComment(
      forbidden,
      eligible(() => false),
      controller.signal,
    ),
    undefined,
  );
  controller.abort();
  assert.equal(
    await preparePlaylistSongComment(forbidden, eligible(), controller.signal),
    undefined,
  );

  for (const code of ['stale', 'profile_changed', 'session_expired']) {
    assert.equal(
      await preparePlaylistSongComment(
        {
          playlistSongRowCommentCount: async () => {
            throw { code, message: 'old source' };
          },
        },
        eligible(),
        new AbortController().signal,
      ),
      undefined,
    );
  }
  assert.equal(
    await preparePlaylistSongComment(
      {
        playlistSongRowCommentCount: async () => ({ epoch: '18', value: 43 }),
      },
      eligible(),
      new AbortController().signal,
    ),
    undefined,
  );
});

test('a delayed count cannot open the menu after its visit or anchor has changed', async () => {
  for (const change of ['visit', 'anchor']) {
    const controller = new AbortController();
    let current = true;
    let respond!: (value: { epoch: string; value: number }) => void;
    const response = new Promise<{ epoch: string; value: number }>((resolve) => {
      respond = resolve;
    });
    const result = preparePlaylistSongComment(
      { playlistSongRowCommentCount: () => response },
      eligible(() => current),
      controller.signal,
    );
    if (change === 'visit') current = false;
    else controller.abort();
    respond({ epoch: '17', value: 43 });
    assert.equal(await result, undefined);
  }
});
