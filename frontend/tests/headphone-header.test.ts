import assert from 'node:assert/strict';
import test from 'node:test';
import { ref } from 'vue';
import { useHeadphoneHeader } from '../src/composables/useHeadphoneHeader.ts';
import type { AccountView } from '../src/services/contracts.ts';
import type { DeviceTuningComparison } from '../src/services/effectDirectory.ts';
import type { DeviceParametricView, PlayerView } from '../src/services/player.ts';

const owner: AccountView = { epoch: '1', status: 'authenticated', nickname: null, avatarUrl: null };
const snapshot = (
  status: DeviceParametricView['status'],
  receipt?: string,
  playId = 'play-1',
  epoch = '1',
) =>
  ({
    epoch,
    playId,
    deviceParametric: {
      brand: 2,
      model: 3,
      activationId: 'headphone',
      status,
      comparison: receipt ? { playId, receipt } : undefined,
    },
  }) as PlayerView;

test('the shared headphone switch submits one displayed receipt and waits for native readback', async () => {
  let acknowledge!: () => void;
  const sent: [string, DeviceTuningComparison][] = [];
  const state = ref(snapshot('configured', 'first'));
  const header = useHeadphoneHeader(
    {
      effectDeviceCompare: (epoch, request) => {
        sent.push([epoch, request]);
        return new Promise<void>((resolve) => {
          acknowledge = resolve;
        });
      },
    },
    () => owner,
    () => state.value,
    () => true,
  );
  const first = header.toggle('first');
  await header.toggle('first');
  assert.deepEqual(sent, [
    [
      '1',
      {
        brand: 2,
        model: 3,
        activationId: 'headphone',
        playId: 'play-1',
        receipt: 'first',
        paused: true,
      },
    ],
  ]);
  assert.equal(header.target.value?.device.status, 'configured');
  acknowledge();
  await first;
  assert.ok(header.pending.value, 'void IPC acknowledgement does not unlock another click');
  state.value = snapshot('paused_locally', 'next');
  assert.equal(header.pending.value, undefined);
  assert.ok(
    header.visible.value,
    'the same enabled observation may continue in the same modern dialog',
  );
  await header.toggle('first');
  assert.equal(sent.length, 1);
  const second = header.toggle('next');
  assert.equal(sent[1]?.[1].paused, false);
  acknowledge();
  await second;
  header.dispose();
});

test('modern off-mount, dry source, and stale account cannot retarget the shared switch', async () => {
  const account = ref(owner);
  const modern = ref(true);
  const state = ref(snapshot('paused_locally', 'paused'));
  let reject!: (reason: unknown) => void;
  let sends = 0;
  const header = useHeadphoneHeader(
    {
      effectDeviceCompare: () => {
        sends++;
        return new Promise<void>((_, fail) => {
          reject = fail;
        });
      },
    },
    () => account.value,
    () => state.value,
    () => modern.value,
  );
  assert.equal(header.visible.value, false);
  await header.toggle('paused');
  assert.equal(sends, 0);
  modern.value = false;
  assert.equal(header.visible.value, true, 'legacy keeps the selected disabled device');
  const started = header.toggle('paused');
  state.value = snapshot('bypassed_for_source', 'old-dry');
  assert.equal(header.visible.value, false);
  account.value = { ...owner, epoch: '2' };
  state.value = snapshot('configured', 'other', 'play-2', '2');
  reject({ code: 'stale', message: 'retired error' });
  await started;
  assert.equal(header.error.value, '');
  assert.equal(header.pending.value, undefined);
  assert.equal(header.target.value?.epoch, '2');
  await header.toggle('paused');
  assert.equal(sends, 1);
  const current = header.toggle('other');
  reject({ code: 'busy', message: '当前调音未确认' });
  await current;
  assert.equal(header.error.value, '当前调音未确认');
  assert.equal(header.target.value?.device.status, 'configured');
  header.dispose();
});
