import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useDesktopLyricSettings } from '../src/composables/useDesktopLyricSettings.ts';
import type { LyricSettingsClient, LyricSettingsView } from '../src/services/desktopLyrics.ts';
const view = (epoch = '1', revision = 'initial'): LyricSettingsView => ({
  epoch,
  visible: false,
  opening: false,
  locked: false,
  error: null,
  colors: null,
  preferences: {
    damaged: false,
    current: {
      revision,
      enabled: false,
      geometry: null,
      customPalette: null,
      appearance: {
        auxiliary: 'translate',
        alwaysOnTop: true,
        font: '',
        fontSize: 36,
        bold: false,
        outline: true,
        singleLine: true,
        composition: 'horizontal',
        singleAlignment: 'center',
        doubleAlignment: 'center',
        palette: { kind: 'preset', preset: 'red' },
      },
    },
  },
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { resolve, promise };
}
test('desktop lyric late reads and writes cannot replace a newer account or disposed view', async () => {
  const initial = deferred<LyricSettingsView>(),
    write = deferred<LyricSettingsView>();
  const client: LyricSettingsClient = {
    read: async (epoch) => (epoch === '1' ? initial.promise : view(epoch)),
    change: () => write.promise,
    window: async (epoch) => view(epoch),
  };
  const model = useDesktopLyricSettings(client);
  const old = model.connect('1');
  await model.connect('2');
  initial.resolve(view('1'));
  await old;
  assert.equal(model.view.value?.epoch, '2');
  const pending = model.change({ kind: 'enabled', value: true });
  await model.connect('3');
  write.resolve(view('2', 'written'));
  assert.equal(await pending, false);
  assert.equal(model.view.value?.epoch, '3');
  assert.equal(model.busy.value, false);
  model.dispose();
  await model.connect('4');
  assert.equal(model.view.value?.epoch, '3');
});
test('lost lyric preference acknowledgement only reads back and never replays window creation', async () => {
  let writes = 0,
    reads = 0;
  const model = useDesktopLyricSettings({
    read: async (epoch) => {
      reads++;
      const result = view(epoch, reads > 1 ? 'saved' : 'initial');
      result.preferences.current!.enabled = reads > 1;
      result.error = reads > 1 ? 'window failed' : null;
      return result;
    },
    change: async () => {
      writes++;
      throw { message: 'lost acknowledgement' };
    },
    window: async () => {
      throw Error('must not automatically retry');
    },
  });
  await model.connect('1');
  assert.equal(await model.change({ kind: 'enabled', value: true }), false);
  assert.equal(writes, 1);
  assert.equal(reads, 2);
  assert.equal(model.preferences.value?.enabled, true);
  assert.equal(model.saved.value, false);
  assert.ok(model.error.value);
  assert.equal(model.view.value?.error, 'window failed');
});
test('damaged desktop lyrics disable normal edits and explicit repair restores fresh revision', async () => {
  let repairs = 0;
  const model = useDesktopLyricSettings({
    read: async (epoch) => ({ ...view(epoch), preferences: { current: null, damaged: true } }),
    change: async (epoch, command) => {
      assert.equal(command.kind, 'repair');
      repairs++;
      return view(epoch, 'repaired');
    },
    window: async (epoch) => view(epoch),
  });
  await model.connect('1');
  assert.equal(model.canChange.value, false);
  assert.equal(await model.change({ kind: 'enabled', value: true }), false);
  assert.equal(await model.send({ kind: 'repair' }), true);
  assert.equal(repairs, 1);
  assert.equal(model.preferences.value?.revision, 'repaired');
  assert.equal(model.canChange.value, true);
});
test('passive lyric status cannot overwrite a later save or erase its failure message', async () => {
  const pending = deferred<LyricSettingsView>();
  let reads = 0;
  const model = useDesktopLyricSettings({
    read: async (epoch) =>
      ++reads === 2 ? pending.promise : view(epoch, reads > 2 ? 'saved' : 'initial'),
    change: async () => {
      throw { message: 'write acknowledgement lost' };
    },
    window: async (epoch) => view(epoch),
  });
  await model.connect('1');
  const passive = model.status();
  await model.change({ kind: 'enabled', value: true });
  const failure = model.error.value;
  assert.ok(failure);
  pending.resolve(view('1', 'outdated'));
  await passive;
  assert.equal(model.preferences.value?.revision, 'saved');
  await model.status();
  assert.equal(model.error.value, failure);
});
