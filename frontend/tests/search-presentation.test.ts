import assert from 'node:assert/strict';
import test from 'node:test';
import { useSearchResults } from '../src/composables/useSearchResults.ts';
import { createSearchViewportMemory } from '../src/services/searchViewport.ts';
import { patchSearchPresentation } from '../src/services/searchPresentation.ts';
import type { AccountData, AccountView } from '../src/services/contracts.ts';
import type { PcPresentationView } from '../src/services/pcPresentation.ts';
import type {
  SearchPresentationPatch,
  SearchPrivilegeObservation,
  SearchPrivilegePatch,
  SearchResultView,
  SearchResultsClient,
} from '../src/services/search.ts';

const account: AccountView = {
  epoch: '1',
  status: 'authenticated',
  nickname: null,
  avatarUrl: null,
};
const query = {
  keyword: 'Synthetic',
  category: 'all' as const,
  offset: 0,
  limit: 30,
  correct: true,
};
const observation = (revision: string, footer = true): PcPresentationView => ({
  epoch: '1',
  revision,
  searchFooter: footer,
  searchExpanded: footer,
  searchDense: footer,
});
function page(): SearchResultView {
  return {
    query,
    presentationReceipt: 'page-1',
    presentationRevision: '1',
    privilegeReceipt: 'rights-1',
    total: 7,
    more: false,
    nextOffset: 0,
    correction: null,
    unavailableCount: 0,
    notice: null,
    sections: [
      {
        id: 'search_block_song:0',
        title: 'Songs',
        category: 'songs',
        items: Array.from({ length: 7 }, (_, n) => ({
          kind: 'song' as const,
          track: {
            id: String(n + 1),
            name: 'Song',
            artists: [],
            album: '',
            coverUrl: null,
            durationMs: 60000,
            fee: 1,
            missing: false,
            local: false,
          },
          excerpt: [],
          hint: { kind: 'alias' as const, text: 'original' },
        })),
      },
    ],
  };
}
function patch(revision: string, footer = true): AccountData<SearchPresentationPatch> {
  return {
    epoch: '1',
    value: {
      receipt: 'page-1',
      revision,
      previewLayout: footer ? 'compact_carousel' : null,
      sections: [
        {
          id: 'search_block_song:0',
          items: Array.from({ length: 6 }, (_, index) => ({
            index,
            trackId: String(index + 1),
            hint: footer ? { kind: 'alias' as const, text: revision } : null,
            reasons: [],
          })),
        },
      ],
    },
  };
}
const hint = (view: SearchResultView | undefined) => {
  const item = view?.sections[0]?.items[0];
  return item?.kind === 'song' ? item.hint : undefined;
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const tick = () => new Promise<void>((done) => setImmediate(done));

for (const delivery of ['before ordinary rows', 'after ordinary rows']) {
  test(`a delayed signed startup view restores its existing receipt ${delivery}`, async () => {
    const memory = createSearchViewportMemory();
    memory.connect(account);
    memory.observePresentation(observation('1', false));
    const stale = deferred<AccountData<SearchPresentationPatch>>();
    const revisions: string[] = [];
    let searches = 0;
    const model = useSearchResults(
      {
        searchResults: async () => {
          searches++;
          return { epoch: '1', value: { ...page(), presentationRevision: '2' } };
        },
        searchPresentation: async (_epoch, receipt, revision) => {
          assert.equal(receipt, 'page-1');
          revisions.push(revision);
          return revision === '1' ? stale.promise : patch('2');
        },
      } as SearchResultsClient,
      () => assert.fail('no auth transition'),
      memory,
    );
    model.connect(account);
    const opening = model.open(query, false, 1);
    await tick();
    assert.deepEqual(revisions, ['1']);
    if (delivery === 'before ordinary rows') memory.observePresentation(observation('2'));
    stale.reject({ code: 'stale', message: 'signed native revision already advanced' });
    await opening;
    if (delivery === 'after ordinary rows') {
      assert.equal(model.loading.value, false);
      assert.equal(hint(model.view.value), undefined, 'ordinary rows may render before the event');
      memory.observePresentation(observation('2'));
    }
    await tick();
    assert.deepEqual(hint(model.view.value), { kind: 'alias', text: '2' });
    assert.equal(model.view.value?.presentationRevision, '2');
    assert.deepEqual(
      revisions,
      ['1', '2'],
      'project the same existing receipt at the new revision',
    );
    assert.equal(searches, 1, 'the delayed event must never replay the search');
    model.dispose();
  });
}

test('live presentation removes footers immediately and only the newest native receipt may restore them', async () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  memory.observePresentation(observation('1'));
  const off = deferred<AccountData<SearchPresentationPatch>>(),
    on = deferred<AccountData<SearchPresentationPatch>>();
  let searches = 0;
  const projections: string[] = [];
  const model = useSearchResults(
    {
      searchResults: async () => {
        searches++;
        return { epoch: '1', value: page() };
      },
      searchPresentation: (_epoch, receipt, revision) => {
        assert.equal(receipt, 'page-1');
        projections.push(revision);
        return revision === '2' ? off.promise : on.promise;
      },
    } as SearchResultsClient,
    () => assert.fail('no auth transition'),
    memory,
  );
  model.connect(account);
  await model.open(query, true, 11);
  const position = {
    top: 312,
    tracks: 0,
    focus: 'song:1',
    focusAction: 'reason:0',
    previewPositions: { albums: 2 },
  };
  model.savePosition(position);
  memory.observePresentation(observation('2', false));
  assert.equal(hint(model.view.value), undefined, 'revocation hides before local IPC completion');
  memory.observePresentation(observation('3'));
  on.resolve(patch('3'));
  await tick();
  assert.deepEqual(hint(model.view.value), { kind: 'alias', text: '3' });
  off.resolve(patch('2', false));
  await tick();
  assert.deepEqual(hint(model.view.value), { kind: 'alias', text: '3' });
  assert.equal(model.view.value?.previewLayout, 'compact_carousel');
  assert.equal(
    model.view.value?.sections[0]?.items.length,
    7,
    'layout projection never loses expansion candidates',
  );
  assert.deepEqual(model.position.value, position);
  assert.equal(searches, 1);
  assert.deepEqual(projections, ['2', '3']);
  model.dispose();
});

test('Back projects its account-owned snapshot before publication and eviction never replays search', async () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  memory.observePresentation(observation('1'));
  let searches = 0,
    projections = 0;
  const held = deferred<AccountData<SearchPresentationPatch>>();
  const api = {
    searchResults: async () => {
      searches++;
      return { epoch: '1', value: page() };
    },
    searchPresentation: () => {
      projections++;
      return held.promise;
    },
  } as SearchResultsClient;
  const first = useSearchResults(api, () => {}, memory);
  first.connect(account);
  await first.open(query, true, 9);
  first.savePosition({ top: 290, tracks: 0, focus: 'song:1' });
  first.dispose();
  memory.observePresentation(observation('2'));
  const back = useSearchResults(api, () => {}, memory);
  back.connect(account);
  const opening = back.open(query, true, 9);
  assert.equal(back.view.value, undefined, 'old granted footer is not flashed on Back');
  held.reject({ code: 'stale', message: 'evicted native receipt' });
  await opening;
  await tick();
  assert.equal(hint(back.view.value), undefined);
  assert.equal(back.view.value?.sections[0]?.items.length, 7);
  assert.deepEqual(back.position.value, { top: 290, tracks: 0, focus: 'song:1' });
  assert.equal(searches, 1);
  assert.equal(projections, 1, 'failure adds no automatic retry');
  back.dispose();
});

test('a page without a native candidate receipt cannot acquire footer eligibility from an event', async () => {
  const memory = createSearchViewportMemory();
  const source = page();
  delete source.presentationReceipt;
  let projections = 0;
  const model = useSearchResults(
    {
      searchResults: async () => ({ epoch: '1', value: source }),
      searchPresentation: async () => {
        projections++;
        return patch('2');
      },
    } as SearchResultsClient,
    () => {},
    memory,
  );
  model.connect(account);
  await model.open(query, true, 1);
  memory.observePresentation(observation('2'));
  await tick();
  assert.equal(hint(model.view.value), undefined);
  assert.equal(model.view.value?.sections[0]?.items.length, 7);
  assert.equal(projections, 0, 'no raw rows or renderer event substitute for a native receipt');
  model.dispose();
});

test('Back rechecks a native receipt even when its global presentation revision is unchanged', async () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  memory.observePresentation(observation('1'));
  let searches = 0,
    projections = 0;
  const api = {
    searchResults: async () => {
      searches++;
      return { epoch: '1', value: page() };
    },
    searchPresentation: async () => {
      projections++;
      throw { code: 'stale' };
    },
  } as SearchResultsClient;
  const first = useSearchResults(api, () => {}, memory);
  first.connect(account);
  await first.open(query, true, 8);
  assert(hint(first.view.value));
  first.dispose();
  const back = useSearchResults(api, () => {}, memory);
  back.connect(account);
  await back.open(query, true, 8);
  assert.equal(hint(back.view.value), undefined, 'eviction removes old footer without searching');
  assert.equal(projections, 1);
  assert.equal(searches, 1);
  back.dispose();
});

for (const order of ['privilege-first', 'presentation-first']) {
  test(`membership and presentation merge without overwriting each other (${order})`, async () => {
    const memory = createSearchViewportMemory();
    memory.connect(account);
    memory.observePresentation(observation('1'));
    const appearance = deferred<AccountData<SearchPresentationPatch>>();
    const rights = deferred<AccountData<SearchPrivilegePatch>>();
    let push!: (event: SearchPrivilegeObservation) => void;
    const model = useSearchResults(
      {
        searchResults: async () => ({ epoch: '1', value: page() }),
        searchPresentation: () => appearance.promise,
        onSearchPrivilegeChanged: async (listener) => {
          push = listener;
          return () => {};
        },
        searchRefreshPrivileges: () => rights.promise,
      } as SearchResultsClient,
      () => assert.fail('same account'),
      memory,
    );
    model.connect(account);
    await model.open(query, true, 1);
    memory.observePresentation(observation('2'));
    push({ epoch: '1', revision: 'verified-vip' });
    const resolvedRights: AccountData<SearchPrivilegePatch> = {
      epoch: '1',
      value: {
        receipt: 'rights-1',
        section: 'search_block_song:0',
        playAllIds: ['1', '2', '3', '4', '5', '6'],
        rows: [
          {
            id: '1',
            coverUrl: 'https://p1.music.126.net/synthetic-new.jpg',
            mvId: '55',
            replacementMv: false,
            vipBadge: false,
            limitedFree: true,
            qualityBadge: 'sky',
          },
        ],
      },
    };
    if (order === 'privilege-first') {
      rights.resolve(resolvedRights);
      await tick();
    }
    appearance.resolve(patch('2'));
    await tick();
    if (order === 'presentation-first') {
      rights.resolve(resolvedRights);
      await tick();
    }
    const section = model.view.value!.sections[0];
    const row = section.items[0];
    assert(row.kind === 'song');
    assert.deepEqual(row.hint, { kind: 'alias', text: '2' });
    assert.equal(row.track.coverUrl, resolvedRights.value.rows[0].coverUrl);
    assert.equal(row.mvId, '55');
    assert.equal(row.limitedFree, true);
    assert.equal(row.qualityBadge, 'sky');
    assert.deepEqual(section.playAllIds, resolvedRights.value.playAllIds);
    model.dispose();
  });
}

test('cross-account late projection and mismatched row identity cannot publish candidates', async () => {
  const source = page();
  const bad = patch('2').value;
  bad.sections[0].items[0].trackId = 'other-song';
  assert.equal(patchSearchPresentation(source, bad), undefined);
  const memory = createSearchViewportMemory();
  memory.connect(account);
  memory.observePresentation(observation('1'));
  const held = deferred<AccountData<SearchPresentationPatch>>();
  const model = useSearchResults(
    {
      searchResults: async () => ({ epoch: '1', value: source }),
      searchPresentation: () => held.promise,
    } as SearchResultsClient,
    () => {},
    memory,
  );
  model.connect(account);
  await model.open(query, true, 1);
  memory.observePresentation(observation('2'));
  model.connect({ ...account, epoch: '2' });
  held.resolve(patch('2'));
  await tick();
  assert.equal(model.view.value, undefined);
  assert.equal(memory.presentation('2'), undefined);
  model.dispose();
});
