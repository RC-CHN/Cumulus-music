import assert from 'node:assert/strict';
import { test } from 'node:test';
import { playerEffectTag } from '../src/services/playerEffectTag.ts';
import type { EqualizerView } from '../src/services/player.ts';

const account = { epoch: 'account-1', status: 'authenticated' } as const;
type Playback = NonNullable<Parameters<typeof playerEffectTag>[2]>;
const settings: EqualizerView['settings'] = { enabled: false, gains: Array<number>(10).fill(0) };
const equalizer: EqualizerView = { revision: '1', settings, status: 'configured', damaged: false };
const idle: Playback = { epoch: account.epoch, resourceKind: 'catalog', equalizer };
const label = (view: Playback) => playerEffectTag('active_only', account, view)?.label;
const eq = (
  settingsChange: Partial<EqualizerView['settings']>,
  change: Partial<EqualizerView> = {},
) => ({
  ...idle,
  equalizer: { ...equalizer, settings: { ...settings, ...settingsChange }, ...change },
});

test('legacy icon, hidden modern group and modern voice eligibility are independent of EQ selection', () => {
  const voice = { ...eq({ enabled: true }), resourceKind: 'voice' as const };
  const downloadedVoice = {
    ...voice,
    resourceKind: 'downloaded' as const,
    track: { voiceId: '15' },
  };
  assert.deepEqual(playerEffectTag('legacy', account, voice), { kind: 'legacy', label: '音效' });
  assert.equal(playerEffectTag('hidden', account, eq({ enabled: true })), undefined);
  assert.equal(playerEffectTag('always', account, voice), undefined);
  assert.equal(playerEffectTag('always', account, downloadedVoice), undefined);
  assert.equal(label({ ...downloadedVoice, track: {} }), '均衡器');
});

test('modern t1 shows a default tag while t2 requires a confirmed selection', () => {
  assert.deepEqual(playerEffectTag('always', account, idle), { kind: 'modern', label: '音效' });
  assert.equal(label(idle), undefined);
  assert.equal(label(eq({ enabled: true })), '均衡器');
  assert.equal(label(eq({ enabled: true }, { source_bypassed: true })), '均衡器');
  assert.equal(label(eq({ enabled: true, master_bypassed: true })), undefined);
  assert.equal(label(eq({ enabled: true }, { damaged: true })), undefined);
  assert.equal(label(eq({ enabled: true }, { status: 'failed' })), undefined);
  assert.equal(label(eq({ gains: [2, ...settings.gains.slice(1)] })), undefined);
  assert.equal(label(eq({ bass_db: 2, tone_enabled: true })), '均衡器');
  assert.equal(label(eq({ bass_db: 2, tone_enabled: false })), undefined);
  assert.equal(label(eq({}, { spatial_status: 'configured' })), '均衡器');
});

test('headphone and applied catalogue labels require matching account and confirmed state', () => {
  const headphone: NonNullable<Playback['deviceParametric']> = {
    brand: 1,
    model: 2,
    activationId: 'synthetic',
    status: 'bypassed_for_source',
  };
  const selected = { ...idle, deviceParametric: headphone };
  assert.equal(label(selected), '耳机音效');
  assert.equal(
    label({ ...selected, deviceParametric: { ...headphone, status: 'paused_locally' } }),
    undefined,
  );
  assert.equal(label({ ...selected, epoch: 'retired' }), undefined);
  assert.equal(
    playerEffectTag('active_only', { ...account, status: 'expired' }, selected),
    undefined,
  );
  assert.equal(label({ ...idle, boundParametric: 'configured' }), undefined);
  assert.equal(label({ ...idle, appliedEffectId: 54, boundParametric: 'deferred' }), undefined);
  assert.equal(label({ ...idle, appliedEffectId: 54, boundParametric: 'configured' }), '音效');
  assert.equal(label({ ...selected, equalizer: eq({ enabled: true }).equalizer }), '音效');
});
