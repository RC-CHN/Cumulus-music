import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useLocalBatch } from '../src/composables/useLocalBatch.ts';
import {
  defaultLocalQuery,
  type LocalCommand,
  type LocalPage,
  type LocalQuery,
  type LocalTrack,
} from '../src/services/local.ts';

function track(index: number): LocalTrack {
  return {
    track: {
      id: `local-${index}`,
      name: `合成 ${index}`,
      artists: [],
      album: '',
      coverUrl: null,
      durationMs: 1000,
      fee: 0,
      missing: false,
      local: true,
    },
    revision: `row-${index}`,
    fileName: `synthetic-${index}.flac`,
    folder: '/synthetic',
    format: 'flac',
    cue: false,
    encrypted: false,
    unreadable: false,
  };
}
function page(offset: number, revision: string, total = 101): LocalPage {
  return {
    revision,
    tracks: Array.from({ length: Math.max(0, Math.min(100, total - offset)) }, (_, n) =>
      track(offset + n),
    ),
    groups: [],
    roots: [],
    total,
    playable: total,
    offset,
    more: offset + 100 < total,
  };
}
const stale = { code: 'snapshot_changed', message: '已更新' };
const io = { code: 'io', message: '合成读写故障' };

test('flat batch pins pages, preserves exact cross-page refs on failure and refreshes after one explicit retry', async () => {
  const reads: LocalQuery[] = [],
    commands: LocalCommand[] = [];
  const model = useLocalBatch(
    {
      ...defaultLocalQuery(),
      keyword: '合成',
      group: 'folder',
      groupKey: 'fixture',
      sort: 'artist',
      descending: false,
      includeMissing: false,
      offset: 200,
      revision: 'old-list',
    },
    {
      localPage: async (query) => {
        reads.push(query);
        return commands.length === 2
          ? page(query.offset, 'after', 99)
          : page(query.offset, 'before');
      },
      localCommand: async (command) => {
        commands.push(command);
        if (commands.length === 1) throw io;
      },
    },
  );
  await model.load();
  await model.move(1);
  model.toggle('local-100');
  await model.move(-1);
  model.toggle('local-3');
  await model.move(1);
  assert.equal(model.selectionCount.value, 2);
  assert.equal(model.pageSelection.value.all, true);
  await model.remove();
  assert.equal(model.selectionCount.value, 2);
  assert.match(model.commandError.value, /合成读写故障/);
  assert.equal(reads.length, 4, 'a same-revision command failure does not replay or reload');
  await model.remove();
  assert.deepEqual(
    commands,
    Array.from({ length: 2 }, () => ({
      kind: 'hide_tracks',
      query: {
        keyword: '合成',
        sort: 'artist',
        descending: false,
        group: 'list',
        groupKey: null,
        includeMissing: false,
        offset: 0,
        limit: 100,
        revision: null,
      },
      revision: 'before',
      tracks: [
        { id: 'local-3', revision: 'row-3' },
        { id: 'local-100', revision: 'row-100' },
      ],
    })),
  );
  assert.deepEqual(
    reads.map(({ offset, revision }) => [offset, revision]),
    [
      [0, null],
      [100, 'before'],
      [0, 'before'],
      [100, 'before'],
      [100, null],
      [0, 'after'],
    ],
  );
  assert.equal(model.selectionCount.value, 0);
  assert.match(model.notice.value, /源文件仍保留/);
});

test('a stale page or command retires all selections without replaying the mutation', async () => {
  const commands: LocalCommand[] = [];
  let revision = 'first';
  const model = useLocalBatch(defaultLocalQuery(), {
    localPage: async (query) => {
      if (query.revision && query.revision !== revision) throw stale;
      return page(query.offset, revision);
    },
    localCommand: async (command) => {
      commands.push(command);
      revision = 'third';
      throw stale;
    },
  });
  await model.load();
  model.toggle();
  assert.equal(model.selectionCount.value, 100);
  revision = 'second';
  await model.move(1);
  assert.equal(model.selectionCount.value, 0);
  assert.equal(model.page.value?.offset, 0);
  assert.equal(model.page.value?.revision, 'second');
  assert.match(model.notice.value, /已清空选择/);
  model.toggle('local-1');
  await model.remove();
  assert.equal(commands.length, 1);
  assert.equal(model.selectionCount.value, 0);
  assert.equal(model.page.value?.revision, 'third');
  await model.remove();
  assert.equal(commands.length, 1, 'fresh rows require a new selection');
});

test('a failed read is retried at its exact offset and a departed visit cannot apply a held reply', async () => {
  let live = true;
  let reads = 0;
  let writes = 0;
  let finish!: (page: LocalPage) => void;
  const held = new Promise<LocalPage>((resolve) => {
    finish = resolve;
  });
  const model = useLocalBatch(
    defaultLocalQuery(),
    {
      localPage: async (query) => {
        reads++;
        if (reads === 1) return page(0, 'fixed');
        assert.equal(query.offset, 100);
        assert.equal(query.revision, 'fixed');
        if (reads === 2) throw io;
        return held;
      },
      localCommand: async () => {
        writes++;
      },
    },
    () => live,
  );
  await model.load();
  model.toggle('local-2');
  await model.move(1);
  assert.match(model.readError.value, /合成读写故障/);
  assert.equal(model.selectionCount.value, 1);
  await model.remove();
  assert.equal(writes, 0, 'read error cannot authorize the retained selection');
  const retry = model.retry();
  live = false;
  finish(page(100, 'fixed'));
  await retry;
  assert.equal(model.page.value?.offset, 0);
  await model.remove();
  assert.equal(writes, 0, 'the leaving view cannot submit');
});

test('play sends the canonical cross-page refs without changing selection; stale play reloads rather than replaying', async () => {
  const requests: import('../src/services/local.ts').LocalBatchPlayback[] = [];
  let revision = 'before';
  let reject = false;
  const model = useLocalBatch(
    defaultLocalQuery(),
    {
      localPage: async (query) => page(query.offset, revision),
      localCommand: async () => assert.fail('play must not mutate the library index'),
    },
    undefined,
    async (selection) => {
      requests.push(selection);
      if (reject) throw stale;
    },
  );
  await model.load(100);
  model.toggle('local-100');
  await model.move(-1);
  model.toggle('local-4');
  await model.play();
  assert.deepEqual(requests[0], {
    query: model.query,
    revision: 'before',
    tracks: [
      { id: 'local-4', revision: 'row-4' },
      { id: 'local-100', revision: 'row-100' },
    ],
  });
  assert.equal(model.selectionCount.value, 2);
  assert.equal(model.page.value?.offset, 0);
  reject = true;
  revision = 'after';
  await model.play();
  assert.equal(requests.length, 2);
  assert.equal(model.selectionCount.value, 0);
  assert.equal(model.page.value?.revision, 'after');
  assert.match(model.notice.value, /列表已更新/);
});
