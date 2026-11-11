import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSearchResults } from '../src/composables/useSearchResults.ts';
import { createSearchViewportMemory } from '../src/services/searchViewport.ts';
import {
  comprehensivePlayAllTracks,
  displayedSearchCategories,
  searchExplanation,
  type SearchResultQuery,
  type SearchResultView,
  type SearchResultsClient,
} from '../src/services/search.ts';
import type { AccountData, AccountView } from '../src/services/contracts.ts';
const account: AccountView = {
  epoch: '1',
  status: 'authenticated',
  nickname: '合成账号',
  avatarUrl: null,
};
const query: SearchResultQuery = {
  keyword: '合成',
  category: 'all',
  offset: 0,
  limit: 30,
  correct: true,
};
const view = (q: SearchResultQuery): AccountData<SearchResultView> => ({
  epoch: '1',
  value: {
    query: q,
    sections: [],
    total: 0,
    more: false,
    nextOffset: q.offset,
    correction: null,
    unavailableCount: 0,
    notice: null,
  },
});
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
test('category and page changes reject late responses and disposal prevents resurrection', async () => {
  const old = deferred<AccountData<SearchResultView>>();
  let calls = 0;
  const api: SearchResultsClient = {
    searchResults: async (_e, q) => (++calls === 1 ? old.promise : view(q)),
  };
  const model = useSearchResults(api, () => assert.fail('no expiry'));
  model.connect(account);
  const first = model.open(query);
  const second = { ...query, category: 'artists' as const, offset: 12 };
  await model.open(second);
  old.resolve(view(query));
  await first;
  assert.deepEqual(model.view.value?.query, second);
  model.dispose();
  assert.equal(model.view.value, undefined);
  await model.open(query);
  assert.equal(calls, 2);
});
test('stale failures and mismatched account results cannot expire the new account', async () => {
  const old = deferred<AccountData<SearchResultView>>();
  let expired = 0,
    calls = 0;
  const model = useSearchResults(
    { searchResults: async () => (++calls === 1 ? old.promise : view(query)) },
    () => expired++,
  );
  model.connect(account);
  const first = model.open(query);
  model.connect({ ...account, epoch: '2' });
  await model.open(query);
  old.reject({ code: 'session_expired', message: '旧账号失效' });
  await first;
  assert.equal(model.view.value, undefined);
  assert.equal(model.error.value, '');
  assert.equal(expired, 0);
});
test('failure has a retry path and current expiry is forwarded', async () => {
  let mode = 'error',
    expired = 0;
  const model = useSearchResults(
    {
      searchResults: async (_e, q) => {
        if (mode !== 'good')
          throw { code: mode === 'expired' ? 'session_expired' : 'transport', message: '请重试' };
        return view(q);
      },
    },
    () => expired++,
  );
  model.connect(account);
  await model.open(query);
  assert.equal(model.error.value, '请重试');
  assert.equal(model.loading.value, false);
  mode = 'good';
  await model.open(query);
  assert.equal(model.error.value, '');
  assert.equal(model.view.value?.total, 0);
  mode = 'expired';
  await model.open(query);
  assert.equal(expired, 1);
  assert.equal(model.view.value, undefined);
});

const artistQuery = { ...query, category: 'artists' as const, limit: 2 };
function artistPage(q: SearchResultQuery, ids: string[], more = true) {
  const result = view(q);
  result.value.sections = [
    {
      id: 'artists',
      title: '歌手',
      category: 'artists',
      items: ids.map((id) => ({
        kind: 'artist' as const,
        artist: { id, name: id, coverUrl: null, aliases: [], songCount: 3, albumCount: 1 },
      })),
    },
  ];
  result.value.more = more;
  result.value.nextOffset = q.offset + q.limit;
  return result;
}
test('continuous loading preserves the first records and retries the same cursor once', async () => {
  const calls: number[] = [];
  let fail = true;
  const model = useSearchResults(
    {
      searchResults: async (_e, q) => {
        calls.push(q.offset);
        if (q.offset && fail) throw { code: 'transport', message: '请重试' };
        return artistPage(q, q.offset ? ['2', '3'] : ['1', '2'], !q.offset);
      },
    },
    () => assert.fail('expiry'),
  );
  model.connect(account);
  await model.open(artistQuery);
  model.savePosition({ top: 250, tracks: 0 });
  await model.more();
  assert.equal(model.moreError.value, '请重试');
  assert.equal(model.view.value?.sections[0]?.items.length, 2);
  assert.equal(model.position.value.top, 250);
  fail = false;
  await model.more();
  assert.deepEqual(calls, [0, 2, 2]);
  assert.equal(model.view.value?.sections[0]?.items.length, 3);
  assert.equal(model.view.value?.more, false);
  assert.equal(model.moreError.value, '');
});
test('continuous loading uses the latest confirmed rewrite and keeps it on later pages', async () => {
  const calls: string[] = [];
  const model = useSearchResults(
    {
      searchResults: async (_epoch, q) => {
        calls.push(q.keyword);
        const page = artistPage(q, [String(q.offset)]);
        page.value.correction = q.offset === 0 ? '初次纠正' : q.offset === 2 ? '进一步纠正' : null;
        return page;
      },
    },
    () => assert.fail('expiry'),
  );
  model.connect(account);
  await model.open(artistQuery);
  await model.more();
  await model.more();
  assert.deepEqual(calls, ['合成', '初次纠正', '进一步纠正']);
  assert.equal(model.view.value?.correction, '进一步纠正');
  assert.equal(model.view.value?.sections[0]?.items.length, 3);
});
test('in-flight append is coalesced and its old-account error cannot expire a new search', async () => {
  const pending = deferred<AccountData<SearchResultView>>();
  let appends = 0;
  const model = useSearchResults(
    {
      searchResults: async (_e, q) =>
        q.offset ? (appends++, pending.promise) : artistPage(q, ['1', '2']),
    },
    () => assert.fail('late expiry'),
  );
  model.connect(account);
  await model.open(artistQuery);
  const first = model.more();
  await model.more();
  assert.equal(appends, 1);
  model.connect({ ...account, epoch: '2' });
  pending.reject({ code: 'session_expired', message: '旧账号' });
  await first;
  assert.equal(model.moreError.value, '');
  assert.equal(model.view.value, undefined);
});
test('filtered search pages retain the native cursor and permit reaching later visible results', async () => {
  const requests: number[] = [];
  const model = useSearchResults(
    {
      searchResults: async (_epoch, q) => {
        requests.push(q.offset);
        if (q.offset === 4) return artistPage(q, ['5'], false);
        const filtered = artistPage(q, []);
        filtered.value.unavailableCount = 2;
        return filtered;
      },
    },
    () => assert.fail('no expiry'),
  );
  model.connect(account);
  await model.open(artistQuery);
  assert.equal(model.view.value?.more, true);
  await model.more();
  assert.equal(model.view.value?.more, true);
  assert.equal(model.view.value?.nextOffset, 4);
  assert.match(model.view.value?.notice ?? '', /可以继续加载/);
  await model.more();
  assert.deepEqual(requests, [0, 2, 4]);
  assert.equal(model.view.value?.unavailableCount, 4);
  assert.equal(model.view.value?.sections[0]?.items[0]?.kind, 'artist');
  assert.equal(model.view.value?.more, false);
});
test('duplicate cards pause automatic loading but retain manual continuation across back navigation', async () => {
  const requests: number[] = [];
  const model = useSearchResults(
    {
      searchResults: async (_e, q) => {
        requests.push(q.offset);
        return q.offset === 4 ? artistPage(q, ['3'], false) : artistPage(q, ['1', '2']);
      },
    },
    () => {},
  );
  model.connect(account);
  await model.open(artistQuery, true, 1);
  await model.more();
  assert.equal(model.view.value?.sections[0]?.items.length, 2);
  assert.equal(model.view.value?.more, true);
  assert.equal(model.manualMore.value, true);
  assert.match(model.view.value!.notice!, /可以继续加载/);
  await model.open(query, true, 1);
  assert.equal(model.manualMore.value, false);
  await model.open(artistQuery, true, 1);
  assert.equal(model.manualMore.value, true);
  await model.more();
  assert.deepEqual(requests, [0, 2, 0, 4]);
  assert.equal(model.view.value?.sections[0]?.items.length, 3);
  assert.equal(model.view.value?.more, false);
  assert.equal(model.manualMore.value, false);
});
test('return navigation restores account-owned results, scroll and retry state; new searches refresh', async () => {
  const memory = createSearchViewportMemory();
  let calls = 0;
  const api: SearchResultsClient = {
    searchResults: async (_e, q) => {
      calls++;
      if (q.offset) throw { code: 'transport', message: '续页失败' };
      return artistPage(q, ['1', '2']);
    },
  };
  const first = useSearchResults(api, () => {}, memory);
  first.connect(account);
  await first.open(artistQuery, true, 1);
  await first.more();
  first.savePosition({ top: 400, tracks: 0, focus: 'artist:2' });
  first.dispose();
  const second = useSearchResults(api, () => {}, memory);
  second.connect(account);
  await second.open(artistQuery, true, 1);
  assert.equal(calls, 2);
  assert.deepEqual(second.position.value, { top: 400, tracks: 0, focus: 'artist:2' });
  assert.equal(second.moreError.value, '续页失败');
  await second.open(artistQuery, true, 2);
  assert.equal(calls, 3);
  assert.equal(second.position.value.top, 0);
  second.connect({ ...account, status: 'expired' });
  second.connect(account);
  await second.open(artistQuery, true, 1);
  assert.equal(calls, 4);
});
test('browsing memory evicts old visits and never accepts a non-advancing cursor', async () => {
  const memory = createSearchViewportMemory(2);
  let calls = 0;
  const model = useSearchResults(
    {
      searchResults: async (_e, q) => {
        calls++;
        const result = artistPage(q, ['1', '2']);
        if (q.offset) result.value.nextOffset = q.offset;
        return result;
      },
    },
    () => {},
    memory,
  );
  model.connect(account);
  for (const session of [1, 2, 3, 1]) await model.open(artistQuery, true, session);
  assert.equal(calls, 4);
  await model.more();
  assert.equal(model.view.value?.sections[0]?.items.length, 2);
  assert.match(model.moreError.value, /未更新/);
});
test('unmounting an old account page cannot repopulate the replacement account memory', async () => {
  const memory = createSearchViewportMemory();
  const old = useSearchResults(
    { searchResults: async (_e, q) => artistPage(q, ['1']) },
    () => {},
    memory,
  );
  old.connect(account);
  await old.open(artistQuery, true, 1);
  memory.connect({ ...account, epoch: '2' });
  old.savePosition({ top: 100, tracks: 0 });
  assert.equal(memory.get(memory.key(artistQuery, 1), '2'), undefined);
});

test('a comprehensive correction reminder is confined to its original term, visit, and account', async () => {
  const memory = createSearchViewportMemory();
  const rewrite = '合成正词';
  const parts = [{ text: query.keyword, highlight: true }];
  let withReminder = true;
  const api: SearchResultsClient = {
    searchResults: async (epoch, q) => {
      const result = { ...view(q), epoch };
      if (q.category === 'all' && withReminder) {
        result.value.correction = rewrite;
        result.value.correctionParts = parts;
      }
      return result;
    },
  };
  const model = useSearchResults(api, () => assert.fail('no expiry'), memory);
  const songs = { ...query, category: 'songs' as const, keyword: rewrite };
  model.connect(account);
  await model.open(query, false, 7);
  await model.open(songs, false, 7, query.keyword);
  assert.deepEqual(model.correctionReminder.value, { rewrite, parts });
  await model.open(songs, false, 8, query.keyword);
  assert.equal(model.correctionReminder.value, undefined);
  await model.open(songs, false, 7, '另一个原词');
  assert.equal(model.correctionReminder.value, undefined);
  await model.open({ ...songs, correct: false }, false, 7, query.keyword);
  assert.equal(model.correctionReminder.value, undefined);

  withReminder = false;
  await model.open(query, false, 7);
  await model.open(songs, false, 7, query.keyword);
  assert.equal(model.correctionReminder.value, undefined);

  withReminder = true;
  await model.open(query, false, 7);
  model.connect({ ...account, epoch: '2' });
  await model.open(songs, false, 7, query.keyword);
  assert.equal(model.correctionReminder.value, undefined);
});

test('hot search channel survives continuation and does not share an ordinary result cache', async () => {
  const reads: SearchResultQuery[] = [];
  const model = useSearchResults(
    {
      searchResults: async (_epoch, q) => {
        reads.push(q);
        const reply = view(q);
        reply.value.more = q.offset === 0;
        reply.value.nextOffset = 7;
        return reply;
      },
    },
    () => assert.fail('no expiry'),
  );
  const ordinary = { ...query, category: 'mvs' as const };
  const hot = { ...ordinary, channel: 'hotquery' as const };
  model.connect(account);
  await model.open(hot, true, 11);
  await model.more();
  await model.open(ordinary, true, 11);
  assert.deepEqual(
    reads.map((q) => [q.offset, q.channel]),
    [
      [0, 'hotquery'],
      [7, 'hotquery'],
      [0, undefined],
    ],
  );
  assert.equal(model.view.value?.query.channel, undefined);
});

test('comprehensive tabs retain server order through category navigation, remount and Back', async () => {
  const memory = createSearchViewportMemory();
  let calls = 0;
  const api: SearchResultsClient = {
    searchResults: async (epoch, q) => {
      calls++;
      const reply = view(q);
      // A category response must not override the comprehensive route.
      reply.value.tabs = q.category === 'all' ? ['users', 'songs', 'albums'] : ['mvs'];
      return { ...reply, epoch };
    },
  };
  const first = useSearchResults(api, () => {}, memory);
  first.connect(account);
  await first.open(query, true, 7);
  await first.open(artistQuery, true, 7);
  assert.deepEqual(first.tabs.value, ['users', 'songs', 'albums']);
  assert.deepEqual(
    displayedSearchCategories(first.tabs.value, 'artists').map((tab) => tab.key),
    ['all', 'users', 'songs', 'albums', 'artists'],
  );
  first.dispose();

  const second = useSearchResults(api, () => {}, memory);
  second.connect(account);
  await second.open(artistQuery, true, 7);
  assert.deepEqual(second.tabs.value, ['users', 'songs', 'albums']);
  await second.open(query, true, 7);
  assert.deepEqual(
    displayedSearchCategories(second.tabs.value, 'all').map((tab) => tab.key),
    ['all', 'users', 'songs', 'albums'],
  );
  assert.equal(calls, 2);

  await second.open(artistQuery, true, 8);
  assert.equal(second.tabs.value, undefined);
  assert.equal(displayedSearchCategories(second.tabs.value, 'artists').length, 10);
  second.connect({ ...account, epoch: '2' });
  await second.open(artistQuery, true, 7);
  assert.equal(second.tabs.value, undefined);
  assert.equal(memory.getTabs(7, '1'), undefined);
});

test('a superseded comprehensive reply cannot populate another route or its Back memory', async () => {
  const memory = createSearchViewportMemory();
  const pending = deferred<AccountData<SearchResultView>>();
  const model = useSearchResults(
    {
      searchResults: async (_epoch, q) => (q.category === 'all' ? pending.promise : view(q)),
    },
    () => {},
    memory,
  );
  model.connect(account);
  const obsolete = model.open(query, true, 9);
  await model.open(artistQuery, true, 10);
  pending.resolve({ ...view(query), value: { ...view(query).value, tabs: ['users'] } });
  await obsolete;
  assert.equal(model.tabs.value, undefined);
  assert.equal(memory.getTabs(9, account.epoch), undefined);
});

test('server tab order preserves comprehensive position and handles duplicate keys', () => {
  assert.deepEqual(
    displayedSearchCategories(['users', 'all', 'users', 'songs'], 'songs').map((tab) => tab.key),
    ['users', 'all', 'songs'],
  );
  assert.deepEqual(
    displayedSearchCategories([], 'mvs').map((tab) => tab.key),
    ['all', 'mvs'],
  );
});

test('hot search explanation is all-only and does not repeat empty-state or notice text', () => {
  const result = {
    ...view(query).value,
    hotExplanation: { title: '  暂无单曲 ', text: '官方解释', pictureUrl: 'https://unused' },
    notice: '官方解释',
    emptyTitle: '空结果',
  };
  assert.deepEqual(searchExplanation(result, 'all', false), {
    emptyTitle: '暂无单曲',
    notice: '官方解释',
    lines: [],
  });
  assert.deepEqual(searchExplanation(result, 'all', true), {
    emptyTitle: undefined,
    notice: '官方解释',
    lines: ['暂无单曲'],
  });
  assert.deepEqual(searchExplanation(result, 'songs', false), {
    emptyTitle: '空结果',
    notice: '官方解释',
    lines: [],
  });
});

test('lyric append follows raw continuation, preserves distinct occurrences, and caps playback input', async () => {
  const calls: number[] = [];
  const api: SearchResultsClient = {
    searchResults: async (_epoch, q) => {
      calls.push(q.offset);
      const response = view(q);
      response.value.more = true;
      response.value.nextOffset = q.offset + 10;
      response.value.sections = [
        {
          id: 'lyrics',
          title: '歌词',
          category: 'lyrics',
          items: Array.from({ length: q.offset ? 10 : 9 }, (_, i) => ({
            kind: 'song' as const,
            track: {
              id: '7',
              name: 'Same song',
              artists: [],
              album: '',
              coverUrl: null,
              durationMs: 1000,
              fee: 0,
              missing: false,
              local: false,
            },
            excerpt: [`Occurrence ${q.offset + i}`],
            excerptTruncated: false,
          })),
        },
      ];
      return response;
    },
  };
  const model = useSearchResults(api, () => assert.fail('no expiry'));
  model.connect(account);
  await model.open({ ...query, category: 'lyrics', limit: 10 });
  for (let i = 0; i < 51; i++) await model.more();
  const result = model.view.value!;
  const items = result.sections[0]!.items;
  assert.deepEqual(calls.slice(0, 3), [0, 10, 20]);
  assert.equal(items.length, 500);
  assert.deepEqual(
    items.slice(8, 10).map((i) => i.kind === 'song' && i.excerpt[0]),
    ['Occurrence 8', 'Occurrence 10'],
  );
  assert.equal(result.more, false);
  assert.match(result.notice ?? '', /浏览上限/);
});

test('a terminal lyric page keeps the effective term, expanded anchor and no-more state on Back', async () => {
  const requests: [string, string, number][] = [];
  const lyric: SearchResultQuery = {
    ...query,
    keyword: '已纠正的词',
    category: 'lyrics',
    limit: 10,
  };
  const memory = createSearchViewportMemory();
  const model = useSearchResults(
    {
      searchResults: async (epoch, q) => {
        requests.push([epoch, q.keyword, q.offset]);
        const result = songPage(
          q,
          q.offset ? ['9', '9', '10'] : ['3'],
          q.offset ? 13 : 10,
          !q.offset,
        );
        result.epoch = epoch;
        result.value.sections[0]!.category = 'lyrics';
        return result;
      },
    },
    () => assert.fail('no expiry'),
    memory,
  );
  model.connect(account);
  await model.open(lyric, false, 7, '原词');
  const position = {
    top: 231,
    tracks: 0,
    focus: 'lyric:0:3',
    focusAction: 'expand',
    expandedLyrics: ['0:3'],
    lyricAnchor: { index: 0, offset: 123 },
  };
  model.savePosition(position);
  await model.more();
  assert.equal(model.view.value?.more, false);
  assert.equal(model.view.value?.nextOffset, 13);
  await model.open(lyric, true, 7, '原词');
  await model.more();
  assert.deepEqual(model.position.value, position);
  assert.equal(model.view.value?.query.keyword, '已纠正的词');
  assert.equal(model.view.value?.sections[0]?.items.length, 4);
  assert.deepEqual(requests, [
    ['1', '已纠正的词', 0],
    ['1', '已纠正的词', 10],
  ]);
  model.connect({ ...account, epoch: '2' });
  await model.open(lyric, true, 7, '原词');
  assert.equal(model.view.value?.more, true);
  assert.equal(model.position.value.top, 0);
  assert.deepEqual(requests.at(-1), ['2', '已纠正的词', 0]);
  model.dispose();
});

function songPage(q: SearchResultQuery, ids: string[], nextOffset: number, more = true) {
  const response = view(q);
  response.value.more = more;
  response.value.nextOffset = nextOffset;
  response.value.sections = [
    {
      id: 'songs',
      title: '单曲',
      category: 'songs',
      items: ids.map((id) => ({
        kind: 'song',
        track: {
          id,
          name: id,
          artists: [],
          album: '',
          coverUrl: null,
          durationMs: 0,
          fee: 0,
          missing: false,
          local: false,
        },
        excerpt: [],
        excerptTruncated: false,
      })),
    },
  ];
  return response;
}

const rewriteSongs: SearchResultQuery = {
  ...query,
  keyword: '原词',
  category: 'songs',
  limit: 10,
  channel: 'typing',
};
function rewrittenSongPage(
  q: SearchResultQuery,
  id: string,
  correction: string | null = null,
  next = q.offset + q.limit,
  more = true,
) {
  const reply = songPage(q, [id], next, more);
  reply.value.correction = correction;
  const item = reply.value.sections[0]!.items[0]!;
  assert.equal(item.kind, 'song');
  item.queueDropReceipt = `${q.keyword}:${q.offset}`;
  return reply;
}
const signedWaterfall = true;
const openRewriteSongs = (model: ReturnType<typeof useSearchResults>, reuse = false) =>
  model.open(rewriteSongs, reuse, 12, '原词', undefined, undefined, signedWaterfall);
const visibleSongReceipts = (view?: SearchResultView) =>
  view?.sections.flatMap((section) =>
    section.items.flatMap((item) => (item.kind === 'song' ? [item.queueDropReceipt] : [])),
  );

test('signed song rewrites restart both the first page and a continuation with fresh receipts', async () => {
  const calls: [string, number, number, SearchResultQuery['channel']][] = [];
  const readC = deferred<AccountData<SearchResultView>>();
  const startedC = deferred<void>();
  const memory = createSearchViewportMemory();
  const api: SearchResultsClient = {
    searchResults: async (_epoch, q, defaultReceipt, correctionReceipt) => {
      assert.equal(defaultReceipt, undefined);
      assert.equal(correctionReceipt, undefined);
      calls.push([q.keyword, q.offset, q.limit, q.channel]);
      if (q.keyword === '原词') return rewrittenSongPage(q, '1', '纠正乙');
      if (q.keyword === '纠正乙' && q.offset) {
        // Even a redirecting reply whose raw cursor did not advance is not appended.
        return rewrittenSongPage(q, '2', '纠正丙', q.offset);
      }
      if (q.keyword === '纠正丙' && !q.offset) {
        startedC.resolve();
        return readC.promise;
      }
      return rewrittenSongPage(q, q.offset ? '3' : '1');
    },
  };
  const model = useSearchResults(api, () => assert.fail('no expiry'), memory);
  model.connect(account);
  await openRewriteSongs(model);
  assert.equal(model.songRewrite.value, '纠正乙');
  assert.deepEqual(visibleSongReceipts(model.view.value), ['纠正乙:0']);
  model.savePosition({ top: 20, tracks: 300, focus: 'song-row:2:1' });
  const replacing = model.more();
  await startedC.promise;
  assert.equal(model.view.value, undefined);
  assert.equal(model.loading.value, true);
  assert.deepEqual(model.position.value, { top: 0, tracks: 0 });
  readC.resolve(rewrittenSongPage({ ...rewriteSongs, keyword: '纠正丙' }, '1'));
  await replacing;
  assert.equal(model.songRewrite.value, '纠正丙');
  assert.deepEqual(visibleSongReceipts(model.view.value), ['纠正丙:0']);
  await model.more();
  assert.deepEqual(visibleSongReceipts(model.view.value), ['纠正丙:0', '纠正丙:10']);
  assert.deepEqual(calls, [
    ['原词', 0, 10, 'typing'],
    ['纠正乙', 0, 10, 'typing'],
    ['纠正乙', 10, 10, 'typing'],
    ['纠正丙', 0, 10, 'typing'],
    ['纠正丙', 10, 10, 'typing'],
  ]);
  model.dispose();
  const back = useSearchResults(api, () => assert.fail('no expiry'), memory);
  back.connect(account);
  await openRewriteSongs(back, true);
  assert.equal(calls.length, 5);
  assert.equal(back.songRewrite.value, '纠正丙');
  assert.equal(back.view.value?.query.keyword, '纠正丙');
  assert.deepEqual(visibleSongReceipts(back.view.value), ['纠正丙:0', '纠正丙:10']);
  back.dispose();
});

test('a failed replacement evicts the previous result and retries only the pending new head', async () => {
  const calls: [string, number][] = [];
  const memory = createSearchViewportMemory();
  let failHead = true;
  const model = useSearchResults(
    {
      searchResults: async (_epoch, q) => {
        calls.push([q.keyword, q.offset]);
        if (q.offset) return rewrittenSongPage(q, '2', '纠正词', q.offset, false);
        if (q.keyword === '纠正词' && failHead) {
          failHead = false;
          throw { code: 'transport', message: '新搜索暂不可用' };
        }
        return rewrittenSongPage(q, '1');
      },
    },
    () => assert.fail('no expiry'),
    memory,
  );
  model.connect(account);
  await openRewriteSongs(model);
  await model.more();
  assert.equal(model.view.value, undefined);
  assert.equal(model.error.value, '新搜索暂不可用');
  assert.equal(model.moreError.value, '');
  assert.equal(model.songRewrite.value, '纠正词');
  assert.equal(memory.get(memory.key(rewriteSongs, 12), account.epoch), undefined);
  await model.more();
  assert.deepEqual(calls, [
    ['原词', 0],
    ['原词', 10],
    ['纠正词', 0],
  ]);
  await model.retrySongRewrite();
  assert.deepEqual(calls.at(-1), ['纠正词', 0]);
  assert.equal(model.error.value, '');
  assert.deepEqual(visibleSongReceipts(model.view.value), ['纠正词:0']);
  assert.equal(model.retrySongRewrite(), undefined);
});

test('a late rewritten head cannot take over a new account or its browsing memory', async () => {
  const readOld = deferred<AccountData<SearchResultView>>();
  const startedOld = deferred<void>();
  const calls: [string, string, number][] = [];
  const memory = createSearchViewportMemory();
  const model = useSearchResults(
    {
      searchResults: async (epoch, q) => {
        calls.push([epoch, q.keyword, q.offset]);
        if (epoch === '2') return { ...rewrittenSongPage(q, '20'), epoch };
        if (q.keyword === '原词') return rewrittenSongPage(q, '1', '旧账号纠正');
        startedOld.resolve();
        return readOld.promise;
      },
    },
    () => assert.fail('late expiry'),
    memory,
  );
  model.connect(account);
  const old = openRewriteSongs(model);
  await startedOld.promise;
  model.connect({ ...account, epoch: '2' });
  await openRewriteSongs(model);
  readOld.resolve(rewrittenSongPage({ ...rewriteSongs, keyword: '旧账号纠正' }, '99', '不得继续'));
  await old;
  assert.deepEqual(calls, [
    ['1', '原词', 0],
    ['1', '旧账号纠正', 0],
    ['2', '原词', 0],
  ]);
  assert.equal(model.songRewrite.value, null);
  assert.equal(model.view.value?.sections[0]?.items[0]?.kind, 'song');
  assert.deepEqual(visibleSongReceipts(model.view.value), ['原词:0']);
  assert.equal(memory.get(memory.key(rewriteSongs, 12), '1'), undefined);
  assert.deepEqual(visibleSongReceipts(memory.get(memory.key(rewriteSongs, 12), '2')?.view), [
    '原词:0',
  ]);
});

test('signed song rewrite loops and excessive chains stop before another automatic request', async () => {
  for (const [targets, expected] of [
    [
      ['乙', '原词'],
      ['原词', '乙'],
    ],
    [
      ['乙', '丙', '丁', '戊'],
      ['原词', '乙', '丙', '丁'],
    ],
  ] as const) {
    const calls: string[] = [];
    const model = useSearchResults(
      {
        searchResults: async (_epoch, q) => {
          assert.equal(q.offset, 0);
          const correction = targets[calls.length] ?? null;
          calls.push(q.keyword);
          return rewrittenSongPage(q, '1', correction);
        },
      },
      () => assert.fail('no expiry'),
    );
    model.connect(account);
    await openRewriteSongs(model);
    await model.more();
    assert.deepEqual(calls, expected);
    assert.match(model.error.value, /纠错结果反复变化/);
    assert.equal(model.view.value, undefined);
    assert.equal(model.loading.value, false);
    assert.equal(model.songRewrite.value, expected.at(-1));
  }
});

test('a verified rewrite to the original is authoritative over the inherited term', async () => {
  const calls: [string, number][] = [];
  const model = useSearchResults(
    {
      searchResults: async (_epoch, q) => {
        calls.push([q.keyword, q.offset]);
        if (q.keyword === '综合继承词' && q.offset) return rewrittenSongPage(q, '2', '原词');
        return rewrittenSongPage(q, '1');
      },
    },
    () => assert.fail('no expiry'),
  );
  model.connect(account);
  await model.open(
    { ...rewriteSongs, keyword: '综合继承词' },
    false,
    12,
    '原词',
    undefined,
    undefined,
    signedWaterfall,
  );
  assert.equal(model.songRewrite.value, '综合继承词');
  await model.more();
  assert.deepEqual(calls, [
    ['综合继承词', 0],
    ['综合继承词', 10],
    ['原词', 0],
  ]);
  assert.equal(model.songRewrite.value, null);
  assert.deepEqual(visibleSongReceipts(model.view.value), ['原词:0']);
});

test('a mismatched echo never authorizes a rewrite or replacement', async () => {
  const calls: [string, number][] = [];
  const model = useSearchResults(
    {
      searchResults: async (_epoch, q) => {
        calls.push([q.keyword, q.offset]);
        return q.offset
          ? rewrittenSongPage({ ...q, keyword: '别的请求' }, '2', '不应继续')
          : rewrittenSongPage(q, '1');
      },
    },
    () => assert.fail('no expiry'),
  );
  model.connect(account);
  await openRewriteSongs(model);
  await model.more();
  assert.deepEqual(calls, [
    ['原词', 0],
    ['原词', 10],
  ]);
  assert.equal(model.moreError.value, '搜索结果已更新，请重试');
  assert.equal(model.songRewriteReset.value, 0);
  assert.deepEqual(visibleSongReceipts(model.view.value), ['原词:0']);
});

test('still-search, numbered pages and guest reads do not acquire signed waterfall rewriting', async () => {
  const cases: [AccountView, SearchResultQuery, boolean, string | undefined][] = [
    [account, { ...rewriteSongs, correct: false, channel: undefined }, false, 'original-receipt'],
    [account, { ...rewriteSongs, offset: 30, limit: 30 }, false, undefined],
    [{ ...account, status: 'signed_out' }, rewriteSongs, true, undefined],
  ];
  for (const [viewer, input, guest, receipt] of cases) {
    const calls: [SearchResultQuery, string | undefined][] = [];
    const model = useSearchResults(
      {
        searchResults: async (_epoch, q, _default, correction) => {
          calls.push([q, correction]);
          return rewrittenSongPage(q, '1', '未启用的重开');
        },
      },
      () => assert.fail('no expiry'),
    );
    model.connect(viewer, guest);
    await model.open(input, false, 12, '原词', undefined, receipt, signedWaterfall);
    assert.deepEqual(calls, [[input, receipt]]);
    assert.equal(model.songRewrite.value, undefined);
    assert.equal(model.songRewriteReset.value, 0);
    assert.deepEqual(visibleSongReceipts(model.view.value), [`原词:${input.offset}`]);
  }
});

test('song waterfall retains its prefix and exact cursor through retry and numbered fallback', async () => {
  const calls: [number, number][] = [];
  let fail = true;
  const model = useSearchResults(
    {
      searchResults: async (_epoch, q) => {
        calls.push([q.offset, q.limit]);
        if (q.limit === 30) return songPage(q, ['20'], 30, false);
        if (q.offset === 0) return songPage(q, ['1', '2'], 10);
        if (q.offset === 10) {
          if (fail) throw { code: 'network', message: '网络暂不可用' };
          // Raw cursor advances even when this page repeats previous results.
          return songPage(q, ['1', '2'], 20);
        }
        return songPage(q, ['2', '3', '4'], 30, false);
      },
    },
    () => assert.fail('no expiry'),
  );
  const songs = { ...query, category: 'songs' as const, limit: 10 };
  const ids = () =>
    model.view.value?.sections[0]?.items.map((item) => item.kind === 'song' && item.track.id);
  const position = { top: 0, tracks: 48, focus: 'song-row:3:2', focusAction: 'album' };
  model.connect(account);
  await model.open(songs, true, 1);
  model.savePosition(position);
  await model.more();
  assert.deepEqual(ids(), ['1', '2']);
  assert.equal(model.moreError.value, '网络暂不可用');
  await model.open({ ...songs, limit: 30 }, true, 1);
  assert.deepEqual(ids(), ['20']);
  await model.open(songs, true, 1);
  assert.deepEqual(ids(), ['1', '2']);
  assert.deepEqual(model.position.value, position);
  assert.equal(model.moreError.value, '网络暂不可用');
  fail = false;
  await model.more();
  assert.deepEqual(ids(), ['1', '2']);
  assert.equal(model.view.value?.more, true);
  await model.more();
  assert.deepEqual(ids(), ['1', '2', '3', '4']);
  assert.deepEqual(calls, [
    [0, 10],
    [10, 10],
    [0, 30],
    [10, 10],
    [20, 10],
  ]);
  assert.equal(model.view.value?.more, false);
});

test('song waterfall bounds the loaded playback selection and stops at the cap', async () => {
  let requests = 0;
  const model = useSearchResults(
    {
      searchResults: async (_epoch, q) => {
        requests++;
        return songPage(
          q,
          Array.from({ length: 10 }, (_, i) => String(q.offset + i + 1)),
          q.offset + 10,
        );
      },
    },
    () => assert.fail('no expiry'),
  );
  model.connect(account);
  await model.open({ ...query, category: 'songs', limit: 10 });
  for (let i = 0; i < 52; i++) await model.more();
  const result = model.view.value!;
  assert.equal(requests, 50);
  assert.equal(result.sections[0]?.items.length, 500);
  assert.equal(result.more, false);
  assert.match(result.notice ?? '', /单曲结果已达到本次连续浏览上限/);
});

test('verified rights refresh patches the first six ordinary cards without changing visit memory', async () => {
  const song = (id: string) => ({
    kind: 'song' as const,
    track: {
      id,
      name: id,
      artists: [],
      album: '',
      coverUrl: null,
      durationMs: 40000,
      fee: 1,
      missing: false,
      local: false,
    },
    excerpt: [],
    qualityBadge: 'sq' as const,
  });
  const page = view(query);
  page.value.privilegeReceipt = 'observed-receipt';
  page.value.sections = [
    { id: 'search_block_best_match:0', title: '最佳匹配', category: null, items: [song('1')] },
    {
      id: 'search_block_song:1',
      title: '单曲',
      category: 'songs',
      items: ['1', '2', '3', '4', '5', '6', '1'].map(song),
    },
  ];
  type Observation = import('../src/services/search.ts').SearchPrivilegeObservation;
  type Patch = import('../src/services/search.ts').SearchPrivilegePatch;
  let push!: (event: Observation) => void;
  let stop = 0,
    searchCalls = 0;
  const calls: (string | undefined)[] = [];
  let held = deferred<AccountData<Patch>>();
  const model = useSearchResults(
    {
      searchReasonPage: async () => {
        throw Error('unexpected');
      },
      searchResults: async (_e, q) => (++searchCalls, q.category === 'all' ? page : view(q)),
      onSearchPrivilegeChanged: async (handler) => {
        push = handler;
        return () => {
          stop++;
        };
      },
      searchRefreshPrivileges: (epoch, receipt, observation) => {
        assert.equal(epoch, '1');
        assert.equal(receipt, 'observed-receipt');
        calls.push(observation);
        return held.promise;
      },
    },
    () => assert.fail('no active auth error'),
  );
  const patch: AccountData<Patch> = {
    epoch: '1',
    value: {
      receipt: 'observed-receipt',
      section: 'search_block_song:1',
      playAllIds: ['1', '2', '3', '4', '5', '6'],
      rows: [
        {
          id: '1',
          coverUrl: null,
          mvId: '601',
          replacementMv: false,
          vipBadge: false,
          limitedFree: true,
          qualityBadge: 'hires',
        },
        {
          id: '2',
          coverUrl: null,
          mvId: null,
          replacementMv: false,
          vipBadge: true,
          limitedFree: false,
          qualityBadge: null,
        },
      ],
    },
  };
  model.connect(account);
  await model.open(query, true, 23);
  assert.deepEqual(calls, [], 'cold route has no second request');
  assert.equal(comprehensivePlayAllTracks(model.view.value!.sections[1]).length, 7);
  model.savePosition({ top: 231, tracks: 0, focus: 'song:1', focusAction: 'more' });
  push({ epoch: '2', revision: 'wrong account' });
  push({ epoch: '1', revision: 'confirmed' });
  push({ epoch: '1', revision: 'confirmed' });
  assert.deepEqual(calls, ['confirmed']);
  held.resolve(patch);
  await held.promise;
  const [best, section] = model.view.value!.sections;
  assert.equal(best, page.value.sections[0]);
  assert.equal(section.items[0].kind === 'song' && section.items[0].mvId, '601');
  assert.equal(section.items[0].kind === 'song' && section.items[0].limitedFree, true);
  assert.equal(section.items[0].kind === 'song' && section.items[0].qualityBadge, 'hires');
  assert.equal(section.items[1].kind === 'song' && section.items[1].qualityBadge, undefined);
  assert.equal(section.items[2].kind === 'song' && section.items[2].qualityBadge, 'sq');
  assert.equal(section.items[6], page.value.sections[1].items[6]);
  assert.deepEqual(
    comprehensivePlayAllTracks(section).map((row) => row.id),
    ['1', '2', '3', '4', '5', '6'],
  );
  assert.deepEqual(model.position.value, {
    top: 231,
    tracks: 0,
    focus: 'song:1',
    focusAction: 'more',
  });
  assert.equal(searchCalls, 1);

  // Opening a cached comprehensive visit restores position and refreshes once.
  held = deferred();
  await model.open(query, true, 23);
  assert.deepEqual(calls, ['confirmed', undefined]);
  const other = { ...query, category: 'songs' as const };
  await model.open(other, true, 23);
  held.reject({ code: 'session_expired', message: 'stale refresh' });
  await held.promise.catch(() => {});
  assert.deepEqual(model.view.value?.query, other);
  assert.equal(model.error.value, '');
  model.dispose();
  assert.equal(stop, 1);
});
