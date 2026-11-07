import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createVoiceSongGroupDownloadBridge } from '../src/adapters/voiceSongGroupDownload.ts';
import type { DownloadResult } from '../src/services/downloads.ts';
import type {
  VoiceSongGroupDownloadConfirmation,
  VoiceSongGroupDownloadPlan,
  VoiceSongGroupDownloadReceipt,
} from '../src/services/voiceSongs.ts';

test('the group download bridge sends the frozen order and keeps start/cancel receipt-only', async () => {
  const card = {
    epoch: '1',
    playId: 'synthetic-play',
    cardReceipt: '0123456789abcdef0123456789abcdef',
  };
  const selectionReceipt = 'abcdef0123456789abcdef0123456789' as VoiceSongGroupDownloadReceipt;
  const plan: VoiceSongGroupDownloadPlan = {
    epoch: card.epoch,
    playId: card.playId,
    anchorIndex: 2,
    eligibleIndices: [1, 3],
    omittedIndices: [2],
    decision: 'ready',
    selectionReceipt,
  };
  const accepted: DownloadResult = {
    epoch: card.epoch,
    accepted: ['task-1', 'task-2'],
    failed: [],
    operationId: selectionReceipt,
  };
  const calls: [string, Record<string, unknown>][] = [];
  const responses = [plan, undefined, accepted];
  const bridge = createVoiceSongGroupDownloadBridge(
    async <T>(command: string, payload: Record<string, unknown>) => {
      calls.push([command, payload]);
      return responses.shift() as T;
    },
  );

  const indices = [1, 3, 2];
  const request = {
    ...card,
    anchorIndex: 2,
    indices,
    // Wider renderer objects must not pass display data or alternate authority.
    voiceId: '71',
    songIds: ['301', '302'],
    options: { quality: 'lossless' },
  };
  const prepared = await bridge.playerVoiceSongsDownloadPrepare(request);
  assert.equal(prepared, plan);
  assert.equal(prepared.decision, 'ready');
  indices.reverse();

  const target = { ...request, selectionReceipt: prepared.selectionReceipt };
  await bridge.playerVoiceSongsDownloadCancel(target);
  assert.equal(await bridge.playerVoiceSongsDownloadStart(target), accepted);
  const wireTarget = { ...card, selectionReceipt };
  assert.deepEqual(calls, [
    ['player_voice_songs_download_prepare', { ...card, anchorIndex: 2, indices: [1, 3, 2] }],
    ['player_voice_songs_download_cancel', wireTarget],
    ['player_voice_songs_download_start', wireTarget],
  ]);
});

test('purchased group choice is explicit and sends one quality with only the native scope', async () => {
  const card = {
    epoch: '1',
    playId: 'synthetic-play',
    cardReceipt: '0123456789abcdef0123456789abcdef',
  };
  const selectionReceipt = 'abcdef0123456789abcdef0123456789' as VoiceSongGroupDownloadReceipt;
  const plan = {
    epoch: card.epoch,
    playId: card.playId,
    anchorIndex: 0,
    eligibleIndices: [1, 0],
    omittedIndices: [],
    decision: 'purchased_quality_required',
    selectionReceipt,
    purchasedQualities: ['sky', 'hires', 'standard'],
  } satisfies VoiceSongGroupDownloadPlan;
  const blocked = { kind: 'playlist_required' } satisfies VoiceSongGroupDownloadConfirmation;
  const calls: [string, Record<string, unknown>][] = [];
  const responses = [plan, blocked];
  const bridge = createVoiceSongGroupDownloadBridge(
    async <T>(command: string, payload: Record<string, unknown>) => {
      calls.push([command, payload]);
      return responses.shift() as T;
    },
  );

  const request = { ...card, anchorIndex: 0, indices: [1, 0] };
  const prepared = await bridge.playerVoiceSongsDownloadPrepare(request);
  assert.equal(prepared.decision, 'purchased_quality_required');
  assert.deepEqual(prepared.purchasedQualities, ['sky', 'hires', 'standard']);
  assert.deepEqual(calls, [['player_voice_songs_download_prepare', request]]);
  const wide = {
    ...request,
    selectionReceipt: prepared.selectionReceipt,
    quality: 'sky',
    options: { quality: 'lossless' },
    playlistId: '81',
    songIds: ['301', '310'],
  };
  const result = await bridge.playerVoiceSongsDownloadConfirmQuality(wide, 'hires');
  assert.equal(result, blocked);
  assert.deepEqual(calls[1], [
    'player_voice_songs_download_confirm_quality',
    { ...card, selectionReceipt, quality: 'hires' },
  ]);
});
