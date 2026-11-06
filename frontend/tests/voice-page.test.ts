import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, shallowRef, watch } from 'vue';
import { useVoicePage } from '../src/composables/useVoicePage.ts';
import type { TrackView } from '../src/services/contracts.ts';
import type { PlayerView } from '../src/services/player.ts';
import {
  currentVoicePageDetail,
  voicePageCreatorTarget,
  voicePagePodcastTarget,
  voicePageScope,
  type VinylVoiceDetailView,
  type VoicePageScope,
} from '../src/services/voicePage.ts';
import type { VoiceRecommendationsPage } from '../src/services/voiceRecommendations.ts';
import type { VoiceSongsView } from '../src/services/voiceSongs.ts';

const a: VoicePageScope = { epoch: '1', playId: 'a', entryId: 'entry-a', voiceId: '71' };
const b: VoicePageScope = { epoch: '2', playId: 'b', entryId: 'entry-b', voiceId: '72' };

function detail(scope: VoicePageScope): VinylVoiceDetailView {
  return {
    ...scope,
    title: `节目 ${scope.voiceId}`,
    detail: { id: scope.voiceId, creator: null, releaseTime: null, playCount: '0' },
  };
}

function recommendations(
  scope: VoicePageScope,
  receipt: string,
  page = 1,
): VoiceRecommendationsPage {
  return {
    playId: scope.playId,
    voiceId: scope.voiceId,
    receipt,
    page,
    pageCount: 2,
    cards: [{ radioId: String(90 + page), name: `播单 ${page}`, playCount: 0 }],
  };
}

function songs(scope: VoicePageScope, receipt: string): VoiceSongsView {
  return {
    playId: scope.playId,
    voiceId: scope.voiceId,
    receipt,
    tracks: [
      { id: '301', name: '第一条' },
      { id: '301', name: '重复条目' },
    ] as TrackView[],
    availability: [
      { onlineEligible: true, copyrightBlocked: false },
      { onlineEligible: false, copyrightBlocked: true },
    ],
  };
}

function setup(initial?: VoicePageScope) {
  const scope = shallowRef<VoicePageScope | undefined>(initial);
  const lifetime = effectScope();
  const order: [string, string[]][] = [];
  const channel = <T>(lane: string) => {
    const requests: PromiseWithResolvers<T>[] = [];
    const call = (...args: string[]) => {
      order.push([lane, args]);
      const request = Promise.withResolvers<T>();
      requests.push(request);
      return request.promise;
    };
    return { requests, call };
  };
  const details = channel<VinylVoiceDetailView>('detail');
  const recs = channel<VoiceRecommendationsPage>('recommendations');
  const rows = channel<VoiceSongsView>('songs');
  const pages = channel<VoiceRecommendationsPage>('next');
  const model = lifetime.run(() =>
    useVoicePage(
      {
        playerVinylVoiceDetail: details.call,
        playerVinylVoiceRecommendations: recs.call,
        playerVoiceSongs: rows.call,
        playerVinylVoiceRecommendationsNext: pages.call,
      },
      scope,
    ),
  )!;
  return { scope, model, lifetime, order, details, recs, rows, pages };
}

const trio = (scope: VoicePageScope) =>
  ['detail', 'recommendations', 'songs'].map((lane) => [lane, [scope.epoch, scope.playId]]);

async function settle() {
  await Promise.resolve();
  await nextTick();
}

test('joins the official three reads and keeps native receipts for paging and song rows', async () => {
  const f = setup(a);
  try {
    assert.deepEqual(f.order, trio(a), 'all requests start before any reply');
    assert.equal(f.model.status.value, 'loading');
    const releases = {
      detail: detail(a),
      recommendations: recommendations(a, 'recommend-a'),
      songs: songs(a, 'rows-a'),
    };
    const seen: unknown[] = [];
    f.lifetime.run(() =>
      watch(
        f.model.snapshot,
        () =>
          seen.push({
            detail: f.model.detail.value,
            recommendations: f.model.recommendations.view.value,
            songs: f.model.songs.view.value,
          }),
        { flush: 'sync' },
      ),
    );
    f.recs.requests[0]!.resolve(releases.recommendations);
    f.rows.requests[0]!.resolve(releases.songs);
    await settle();
    assert.equal(f.model.snapshot.value, undefined);
    assert.equal(f.model.recommendations.target(releases.recommendations, '91'), undefined);
    assert.equal(f.model.songs.target(releases.songs, 0), undefined);
    assert.deepEqual(seen, []);
    f.details.requests[0]!.resolve(releases.detail);
    await settle();
    assert.equal(f.model.status.value, 'ready');
    assert.deepEqual(seen, [releases], 'one synchronous observer sees all three lanes');
    const song = f.model.songs.target(releases.songs, 1)!;
    assert.deepEqual(song, { epoch: '1', playId: 'a', voiceId: '71', receipt: 'rows-a', index: 1 });
    assert.equal(f.model.songs.ownsTarget(song), true);
    const radio = f.model.recommendations.target(releases.recommendations, '91')!;
    assert.deepEqual(radio, {
      epoch: '1',
      playId: 'a',
      voiceId: '71',
      receipt: 'recommend-a',
      page: 1,
      radioId: '91',
    });
    const advance = f.model.recommendations.next();
    await f.model.recommendations.next();
    assert.deepEqual(f.order, [...trio(a), ['next', ['1', 'a', 'recommend-a']]]);
    f.pages.requests[0]!.resolve(recommendations(a, 'recommend-a', 2));
    await advance;
    assert.equal(f.model.recommendations.view.value?.page, 2);
    assert.equal(f.model.recommendations.ownsTarget(radio), false);
    assert.equal(f.model.detail.value, releases.detail);
    assert.equal(f.model.songs.ownsTarget(song), true);
  } finally {
    f.lifetime.stop();
  }
});

test('one failed lane never publishes siblings or retries; same-tick reopen reads again', async () => {
  const f = setup(a);
  try {
    f.details.requests[0]!.reject(new Error('synthetic program detail refusal'));
    await settle();
    assert.equal(f.model.status.value, 'failed');
    f.recs.requests[0]!.resolve(recommendations(a, 'abandoned'));
    f.rows.requests[0]!.resolve(songs(a, 'abandoned'));
    f.scope.value = { ...a };
    await settle();
    assert.deepEqual(f.order, trio(a));
    assert.equal(f.model.snapshot.value, undefined);
    assert.equal(f.model.recommendations.view.value, undefined);
    assert.equal(f.model.songs.view.value, undefined);
    f.scope.value = undefined;
    f.scope.value = a;
    await nextTick();
    assert.deepEqual(f.order, [...trio(a), ...trio(a)]);
    f.details.requests[1]!.resolve({ ...detail(a), entryId: 'other entry' });
    f.recs.requests[1]!.resolve(recommendations(a, 'new'));
    f.rows.requests[1]!.resolve(songs(a, 'new'));
    await settle();
    assert.equal(f.model.status.value, 'failed');
    assert.equal(f.model.snapshot.value, undefined);
  } finally {
    f.lifetime.stop();
  }
});

test('a full reply from before close cannot publish when the exact same play is reopened', async () => {
  const f = setup(a);
  try {
    f.scope.value = undefined;
    f.scope.value = a;
    await nextTick();
    assert.deepEqual(f.order, [...trio(a), ...trio(a)]);
    f.details.requests[0]!.resolve(detail(a));
    f.recs.requests[0]!.resolve(recommendations(a, 'closed'));
    f.rows.requests[0]!.resolve(songs(a, 'closed-songs'));
    await settle();
    assert.equal(f.model.status.value, 'loading');
    assert.equal(f.model.snapshot.value, undefined);
    f.details.requests[1]!.resolve(detail(a));
    f.recs.requests[1]!.resolve(recommendations(a, 'open'));
    f.rows.requests[1]!.resolve(songs(a, 'open-songs'));
    await settle();
    assert.equal(f.model.status.value, 'ready');
    assert.equal(f.model.recommendations.view.value?.receipt, 'open');
    assert.equal(f.model.songs.view.value?.receipt, 'open-songs');
  } finally {
    f.lifetime.stop();
  }
});

test('account and entry changes clear old targets and cannot deliver an older visit', async () => {
  const f = setup(a);
  try {
    const previous = recommendations(a, 'old');
    f.details.requests[0]!.resolve(detail(a));
    f.recs.requests[0]!.resolve(previous);
    f.rows.requests[0]!.resolve(songs(a, 'old-songs'));
    await settle();
    const target = f.model.recommendations.target(previous, '91')!;
    const paging = f.model.recommendations.next();
    assert.equal(f.model.recommendations.paging.value, true);
    f.scope.value = b;
    assert.equal(f.model.recommendations.ownsTarget(target), false);
    assert.equal(f.model.recommendations.paging.value, false);
    assert.equal(f.model.snapshot.value, undefined);
    await nextTick();
    const replaced = { ...b, entryId: 'replacement' };
    f.scope.value = replaced;
    await nextTick();
    f.details.requests[2]!.resolve(detail(replaced));
    f.recs.requests[2]!.resolve(recommendations(replaced, 'current'));
    f.rows.requests[2]!.resolve(songs(replaced, 'current-songs'));
    await settle();
    assert.equal(f.model.detail.value?.entryId, 'replacement');
    f.details.requests[1]!.resolve(detail(b));
    f.recs.requests[1]!.resolve(recommendations(b, 'late'));
    f.rows.requests[1]!.resolve(songs(b, 'late-songs'));
    f.pages.requests[0]!.resolve(recommendations(a, 'old', 2));
    await paging;
    await settle();
    assert.equal(f.model.detail.value?.entryId, 'replacement');
    assert.equal(f.model.recommendations.view.value?.receipt, 'current');
    assert.equal(f.model.songs.view.value?.receipt, 'current-songs');
    f.lifetime.stop();
    assert.equal(f.model.status.value, 'idle');
    assert.equal(f.model.snapshot.value, undefined);
  } finally {
    f.lifetime.stop();
  }
});

test('a page visit requires an active native voice and exact entry and epoch', () => {
  const view = {
    epoch: a.epoch,
    playId: a.playId,
    entryId: a.entryId,
    state: 'paused',
    resourceKind: 'voice',
    track: { id: '1601', voiceId: a.voiceId },
    voiceDetail: detail(a).detail,
  } as PlayerView;
  assert.deepEqual(voicePageScope(view, '1'), a);
  assert.equal(voicePageScope(view, '2'), undefined);
  assert.equal(voicePageScope({ ...view, entryId: null }, '1'), undefined);
  assert.equal(voicePageScope({ ...view, state: 'loading' }, '1'), undefined);
  assert.equal(voicePageScope({ ...view, fm: true }, '1'), undefined);
});

test('fresh program labels do not grant a navigation or subscription target for different native IDs', () => {
  const fresh = detail(a);
  fresh.detail.creator = { id: '41', name: '新主播名' };
  fresh.detail.radio = { id: '51', name: '新播单名', ownedByViewer: false };
  const current = {
    ...a,
    state: 'paused',
    resourceKind: 'voice',
    track: { id: '1601', voiceId: a.voiceId },
    voiceDetail: {
      ...fresh.detail,
      creator: { id: '41', name: '旧主播名' },
      radio: { id: '51', name: '旧播单名', ownedByViewer: false },
    },
  } as PlayerView;
  assert.equal(currentVoicePageDetail(fresh, current, a.epoch), fresh);
  assert.equal(voicePageCreatorTarget(fresh, current, a.epoch)?.userId, '41');
  assert.equal(voicePagePodcastTarget(fresh, current, a.epoch)?.radioId, '51');

  const changed = {
    ...fresh,
    detail: {
      ...fresh.detail,
      creator: { id: '42', name: '另一位主播' },
      radio: { id: '52', name: '另一个播单', ownedByViewer: false },
    },
  };
  assert.equal(currentVoicePageDetail(changed, current, a.epoch), changed);
  assert.equal(voicePageCreatorTarget(changed, current, a.epoch), undefined);
  assert.equal(voicePagePodcastTarget(changed, current, a.epoch), undefined);
  assert.equal(voicePageCreatorTarget(undefined, current, a.epoch), undefined);
  assert.equal(
    currentVoicePageDetail(fresh, { ...current, entryId: 'different' }, a.epoch),
    undefined,
  );
});
