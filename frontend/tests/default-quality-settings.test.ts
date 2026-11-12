import assert from 'node:assert/strict';
import test from 'node:test';
import { useDefaultQualitySettings } from '../src/composables/useDefaultQualitySettings.ts';
import {
  displayedSettingsQuality,
  type DefaultQualityView,
} from '../src/services/settingsQuality.ts';

function view(
  epoch = '1',
  quality: DefaultQualityView['quality'] = 'standard',
): DefaultQualityView {
  return {
    epoch,
    revision: `revision-${epoch}-${quality}`,
    quality,
    choices: [
      { quality: 'jymaster', access: 'requires_svip' },
      { quality: 'exhigh', access: 'allowed' },
      { quality: 'standard', access: 'allowed' },
    ],
  };
}

test('default quality acts once and reconciles a lost native response by a passive read', async () => {
  const visit = { epoch: '1', page: {} };
  const writes: unknown[] = [],
    reads: unknown[] = [];
  let current = view();
  const model = useDefaultQualitySettings({
    visit: () => visit,
    async read(epoch, refresh) {
      reads.push([epoch, refresh]);
      return current;
    },
    async save(epoch, revision, quality) {
      writes.push([epoch, revision, quality]);
      current = view(epoch, quality);
      throw { code: 'connection', message: 'lost acknowledgement' };
    },
    expired() {
      assert.fail('same account');
    },
  });
  await model.refresh();
  await model.choose('jymaster');
  await model.choose('standard');
  await Promise.all([model.choose('exhigh'), model.choose('exhigh')]);
  assert.deepEqual(writes, [['1', 'revision-1-standard', 'exhigh']]);
  assert.deepEqual(reads, [
    ['1', false],
    ['1', false],
  ]);
  assert.equal(model.value.value?.quality, 'exhigh');
  assert.match(model.feedback.value, /重新读取/);
});

test('an old visit cannot publish a successful write or read back as the new owner', async () => {
  let owner = { epoch: '1', page: {} };
  const held = Promise.withResolvers<DefaultQualityView>();
  const reads: string[] = [];
  const model = useDefaultQualitySettings({
    visit: () => owner,
    async read(epoch) {
      reads.push(epoch);
      return view(epoch);
    },
    save: () => held.promise,
    expired() {
      assert.fail('the retired visit is ignored');
    },
  });
  await model.refresh();
  const pending = model.choose('exhigh');
  owner = { epoch: '2', page: {} };
  model.retire();
  await model.refresh();
  held.resolve(view('1', 'exhigh'));
  await pending;
  assert.deepEqual(reads, ['1', '2']);
  assert.deepEqual(model.value.value, view('2'));
  assert.equal(model.feedback.value, '');
});

test('legacy visual alias is not itself a native preference write and blocked snapshots remain read-only', async () => {
  const owner = { epoch: '1', page: {} };
  const model = useDefaultQualitySettings({
    visit: () => owner,
    async read() {
      return { ...view('1', 'higher'), revision: null };
    },
    save() {
      assert.fail('the blocked native revision must never be sent');
    },
    expired() {
      assert.fail('same account');
    },
  });
  await model.refresh();
  assert.equal(displayedSettingsQuality(model.value.value?.quality), 'standard');
  await model.choose('exhigh');
  assert.equal(model.value.value?.quality, 'higher');
});
