import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, nextTick, ref } from 'vue';
import { useSearchSongShare } from '../src/composables/useSearchSongShare.ts';
import type { AccountData, CoreClient } from '../src/services/contracts.ts';
import type {
  HistoryRowShareOutcome,
  HistoryRowSharePreparation,
} from '../src/services/history.ts';
import type { SearchSongShareClient } from '../src/services/search.ts';

const target = { receipt: 'row-receipt', trackId: '17' };
const offer = 'channel-receipt';
const link = 'https://music.163.com/song?id=17';
const anchor = { isConnected: false } as HTMLElement;
const reply = <T>(value: T): AccountData<T> => ({ epoch: '1', value });
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
type Client = SearchSongShareClient & Pick<CoreClient, 'copyText'>;
function fixture(overrides: Partial<Client> = {}) {
  const active = ref(true);
  const calls: unknown[] = [];
  const client: Client = {
    searchSongSharePrepare: async (...args) => {
      calls.push(['prepare', ...args]);
      return reply({ kind: 'ready', receipt: offer });
    },
    searchSongShareChoose: async (...args) => {
      calls.push(['choose', ...args]);
      return reply({ kind: 'copy', url: link });
    },
    searchSongShareCancel: async (...args) => {
      calls.push(['cancel', ...args]);
    },
    copyText: async (text) => {
      calls.push(['clipboard', text]);
    },
    ...overrides,
  };
  const scope = effectScope();
  const model = scope.run(() => useSearchSongShare(client, () => calls.push(['expired'])))!;
  const visit = { epoch: '1', target, live: () => active.value, fallbackFocus: () => null };
  return { model, active, visit, calls, stop: () => scope.stop() };
}

test('closing a current chooser retires its exact native offer without requesting a channel', async (t) => {
  const { model, visit, calls, stop } = fixture();
  t.after(stop);
  await model.open(visit, anchor);
  assert.equal(model.dialog.value?.stage, 'chooser');
  model.close();
  await model.choose('copy');
  assert.equal(model.dialog.value, undefined);
  assert.equal(model.restoreFocus.value, true);
  assert.equal(model.returnFocus(), anchor);
  assert.deepEqual(calls, [
    ['prepare', '1', target],
    ['cancel', '1', target, offer],
  ]);
});

test('a late offer for a departed search visit is retired and never displayed', async (t) => {
  const prepare = deferred<AccountData<HistoryRowSharePreparation>>();
  const { model, active, visit, calls, stop } = fixture({
    searchSongSharePrepare: () => prepare.promise,
  });
  t.after(stop);
  const opening = model.open(visit, anchor);
  active.value = false;
  prepare.resolve(reply({ kind: 'ready', receipt: offer }));
  await opening;
  assert.equal(model.dialog.value, undefined);
  assert.equal(model.restoreFocus.value, false);
  assert.deepEqual(calls, [['cancel', '1', target, offer]]);
});

test('a late channel reply after leaving never writes the clipboard or presents feedback', async (t) => {
  const channel = deferred<AccountData<HistoryRowShareOutcome>>();
  let choosing = false;
  const { model, active, visit, calls, stop } = fixture({
    searchSongShareChoose: () => {
      choosing = true;
      return channel.promise;
    },
  });
  t.after(stop);
  await model.open(visit, anchor);
  const chosen = model.choose('copy');
  assert.equal(model.dialog.value, undefined);
  await nextTick();
  assert.equal(choosing, true);
  active.value = false;
  channel.resolve(reply({ kind: 'copy', url: link }));
  await chosen;
  assert.equal(model.feedback.value, undefined);
  assert.equal(model.returnFocus(), null);
  assert.deepEqual(calls, [
    ['prepare', '1', target],
    ['cancel', '1', target, offer],
  ]);
});

test('two clicks on a chooser can submit only one channel and copy its returned public URL', async (t) => {
  const { model, visit, calls, stop } = fixture();
  t.after(stop);
  await model.open(visit, anchor);
  await Promise.all([model.choose('copy'), model.choose('copy')]);
  assert.deepEqual(calls, [
    ['prepare', '1', target],
    ['choose', '1', target, offer, 'copy'],
    ['clipboard', link],
  ]);
  assert.deepEqual(model.feedback.value, { text: '复制成功', error: false });
});

test('independent guest Copy writes only the public link and creates no share offer', async (t) => {
  const { model, visit, calls, stop } = fixture();
  t.after(stop);
  await model.copyPublic(link, visit.live);
  assert.deepEqual(calls, [['clipboard', link]]);
  assert.equal(model.dialog.value, undefined);
  assert.deepEqual(model.feedback.value, { text: '链接已复制', error: false });
});

test('a guest Copy acknowledgement is discarded after its exact visit leaves', async (t) => {
  const clipboard = deferred<void>();
  const { model, active, visit, calls, stop } = fixture({ copyText: () => clipboard.promise });
  t.after(stop);
  const copy = model.copyPublic(link, visit.live);
  active.value = false;
  clipboard.resolve(undefined);
  await copy;
  assert.equal(model.feedback.value, undefined);
  assert.equal(model.dialog.value, undefined);
  assert.deepEqual(calls, []);
});
