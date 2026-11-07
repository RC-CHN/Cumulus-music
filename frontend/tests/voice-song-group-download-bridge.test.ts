import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createVoiceSongGroupDownloadBridge } from '../src/adapters/voiceSongGroupDownload.ts';
import type { DownloadResult } from '../src/services/downloads.ts';
import type {
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
