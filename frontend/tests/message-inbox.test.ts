import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createRenderer, defineComponent, h, nextTick, ref } from 'vue';
import { mergePrivateInbox } from '../src/composables/messageInbox.ts';
import { useMessageDrawer } from '../src/composables/useMessageDrawer.ts';
import type {
  MessageClient,
  MessageCountView,
  MessagePage,
  MessageQuery,
  MessageRow,
} from '../src/services/messages.ts';

const row = (id: string, time: number, unread: number, receipt?: string): MessageRow => ({
  id,
  peerId: id,
  author: id,
  body: `预览 ${id} ${time}`,
  caption: null,
  quote: null,
  time,
  unread,
  own: false,
  target: null,
  deletionReceipt: receipt,
});
const page = (rows: MessageRow[], nextCursor: number | null = null): MessagePage => ({
  rows,
  nextCursor,
  hint: null,
});
const settle = async () => {
  for (let i = 0; i < 4; i++) await nextTick();
};

test('inbox first-page merge keeps same-time read state and only fresh native action receipts', () => {
  const read = row('44', 200, 0, 'old');
  const pending = { ...row('45', 100, 7, 'pending'), deletionPending: true };
  const before = [read, pending];
  const same = mergePrivateInbox(before, [row('46', 300, 9), row('44', 200, 6)], 200);
  assert.deepEqual(
    same.rows.map(({ peerId, unread, deletionReceipt }) => [peerId, unread, deletionReceipt]),
    [
      ['46', 9, undefined],
      ['44', 0, undefined],
      ['45', 7, 'pending'],
    ],
  );
  assert.equal(same.rows[2], pending);
  assert.equal(same.cursor, 100, 'older pages continue after the previously loaded tail');
  const fresh = row('44', 201, 6, 'new');
  const changed = mergePrivateInbox(before, [fresh], null);
  assert.equal(changed.rows[0], fresh);
  assert.equal(changed.cursor, null);
});

function mountInbox(first: MessagePage) {
  const account = ref({
    epoch: '1',
    status: 'authenticated' as const,
    nickname: null,
    avatarUrl: null,
  });
  const requests: MessageQuery[] = [];
  let update!: (view: MessageCountView) => void;
  let model!: ReturnType<typeof useMessageDrawer>;
  let next = Promise.resolve(first);
  const core = {
    async watchMessageCounts(notify: typeof update) {
      update = notify;
      return { visibility: async () => {}, navigate: async () => {}, close() {} };
    },
    async messagePage(epoch: string, query: MessageQuery) {
      requests.push(query);
      const value = await next;
      return { epoch, value };
    },
  } as unknown as MessageClient;
  const renderer = createRenderer({
    createElement: () => ({}),
    createText: () => ({}),
    createComment: () => ({}),
    patchProp() {},
    insert() {},
    remove() {},
    setText() {},
    setElementText() {},
    parentNode: () => null,
    nextSibling: () => null,
  });
  const app = renderer.createApp(
    defineComponent({
      setup() {
        model = useMessageDrawer(
          core,
          () => account.value,
          () => assert.fail('unexpected expiry'),
        );
        return () => h('test-inbox');
      },
    }),
  );
  app.mount({});
  function count(value: number, visible = true, epoch = '1') {
    update({
      epoch,
      counts: { private: value, comments: 0, forwards: 0, notices: 0 },
      hidePrivateRowBadges: value === 0,
      drawerVisible: visible,
      refreshing: false,
      failed: false,
    });
  }
  return {
    model,
    account,
    requests,
    count,
    close: () => app.unmount(),
    answer: (value: MessagePage | Promise<MessagePage>) => (next = Promise.resolve(value)),
  };
}

test('private count change merges a visible inbox and waits while the native window is hidden', async () => {
  const f = mountInbox(page([row('44', 200, 3), row('45', 100, 4)]));
  try {
    f.count(7, false);
    f.model.open();
    f.model.setCountVisibility(true);
    f.count(7);
    await settle();
    assert.equal(f.requests.length, 1, 'opening does not duplicate the explicit first page');
    const visible = f.model.current.value;
    f.answer(page([]));
    await f.model.enter(visible.rows[0]);
    assert.equal(visible.rows[0].unread, 0);
    f.count(0);
    await settle();
    assert.equal(f.requests.length, 2, 'only the chosen history is read while in conversation');

    f.answer(page([row('44', 200, 6), row('45', 100, 4)]));
    f.model.back();
    await settle();
    assert.deepEqual(f.requests[2], { tab: 'private', cursor: null });
    assert.equal(f.model.hidePrivateRowBadges.value, true);
    assert.deepEqual(
      visible.rows.map((r) => r.unread),
      [0, 4],
    );

    f.count(10, false);
    await settle();
    assert.equal(f.requests.length, 3, 'a hidden native window cannot read the private inbox');
    f.answer(page([row('44', 201, 6)]));
    f.count(10);
    await settle();
    assert.equal(f.requests.length, 4);
    assert.equal(f.model.hidePrivateRowBadges.value, false);
    assert.deepEqual(
      visible.rows.map((r) => r.unread),
      [6, 4],
    );

    f.account.value.epoch = '2';
    f.count(0, true, '1');
    await settle();
    assert.equal(f.model.shown.value, false);
    assert.equal(f.model.hidePrivateRowBadges.value, false);
    assert.equal(f.requests.length, 4);
  } finally {
    f.close();
  }
});

test('a first page issued before a conversation deletion cannot restore its row or receipt', async () => {
  const removed = row('44', 200, 3, 'old-native');
  const kept = row('45', 100, 4);
  const f = mountInbox(page([removed, kept]));
  try {
    f.count(7, false);
    f.model.open();
    f.model.setCountVisibility(true);
    f.count(7);
    await settle();
    let release!: (value: MessagePage) => void;
    f.answer(new Promise<MessagePage>((resolve) => (release = resolve)));
    f.count(8);
    await settle();
    assert.equal(f.requests.length, 2);
    const current = f.model.current.value;
    const end = f.model.beginInboxDeletion(current);
    current.rows = current.rows.filter((item) => item.peerId !== '44');
    end();
    release(page([removed, kept]));
    await settle();
    assert.equal(current.busy, false);
    assert.deepEqual(
      current.rows.map((r) => r.peerId),
      ['45'],
    );
    f.answer(page([row('44', 201, 6, 'fresh-native')]));
    f.count(9);
    await settle();
    assert.deepEqual(
      current.rows.map((r) => r.deletionReceipt),
      ['fresh-native', undefined],
    );
  } finally {
    f.close();
  }
});
