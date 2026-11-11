import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  currentSongDownloadPicker,
  currentSongDownloadScope,
  type CurrentSongDownloadCommand,
  type CurrentSongDownloadOwner,
  type CurrentSongDownloadReply,
} from '../src/services/currentSongDownload.ts';
import { defaultDownloadOptions } from '../src/services/downloads.ts';
import type { PlaylistView } from '../src/services/contracts.ts';

const owner: CurrentSongDownloadOwner = {
  epoch: 'owner',
  target: { playId: 'play-7', entryId: 'queue-entry-14' },
  trackId: '28',
};
const ignoreGuide = async () => {};
const identity = { epoch: owner.epoch, ...owner.target, trackId: owner.trackId };
const playlist: PlaylistView = {
  id: '50',
  owned: true,
  favorite: false,
  private: true,
  name: 'fixture',
  coverUrl: null,
  creator: 'me',
  description: '',
  trackCount: 0,
};

test('retire a late purchased offer after the exact active song changed', async () => {
  const received: CurrentSongDownloadCommand[] = [];
  let live = true;
  let finish!: (reply: CurrentSongDownloadReply) => void;
  const pending = new Promise<CurrentSongDownloadReply>((resolve) => (finish = resolve));
  const scope = currentSongDownloadScope(
    {
      openCurrentSongPurchasedQualityGuide: ignoreGuide,
      async playerCurrentSongDownload(epoch, target, command) {
        assert.equal(epoch, owner.epoch);
        assert.deepEqual(target, owner.target);
        received.push(command);
        return command.kind === 'prepare' ? pending : { ...identity, kind: 'cancelled' };
      },
    },
    owner,
    () => live,
  );
  const preparing = scope.call({ kind: 'prepare' });
  live = false;
  finish({
    ...identity,
    kind: 'plan',
    decision: 'purchased_quality_required',
    offer: 'one-time offer',
    purchasedQualities: ['lossless'],
  });
  assert.equal(await preparing, undefined);
  assert.deepEqual(received, [{ kind: 'prepare' }, { kind: 'cancel', receipt: 'one-time offer' }]);
});

test('unverified replies cannot present an ordinary or purchased branch', async () => {
  let reply: CurrentSongDownloadReply = {
    ...identity,
    trackId: null,
    kind: 'plan',
    decision: 'ordinary',
  };
  const scope = currentSongDownloadScope(
    {
      playerCurrentSongDownload: async () => reply,
      openCurrentSongPurchasedQualityGuide: ignoreGuide,
    },
    owner,
    () => true,
  );
  assert.equal(await scope.call({ kind: 'prepare' }), undefined);
  reply = { ...identity, entryId: 'another entry', kind: 'plan', decision: 'ordinary' };
  assert.equal(await scope.call({ kind: 'prepare' }), undefined);
  reply = { ...identity, trackId: null, kind: 'plan', decision: 'unavailable' };
  assert.equal(await scope.call({ kind: 'prepare' }), reply);
  reply = { ...identity, kind: 'plan', decision: 'ordinary' };
  assert.equal(await scope.call({ kind: 'prepare' }), reply);
});

test('current picker only submits a minted destination and reconciles without repeating the write', async () => {
  const received: CurrentSongDownloadCommand[] = [];
  const selection = 'exact native selection';
  const scope = currentSongDownloadScope(
    {
      openCurrentSongPurchasedQualityGuide: ignoreGuide,
      async playerCurrentSongDownload(epoch, target, command) {
        assert.equal(epoch, owner.epoch);
        assert.deepEqual(target, owner.target);
        received.push(command);
        if (command.kind === 'destinations')
          return {
            ...identity,
            kind: 'destinations',
            rows: [{ receipt: 'own destination capability', playlist }],
            nextPage: null,
            canCreate: false,
          };
        const picked = {
          playlistId: playlist.id,
          operationId: 'native playlist write operation',
          addFailed: false,
        };
        if (command.kind === 'choose_playlist')
          return {
            ...identity,
            kind: 'picker',
            picker: { ...picked, phase: 'uncertain', continuation: null },
          };
        if (command.kind === 'picker_status')
          return {
            ...identity,
            kind: 'picker',
            picker: { ...picked, phase: 'confirmed', continuation: 'verified continuation' },
          };
        if (command.kind === 'start')
          return {
            ...identity,
            kind: 'started',
            result: {
              epoch: owner.epoch,
              operationId: null,
              accepted: ['native download task'],
              failed: [],
            },
          };
        throw new Error('unexpected scoped download command');
      },
    },
    owner,
    () => true,
  );
  const options = { ...defaultDownloadOptions, quality: 'lossless' as const };
  const picker = currentSongDownloadPicker(scope, selection, options);
  assert.equal(
    await picker({ kind: 'choose', receipt: selection, playlistId: playlist.id }),
    undefined,
  );
  const page = await picker({ kind: 'destinations', receipt: selection, sort: 'default', page: 0 });
  assert.equal(page?.kind, 'destinations');
  assert.equal(page.systemLikedReceipt, null);
  assert.equal(page.canCreate, false);
  assert.equal(await picker({ kind: 'choose', receipt: selection, playlistId: '51' }), undefined);
  assert.equal(
    await picker({ kind: 'choose', receipt: 'unrelated receipt', playlistId: playlist.id }),
    undefined,
  );
  assert.equal(
    await picker({ kind: 'choose_liked', receipt: selection, destinationReceipt: 'not issued' }),
    undefined,
  );
  const lost = await picker({ kind: 'choose', receipt: selection, playlistId: playlist.id });
  assert.equal(lost?.kind, 'playlist');
  assert.equal(lost.result.continuation, null);
  const observed = await picker({ kind: 'reconcile', receipt: selection });
  assert.equal(observed?.kind, 'playlist');
  assert.equal(observed.result.continuation, 'verified continuation');
  assert.equal(
    await picker({
      kind: 'start_from_playlist',
      receipt: selection,
      continuation: 'wrong continuation',
    }),
    undefined,
  );
  const start = await picker({
    kind: 'start_from_playlist',
    receipt: selection,
    continuation: observed.result.continuation,
  });
  assert.equal(start?.kind, 'scoped_accepted');
  assert.deepEqual(start.result.accepted, ['native download task']);
  assert.equal(start.result.operationId, null);
  assert.equal(
    await picker({
      kind: 'start_from_playlist',
      receipt: selection,
      continuation: observed.result.continuation,
    }),
    undefined,
  );
  assert.deepEqual(received, [
    { kind: 'destinations', selection, sort: 'default', page: 0 },
    { kind: 'choose_playlist', selection, destination: 'own destination capability' },
    { kind: 'picker_status', selection, reconcile: true },
    { kind: 'start', selection, options, continuation: 'verified continuation' },
  ]);
});

test('purchased help forwards the exact offer and stops when the player leaves that scope', async () => {
  let live = true;
  const opened: unknown[] = [];
  const scope = currentSongDownloadScope(
    {
      playerCurrentSongDownload: async () => {
        throw new Error('guide must be a distinct action');
      },
      async openCurrentSongPurchasedQualityGuide(...args) {
        opened.push(args);
      },
    },
    owner,
    () => live,
  );
  await scope.openGuide('');
  await scope.openGuide('live offer');
  live = false;
  await scope.openGuide('late offer');
  assert.deepEqual(opened, [[owner.epoch, owner.target, 'live offer']]);
});
