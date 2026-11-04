import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  equalizerPresets,
  equalizerPreset,
  equalizerPresetOption,
  equalizerChange,
  equalizerGainLabel,
  equalizerMessage,
  equalizerCanRetry,
  equalizerLauncher,
  effectLauncher,
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
test('visible and announced gain labels keep slider precision without native float tails', () => {
  assert.equal(equalizerGainLabel(Math.fround(1.79)), '1.79');
  assert.equal(equalizerGainLabel(Math.fround(-3.5)), '-3.5');
  assert.equal(equalizerGainLabel(-0), '0');
  assert.equal(equalizerGainLabel(12), '12');
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
  assert.match(equalizerMessage({ ...enabled, status: 'configured' }), /等待音频处理/);
  assert.match(equalizerMessage({ ...enabled, status: 'processing' }), /已启用/);
});

test('equal curves retain the explicitly chosen preset instead of hiding a manual or built-in selection', () => {
  const gains = Array(10).fill(0);
  const manual = { kind: 'manual' } as const;
  const builtIn = { kind: 'built_in', index: 0 } as const;
  assert.equal(equalizerPresetOption(manual, gains, gains), -1);
  assert.equal(equalizerPresetOption(builtIn, gains, gains), 0);
  assert.equal(equalizerPresetOption({ kind: 'saved' }, gains, gains), -2);
  assert.equal(equalizerPresetOption(null, gains, gains), -2);
  assert.deepEqual(equalizerChange(view(), view().settings, builtIn).preset, builtIn);
});

test('launcher never marks unavailable, damaged, flat or disabled processing as applied', () => {
  const base = view();
  const active = { ...base, settings: { enabled: true, gains: [3, ...Array(9).fill(0)] } };
  assert.equal(equalizerLauncher({ ...active, status: 'processing' }).state, 'processing');
  assert.equal(equalizerLauncher({ ...active, status: 'configured' }).state, 'configured');
  assert.equal(equalizerLauncher({ ...active, status: 'pending' }).state, 'pending');
  assert.equal(equalizerLauncher({ ...active, status: 'unavailable' }).state, 'problem');
  assert.equal(equalizerLauncher({ ...active, status: 'failed' }).state, 'problem');
  assert.equal(equalizerLauncher({ ...base, damaged: true }).state, 'problem');
  assert.equal(equalizerLauncher({ ...base, status: 'processing' }).state, 'off');
  const flat = {
    ...base,
    status: 'processing',
    settings: { ...base.settings, enabled: true },
  } as const;
  assert.equal(equalizerLauncher(flat).state, 'flat');
  const flatWithoutAdapter = { ...flat, status: 'unavailable' } as const;
  assert.equal(equalizerLauncher(flatWithoutAdapter).state, 'flat');
  assert.equal(equalizerCanRetry(flatWithoutAdapter), false);
  assert.equal(equalizerMessage(flatWithoutAdapter), '当前为平直曲线');
  assert.equal(
    equalizerLauncher({ ...flat, settings: { ...flat.settings, bass_db: 6 } }).state,
    'processing',
  );
});

test('a tone-only setting does not enable a saved curve and reports native DSP status', () => {
  const base = view();
  const gains = [4, ...Array(9).fill(0)];
  const toneOnly = { enabled: false, gains, bass_db: 6, tone_enabled: true };
  assert.deepEqual(equalizerChange(base, toneOnly).settings, toneOnly);
  assert.equal(
    equalizerLauncher({ ...base, settings: { enabled: false, gains, bass_db: 6 } }).state,
    'off',
  );
  for (const settings of [toneOnly, { ...toneOnly, enabled: true, gains: Array(10).fill(0) }]) {
    const configured = { ...base, settings, status: 'configured' } as const;
    const processing = { ...base, settings, status: 'processing' } as const;
    assert.deepEqual(equalizerLauncher(configured), {
      state: 'configured',
      label: '低音和高音增强已配置，等待音频处理',
    });
    assert.deepEqual(equalizerLauncher(processing), {
      state: 'processing',
      label: '低音和高音增强已应用',
    });
    assert.equal(equalizerMessage(configured), '低音和高音增强已配置，等待音频处理');
    assert.equal(equalizerMessage(processing), '低音和高音增强已应用');
  }
  assert.equal(
    equalizerLauncher({ ...base, settings: toneOnly, status: 'unavailable' }).state,
    'problem',
  );
  const flat = { enabled: true, gains: Array(10).fill(0), bass_db: 6, tone_enabled: false };
  assert.deepEqual(equalizerChange(base, flat).settings, flat);
  assert.equal(equalizerLauncher({ ...base, settings: flat, status: 'processing' }).state, 'flat');
});

test('master bypass preserves both stages and suppresses stale processing markers', () => {
  const base = view();
  const preset = { kind: 'built_in', index: 1 } as const;
  const settings = {
    enabled: true,
    gains: [3, ...Array(9).fill(0)],
    tone_enabled: true,
    bass_db: 6,
    master_bypassed: true,
  };
  assert.deepEqual(equalizerChange(base, settings, preset), {
    kind: 'equalizer',
    expected_revision: base.revision,
    preset,
    settings,
  });
  assert.deepEqual(equalizerLauncher({ ...base, settings, status: 'processing' }), {
    state: 'off',
    label: '均衡器和低高音增强已暂停，设置已保留',
  });
  assert.equal(equalizerLauncher({ ...base, settings, status: 'pending' }).state, 'pending');
  assert.deepEqual(equalizerLauncher({ ...base, settings, status: 'failed' }), {
    state: 'problem',
    label: '设置已保存，但未能确认音效已暂停。可重新应用。',
  });
  assert.equal(
    equalizerLauncher({
      ...base,
      settings: { ...settings, master_bypassed: false },
      status: 'processing',
    }).state,
    'processing',
  );
});

test('a requested EQ disable is not reported as applied while pending or failed', () => {
  const settings = { enabled: false, gains: [3, ...Array(9).fill(0)] };
  const base = { ...view(), settings };
  const pending = { ...base, status: 'pending' } as const;
  const failed = { ...base, status: 'failed' } as const;
  const unavailable = { ...base, status: 'unavailable' } as const;
  assert.equal(equalizerLauncher(pending).state, 'pending');
  assert.deepEqual(equalizerLauncher(failed), {
    state: 'problem',
    label: '设置已保存，但未能确认均衡器已关闭。可重新应用。',
  });
  assert.equal(equalizerCanRetry(failed), true);
  assert.deepEqual(equalizerLauncher(unavailable), { state: 'off', label: '均衡器已关闭' });
  assert.equal(equalizerMessage(unavailable), '均衡器已关闭');
  assert.equal(equalizerCanRetry(unavailable), false);
});

test('footer reports bound PEQ even while the ordinary equalizer is off', () => {
  assert.equal(effectLauncher(view()).state, 'off');
  assert.deepEqual(effectLauncher(view(), 'configured'), {
    state: 'configured',
    label: '独立参数均衡已配置',
  });
  assert.deepEqual(effectLauncher(view(), 'deferred'), {
    state: 'pending',
    label: '独立参数均衡等待音频加载',
  });
  assert.deepEqual(effectLauncher(view(), 'detach_required'), {
    state: 'problem',
    label: '独立参数均衡需要清理',
  });
  assert.deepEqual(effectLauncher(view(), 'processor_failed'), {
    state: 'problem',
    label: '独立参数均衡处理失败，已在本机停用',
  });
  assert.match(effectLauncher({ ...view(), damaged: true }, 'configured').label, /已配置.*损坏/);
});
