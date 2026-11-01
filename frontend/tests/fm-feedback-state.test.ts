import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, nextTick, shallowRef } from 'vue';
import { useFm } from '../src/composables/useFm.ts';
import { useFmFeedback } from '../src/composables/useFmFeedback.ts';
import type { FmClient, FmView, FmReasons, FmCommand } from '../src/services/fm.ts';
import type { AccountData, TrackView } from '../src/services/contracts.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
function fixture(overrides: Partial<FmClient> = {}) {
  const track: TrackView = {
    id: '1',
    name: 'A long FM song',
    artists: ['Guest'],
    artistRefs: [],
    album: 'Album',
    albumId: null,
    coverUrl: null,
    durationMs: 30000,
    fee: 0,
    missing: false,
    local: false,
  };
  let actual: FmView = {
    source: { kind: 'personal' },
    activeSource: { kind: 'personal' },
    revision: '1',
    items: [{ entryId: 'entry-1', track }],
    current: 'entry-1',
    active: true,
    pending: false,
    modePending: false,
    notice: null,
    operations: [],
    blockedArtists: [],
  };
  const commands: FmCommand[] = [];
  const port: FmClient = {
    fmDiscovery: async () => {
      throw Error('unused');
    },
    fmReasons: async (epoch, _, entryId) => ({
      epoch,
      value: {
        entryId,
        songId: '1',
        options: [{ key: 'artist', label: '屏蔽歌手：Guest', artistId: '2' }],
      },
    }),
    fmView: async (epoch) => ({ epoch, value: actual }),
    fmCommand: async (epoch, _, command) => {
      commands.push(command);
      return { epoch, value: actual };
    },
    fmTrash: async (epoch) => ({
      epoch,
      value: { page: 1, reportedTotal: 0, songs: [], more: false, cursor: '' },
    }),
    ...overrides,
  };
  const model = useFm(port),
    playback = shallowRef({ epoch: '1', entryId: 'entry-1', fm: true });
  model.connect({ epoch: '1', status: 'authenticated', nickname: 'Test', avatarUrl: null });
  model.view.value = actual;
  const scope = effectScope(),
    dialog = scope.run(() => useFmFeedback(model, () => playback.value))!;
  return {
    model,
    playback,
    dialog,
    port,
    commands,
    setView(value: FmView) {
      actual = value;
      model.view.value = value;
    },
    close() {
      scope.stop();
      model.dispose();
    },
  };
}
test('a rejected FM submission stays visible and only an explicit retry can submit it again', async (t) => {
  let writes = 0;
  const f = fixture({
    fmCommand: async () => {
      writes++;
      throw { code: 'service_rejected', message: 'Synthetic refusal', retryable: false };
    },
  });
  t.after(f.close);
  f.dialog.show();
  await nextTick();
  assert.equal(await f.dialog.submit('artist'), false);
  assert.equal(f.dialog.open.value, true);
  assert.equal(f.dialog.issue.value?.message, 'Synthetic refusal');
  assert.equal(f.dialog.target.value?.entryId, 'entry-1');
  await f.model.load();
  assert.equal(writes, 1);
  assert.equal(f.dialog.issue.value?.message, 'Synthetic refusal');
  assert.equal(await f.dialog.submit('artist'), false);
  assert.equal(writes, 2);
});
test('current media changes invalidate the frozen feedback without retargeting its command', async (t) => {
  const f = fixture();
  t.after(f.close);
  f.dialog.show();
  await nextTick();
  f.playback.value = { epoch: '1', entryId: 'entry-2', fm: true };
  assert.equal(f.dialog.changed.value, true);
  assert.equal(await f.dialog.submit(), false);
  assert.equal(f.commands.length, 0);
  assert.equal(f.dialog.target.value?.entryId, 'entry-1');
});
test('unknown remote feedback exposes reconciliation and cannot resend the same dialog', async (t) => {
  const f = fixture();
  t.after(f.close);
  let writes = 0;
  f.port.fmCommand = async () => {
    writes++;
    f.setView({
      ...f.model.view.value!,
      operations: [
        {
          id: 'op',
          kind: 'trash',
          phase: 'uncertain',
          desired: true,
          canApply: false,
          artistId: null,
        },
      ],
    });
    throw { code: 'network', message: 'Lost response', retryable: true };
  };
  f.dialog.show();
  await nextTick();
  await f.dialog.submit('artist');
  assert.equal(f.dialog.open.value, true);
  assert.equal(f.dialog.review.value, true);
  assert.equal(f.dialog.canSubmit.value, false);
  await f.dialog.submit('artist');
  assert.equal(writes, 1);
});
test('a partial artist failure preserves its explanation and cannot trash the new current song', async (t) => {
  const f = fixture();
  t.after(f.close);
  f.port.fmCommand = async () => {
    f.playback.value = { epoch: '1', entryId: 'entry-2', fm: true };
    f.setView({
      ...f.model.view.value!,
      current: 'entry-2',
      operations: [
        {
          id: 'op',
          kind: 'artist',
          phase: 'rejected',
          desired: true,
          canApply: false,
          artistId: '2',
        },
      ],
    });
    throw { code: 'artist_rejected', message: 'Song reduced; artist failed', retryable: true };
  };
  f.dialog.show();
  await nextTick();
  await f.dialog.submit('artist');
  assert.equal(f.dialog.open.value, true);
  assert.equal(f.dialog.changed.value, true);
  assert.equal(f.dialog.issue.value?.code, 'artist_rejected');
  assert.equal(f.dialog.review.value, true);
  assert.equal(f.dialog.canSubmit.value, false);
});
test('an old account acknowledgement cannot dismiss a new feedback dialog', async (t) => {
  const late = deferred<AccountData<FmView>>();
  const f = fixture({ fmCommand: () => late.promise });
  t.after(f.close);
  f.dialog.show();
  await nextTick();
  const old = f.dialog.submit();
  const value = f.model.view.value!;
  f.model.connect({ epoch: '2', status: 'authenticated', nickname: 'New', avatarUrl: null });
  f.playback.value = { epoch: '2', entryId: 'entry-1', fm: true };
  f.setView(value);
  await nextTick();
  f.dialog.show();
  await nextTick();
  const target = f.dialog.target.value;
  late.resolve({ epoch: '1', value });
  assert.equal(await old, false);
  assert.equal(f.dialog.open.value, true);
  assert.equal(f.dialog.target.value, target);
  assert.equal(f.dialog.pending.value, false);
});
test('closing while reasons load rejects the late list; a failed list survives polling and retries explicitly', async (t) => {
  const late = deferred<AccountData<FmReasons>>();
  let reads = 0;
  const f = fixture({
    fmReasons: async () => {
      reads++;
      if (reads === 1) return late.promise;
      throw { code: 'network', message: 'Options failed', retryable: true };
    },
  });
  t.after(f.close);
  f.dialog.show();
  f.dialog.close();
  late.resolve({ epoch: '1', value: { entryId: 'entry-1', songId: '1', options: [] } });
  await nextTick();
  assert.equal(f.model.reasons.value, undefined);
  f.dialog.show();
  await nextTick();
  await nextTick();
  assert.equal(f.model.reasonsIssue.value?.message, 'Options failed');
  await f.model.load();
  assert.equal(f.model.reasonsIssue.value?.message, 'Options failed');
  assert.equal(reads, 2);
  f.dialog.retryReasons();
  await nextTick();
  assert.equal(reads, 3);
});
test('only a confirmed current-context result dismisses the feedback dialog', async (t) => {
  const f = fixture();
  t.after(f.close);
  f.dialog.show();
  await nextTick();
  assert.equal(await f.dialog.submit(), true);
  assert.equal(f.dialog.open.value, false);
  assert.deepEqual(f.commands, [{ kind: 'trash', entry_id: 'entry-1' }]);
});
