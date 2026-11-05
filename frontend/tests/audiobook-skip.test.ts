import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  audiobookSkipDraft,
  audiobookSkipSetting,
  chooseAudiobookSkip,
  moveAudiobookSkip,
} from '../src/services/audiobookSkip.ts';

test('manual editing restores the reversed outro rail and only confirms after touching a slider', () => {
  const restored = audiobookSkipDraft({ kind: 'custom', start: 17, end: 27 });
  assert.equal(restored.endRail, 63);
  assert.equal(audiobookSkipSetting(restored), undefined);
  assert.deepEqual(audiobookSkipSetting(moveAudiobookSkip(restored, 'endRail', 0)), {
    kind: 'custom',
    start: 17,
    end: 90,
  });
  assert.deepEqual(audiobookSkipSetting(moveAudiobookSkip(restored, 'endRail', 90)), {
    kind: 'custom',
    start: 17,
    end: 0,
  });
});

test('mode selection preserves unsaved rail positions and returns only the official fields', () => {
  const manual = chooseAudiobookSkip(audiobookSkipDraft({ kind: 'close' }), 'custom');
  assert.equal(audiobookSkipSetting(manual), undefined);
  const changed = moveAudiobookSkip(manual, 'start', 43);
  const smart = chooseAudiobookSkip(changed, 'smart');
  assert.deepEqual(audiobookSkipSetting(smart), { kind: 'smart' });
  const manualAgain = chooseAudiobookSkip(smart, 'custom');
  assert.equal(audiobookSkipSetting(manualAgain), undefined);
  assert.deepEqual(audiobookSkipSetting(moveAudiobookSkip(manualAgain, 'endRail', 65)), {
    kind: 'custom',
    start: 43,
    end: 25,
  });
  assert.deepEqual(audiobookSkipSetting(chooseAudiobookSkip(changed, 'close')), { kind: 'close' });
});
