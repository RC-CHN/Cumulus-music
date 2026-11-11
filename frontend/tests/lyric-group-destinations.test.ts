import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { TrackView } from '../src/services/contracts.ts';
import {
  lyricCollectionTracks,
  prepareLyricDownloadVisibility,
} from '../src/services/lyricGroupDestinations.ts';
import type {
  SearchGroupDownloadCommand,
  SearchGroupDownloadReply,
} from '../src/services/search.ts';

const anchor = { receipt: 'only-the-clicked-lyric-download-receipt', trackId: '19' };
const makeTrack = (id: string) => ({ id }) as TrackView;

test('collection presentation only accepts a unique ordered subset of the original group', () => {
  const selected = ['7', '2', '5'].map(makeTrack);
  const plan = {
    receipt: 'native-collect-plan',
    canCreate: true,
    selectedCount: 3,
    songIds: ['7', '5'],
  };
  assert.deepEqual(lyricCollectionTracks(plan, selected), [selected[0], selected[2]]);
  assert.deepEqual(lyricCollectionTracks({ ...plan, receipt: null, songIds: [] }, selected), []);
  for (const changed of [
    { songIds: ['5', '7'] },
    { songIds: ['7', '19'] },
    { songIds: ['7', '7'] },
    { selectedCount: 2 },
    { receipt: null },
  ]) {
    assert.equal(lyricCollectionTracks({ ...plan, ...changed }, selected), undefined);
  }
});

test('download visibility reads only the exact anchor and never starts a download', async () => {
  const seen: [string, SearchGroupDownloadCommand][] = [];
  const core = {
    async searchGroupDownload(epoch: string, command: SearchGroupDownloadCommand) {
      seen.push([epoch, command]);
      return { kind: 'lyric_menu', epoch, show: true } as const;
    },
  };
  assert.equal(
    await prepareLyricDownloadVisibility(
      core,
      'account',
      anchor,
      () => true,
      new AbortController().signal,
    ),
    true,
  );
  assert.deepEqual(seen, [['account', { kind: 'preview_lyrics_anchor', anchor }]]);
});

test('unknown or timed-out availability hides only Download, while stale scope aborts the menu', async () => {
  const current = () => true;
  const signal = new AbortController().signal;
  for (const reply of [
    { kind: 'lyric_menu', epoch: 'different-account', show: true } as const,
    { kind: 'lyric_menu', epoch: 'account', show: false } as const,
    { kind: 'pending' } as const,
  ]) {
    assert.equal(
      await prepareLyricDownloadVisibility(
        { searchGroupDownload: async () => reply },
        'account',
        anchor,
        current,
        signal,
      ),
      false,
    );
  }
  assert.equal(
    await prepareLyricDownloadVisibility(
      {
        searchGroupDownload: async () => {
          throw Error('optional preview failed');
        },
      },
      'account',
      anchor,
      current,
      signal,
    ),
    false,
  );
  const hung = { searchGroupDownload: () => new Promise<SearchGroupDownloadReply>(() => {}) };
  assert.equal(
    await prepareLyricDownloadVisibility(hung, 'account', anchor, current, signal, 0),
    false,
  );

  const cancel = new AbortController();
  const waiting = prepareLyricDownloadVisibility(hung, 'account', anchor, current, cancel.signal);
  cancel.abort();
  assert.equal(await waiting, undefined);
  let resolve!: (reply: SearchGroupDownloadReply) => void;
  const reply = new Promise<SearchGroupDownloadReply>((r) => {
    resolve = r;
  });
  let stillCurrent = true;
  const stale = prepareLyricDownloadVisibility(
    { searchGroupDownload: () => reply },
    'account',
    anchor,
    () => stillCurrent,
    signal,
  );
  await Promise.resolve();
  stillCurrent = false;
  resolve({ kind: 'lyric_menu', epoch: 'account', show: true });
  assert.equal(await stale, undefined);
});
