import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  prepareSongRowComment,
  type SongRowCommentClient,
  type SongRowCommentScope,
} from '../src/services/songRowCommentMenu.ts';

const eligible = (
  live = () => true,
  source: SongRowCommentScope['source'] = 'playlist',
): SongRowCommentScope => ({
  source,
  epoch: '17',
  menu: { kind: 'eligible', receipt: 'frozen-native-row-receipt' },
  live,
});

const forbidden = async () => assert.fail('unexpected native comment count read');
const client = (input: Partial<SongRowCommentClient>): SongRowCommentClient => ({
  playlistSongRowCommentCount: forbidden,
  searchSongRowCommentCount: forbidden,
  ...input,
});

const preparePlaylistSongComment = (
  input: Pick<SongRowCommentClient, 'playlistSongRowCommentCount'>,
  scope: SongRowCommentScope,
  signal: AbortSignal,
) => prepareSongRowComment(client(input), scope, signal);

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

test('a dedicated Search Song reads only its native search receipt', async () => {
  const args: unknown[][] = [];
  const core = client({
    searchSongRowCommentCount: async (...input) => {
      args.push(input);
      return { epoch: '17', value: 36 };
    },
  });
  const search = eligible(() => true, 'search');
  const signal = new AbortController().signal;
  assert.deepEqual(await prepareSongRowComment(core, search, signal), { count: 36 });
  for (const menu of [undefined, { kind: 'hidden' } as const]) {
    assert.equal(await prepareSongRowComment(core, { ...search, menu }, signal), undefined);
  }
  assert.deepEqual(args, [['17', 'frozen-native-row-receipt']]);
});

test('unknown Search count leaves a plain menu; an invalidated result does not open it', async () => {
  const search = eligible(() => true, 'search');
  const signal = new AbortController().signal;
  for (const value of [0, null]) {
    const core = client({
      searchSongRowCommentCount: async () => ({ epoch: '17', value }),
    });
    assert.deepEqual(await prepareSongRowComment(core, search, signal), { count: null });
  }
  const offline = client({
    searchSongRowCommentCount: async () => {
      throw new Error('transport down');
    },
  });
  assert.deepEqual(await prepareSongRowComment(offline, search, signal), { count: null });

  for (const code of ['queue_changed', 'snapshot_changed']) {
    const retired = client({
      searchSongRowCommentCount: async () => {
        throw { code, message: 'optional count receipt retired' };
      },
    });
    assert.deepEqual(await prepareSongRowComment(retired, search, signal), { count: null });
  }
  for (const code of ['stale', 'profile_changed', 'session_expired']) {
    const core = client({
      searchSongRowCommentCount: async () => {
        throw { code, message: 'old search result' };
      },
    });
    assert.equal(await prepareSongRowComment(core, search, signal), undefined);
  }
});

test('a delayed dedicated Search count cannot outlive the row selection', async () => {
  let currentSelection = true;
  let respond!: (value: { epoch: string; value: number }) => void;
  const response = new Promise<{ epoch: string; value: number }>((resolve) => {
    respond = resolve;
  });
  const result = prepareSongRowComment(
    client({ searchSongRowCommentCount: () => response }),
    eligible(() => currentSelection, 'search'),
    new AbortController().signal,
  );
  currentSelection = false;
  respond({ epoch: '17', value: 36 });
  assert.equal(await result, undefined);
});
