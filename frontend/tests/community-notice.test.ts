import assert from 'node:assert/strict';
import test from 'node:test';
import { useCommunityNotice } from '../src/composables/useCommunityNotice.ts';
import type { CommunityNoticeClient } from '../src/services/community.ts';

test('a visit replaces a pending badge and a switch discards old cards and errors', async () => {
  type Reply = Awaited<ReturnType<CommunityNoticeClient['communityNewSongNotice']>>;
  const requests: { epoch: string; mode: string; resolve: (reply: Reply) => void }[] = [];
  const core: CommunityNoticeClient = {
    communityNewSongNotice: (epoch, mode) =>
      new Promise((resolve) => requests.push({ epoch, mode, resolve })),
  };
  const model = useCommunityNotice(core, () => assert.fail('unexpected expiration'));
  model.connect('1');
  const visit = model.visit();
  await model.visit();
  assert.deepEqual(
    requests.map(({ epoch, mode }) => [epoch, mode]),
    [
      ['1', 'badge'],
      ['1', 'visit'],
    ],
  );
  const card = { style: 'artists' as const, text: '合成提示', avatars: [], cover: null };
  requests[1]!.resolve({ epoch: '1', value: { showDot: false, card } });
  await visit;
  requests[0]!.resolve({ epoch: '1', value: { showDot: true, card: null } });
  await Promise.resolve();
  assert.equal(model.dot.value, false);
  assert.deepEqual(model.card.value, card);
  const departed = model.visit();
  model.connect('2');
  requests[2]!.resolve({ epoch: '1', value: { showDot: false, card } });
  await departed;
  assert.equal(model.card.value, null);
  assert.equal(model.pending.value, false);
  requests[3]!.resolve({ epoch: '2', value: { showDot: true, card: null } });
  await Promise.resolve();
  assert.equal(model.dot.value, true);
});
