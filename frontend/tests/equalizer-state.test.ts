import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  equalizerPresets,
  equalizerPreset,
  equalizerChange,
  equalizerMessage,
} from '../src/services/equalizer.ts';
import type { EqualizerView } from '../src/services/player.ts';
const view = (): EqualizerView => ({
  revision: '9007199254740993',
  settings: { enabled: false, gains: Array(10).fill(0) },
  status: 'configured',
  damaged: false,
});
test('official presets retain all decimal gains and match native float32 roundtrips', () => {
  assert.equal(equalizerPresets.length, 24);
  equalizerPresets.forEach((p, index) => {
    assert.equal(p.gains.length, 10);
    assert.ok(p.gains.every((g) => Math.abs(g) <= 12));
    assert.equal(equalizerPreset(p.gains.map(Math.fround)), index);
  });
  assert.equal(equalizerPreset([1, 2]), -1);
});
test('full curve edits keep opaque revision and copy the mutable draft', () => {
  const current = view();
  const gains = Array(10).fill(0);
  gains[2] = 3.25;
  const command = equalizerChange(current, { enabled: true, gains });
  gains[2] = -9;
  assert.equal(command.kind, 'equalizer');
  if (command.kind === 'equalizer') {
    assert.equal(command.expected_revision, current.revision);
    assert.equal(command.settings.gains[2], 3.25);
  }
  assert.equal(current.settings.gains[2], 0);
});
test('invalid or damaged settings cannot be silently replaced through an ordinary edit', () => {
  for (const gains of [[], Array(10).fill(NaN), Array(10).fill(12.01)])
    assert.throws(() => equalizerChange(view(), { enabled: true, gains }));
  assert.throws(() =>
    equalizerChange({ ...view(), damaged: true }, { enabled: true, gains: Array(10).fill(0) }),
  );
});
test('saved-but-failed and missing adapters never appear as successfully enabled', () => {
  const enabled = { ...view(), settings: { enabled: true, gains: Array(10).fill(2) } };
  assert.match(equalizerMessage({ ...enabled, status: 'failed' }), /未能应用/);
  assert.match(equalizerMessage({ ...enabled, status: 'unavailable' }), /未就绪/);
  assert.match(equalizerMessage({ ...enabled, status: 'pending' }), /等待/);
  assert.match(equalizerMessage({ ...enabled, damaged: true }), /损坏/);
  assert.match(equalizerMessage({ ...enabled, status: 'processing' }), /已启用/);
});
