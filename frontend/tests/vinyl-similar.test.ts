import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, ref } from 'vue';
import { useVinylSimilar } from '../src/composables/useVinylSimilar.ts';
import { useVinylWiki } from '../src/composables/useVinylWiki.ts';
import {
  activeVinylDetailTab,
  vinylDetailTabs,
  type VinylRecommendationLane,
  type VinylSimilarSeed,
  type VinylSimilarSong,
  type VinylWikiView,
} from '../src/services/vinylSimilar.ts';

function held<T>() {
  let complete!: (value: T) => void;
  return {
    promise: new Promise<T>((resolve) => {
      complete = resolve;
    }),
    complete,
  };
}

function row(id: string, playable = true): VinylSimilarSong {
  return {
    playable,
    track: {
      id,
      name: `Synthetic ${id}`,
      artists: [],
      album: '',
      coverUrl: null,
      durationMs: 30_000,
      fee: 0,
      missing: false,
      local: false,
    },
  };
}

test('wiki is independent of placement and a hidden saved tab falls back to lyrics', () => {
  assert.deepEqual(vinylDetailTabs(true, 'standalone'), ['lyrics', 'wiki', 'similar']);
  assert.deepEqual(vinylDetailTabs(true, 'wiki'), ['lyrics', 'wiki']);
  assert.deepEqual(vinylDetailTabs(true, 'unresolved'), ['lyrics', 'wiki']);
  assert.equal(activeVinylDetailTab('similar', true, 'wiki'), 'lyrics');
  assert.equal(activeVinylDetailTab('wiki', true, 'unresolved'), 'wiki');
  assert.equal(activeVinylDetailTab('wiki', false, 'standalone'), 'lyrics');
});

test('a placement mismatch never displays recommendation rows or uses their receipt', async () => {
  const scope = effectScope();
  const placement = ref<VinylRecommendationLane>();
  const target = ref({ epoch: '7', playId: 'a', songId: '10' });
  let reads = 0;
  let selected = 0;
  const model = scope.run(() =>
    useVinylSimilar(
      {
        playerVinylSimilarSongs: async (_epoch, playId) => {
          reads++;
          return {
            playId,
            songId: '10',
            placement: 'standalone',
            receipt: 'fixture',
            songs: [row('11')],
          };
        },
        playerVinylRelatedPlaylists: async (_epoch, playId) => ({
          playId,
          songId: '10',
          placement: 'wiki',
          playlists: [],
        }),
      },
      target,
      ref(true),
      async () => {
        selected++;
        return true;
      },
      placement,
    ),
  )!;
  try {
    await nextTick();
    assert.equal(reads, 0);
    placement.value = 'wiki';
    await nextTick();
    await nextTick();
    assert.equal(model.songsState.value, 'error');
    assert.equal(model.playlistsState.value, 'ready');
    assert.deepEqual(model.songs.value, []);
    await model.select({ kind: 'header' });
    assert.equal(selected, 0);
    placement.value = 'standalone';
    await nextTick();
    await nextTick();
    assert.equal(model.songs.value[0].track.id, '11');
    assert.equal(model.playlistsState.value, 'error');
    await model.select({ kind: 'header' });
    assert.equal(selected, 1);
  } finally {
    scope.stop();
  }
});

test('wiki waits for the open tab and a late response cannot replace another song', async () => {
  const scope = effectScope();
  const seed = ref({ epoch: '7', playId: 'old', songId: '10' });
  const visible = ref(false);
  const old = held<VinylWikiView>();
  let reads = 0;
  const model = scope.run(() =>
    useVinylWiki(
      {
        playerVinylWiki: async (_epoch, playId) => {
          reads++;
          if (playId === 'old') return old.promise;
          if (reads === 2) throw new Error('fixture error');
          return { playId, songId: '11', card: null };
        },
      },
      seed,
      visible,
    ),
  )!;
  try {
    await nextTick();
    assert.equal(reads, 0);
    visible.value = true;
    await nextTick();
    assert.equal(model.state.value, 'loading');
    seed.value = { epoch: '7', playId: 'new', songId: '11' };
    await nextTick();
    await nextTick();
    assert.equal(model.state.value, 'error');
    await model.retry();
    assert.equal(model.state.value, 'ready');
    assert.equal(model.card.value, null);
    old.complete({
      playId: 'old',
      songId: '10',
      card: { metas: [{ title: 'old', content: 'old' }] },
    });
    await nextTick();
    await nextTick();
    assert.equal(model.state.value, 'ready');
    assert.equal(model.card.value, null);
    assert.equal(reads, 3);
  } finally {
    scope.stop();
  }
});

test('an overlay preserves the Wiki card but an actual tab remount rereads the same play', async () => {
  const scope = effectScope();
  const seed = ref({ epoch: '7', playId: 'same', songId: '10' });
  const visible = ref(true);
  const mounted = ref(true);
  let reads = 0;
  const model = scope.run(() =>
    useVinylWiki(
      {
        playerVinylWiki: async (_epoch, playId) => ({
          playId,
          songId: '10',
          card: { metas: [{ title: 'fixture', content: String(++reads) }] },
        }),
      },
      seed,
      visible,
      mounted,
    ),
  )!;
  try {
    await nextTick();
    assert.equal(model.card.value?.metas[0].content, '1');
    visible.value = false;
    await nextTick();
    visible.value = true;
    await nextTick();
    assert.equal(reads, 1);
    mounted.value = false;
    visible.value = false;
    assert.equal(model.card.value, null);
    await nextTick();
    mounted.value = true;
    visible.value = true;
    await nextTick();
    await nextTick();
    assert.equal(model.card.value?.metas[0].content, '2');
  } finally {
    scope.stop();
  }
});

test('independent cards, explicit retry and captured row action survive a failed sibling read', async () => {
  const scope = effectScope();
  const target = { epoch: '7', playId: 'a', songId: '10' };
  const seed = ref<VinylSimilarSeed>(target);
  const visible = ref(true);
  const commit = held<boolean>();
  const calls: unknown[] = [];
  let songReads = 0;
  let playlistReads = 0;
  const model = scope.run(() =>
    useVinylSimilar(
      {
        playerVinylSimilarSongs: async (_epoch, playId) => {
          if (++songReads === 1) throw new Error('fixture read unavailable');
          return {
            playId,
            songId: '10',
            placement: 'standalone',
            receipt: 'native-snapshot',
            songs: [row('51', false), row('52')],
          };
        },
        playerVinylRelatedPlaylists: async (_epoch, playId) => {
          playlistReads++;
          return {
            playId,
            songId: '10',
            placement: 'standalone',
            playlists: [{ id: '60', name: 'Playlist', coverUrl: null, playCount: 2 }],
          };
        },
      },
      seed,
      visible,
      (...args) => {
        calls.push(args);
        return commit.promise;
      },
      ref('standalone'),
    ),
  )!;
  try {
    await nextTick();
    assert.equal(model.songsState.value, 'error');
    assert.equal(model.playlistsState.value, 'ready');
    assert.equal(model.playlists.value[0].id, '60');
    model.expand();
    assert.equal(model.expanded.value, true);
    await model.retry('songs');
    assert.equal(songReads, 2);
    assert.equal(playlistReads, 1);
    await model.select({ kind: 'header' });
    assert.equal(calls.length, 0, 'denied first row must not skip ahead');
    const selected = model.select({ kind: 'cover', index: 1 });
    await model.select({ kind: 'double_click', index: 1 });
    assert.deepEqual(calls, [[target, 'native-snapshot', { kind: 'cover', index: 1 }]]);
    commit.complete(false);
    await selected;
    assert.match(model.notice.value ?? '', /重新读取/);
    assert.equal(songReads, 2, 'failed actions must not automatically re-read or replay');
    visible.value = false;
    visible.value = true;
    assert.equal(model.expanded.value, true);
  } finally {
    scope.stop();
  }
});

test('a late old play result cannot replace either card or its receipt after a track change', async () => {
  const scope = effectScope();
  const seed = ref({ epoch: '7', playId: 'old', songId: '10' });
  const visible = ref(true);
  const old = held<{
    playId: string;
    songId: string;
    placement: VinylRecommendationLane;
    receipt: string;
    songs: VinylSimilarSong[];
  }>();
  const sends: string[] = [];
  const model = scope.run(() =>
    useVinylSimilar(
      {
        playerVinylSimilarSongs: (_epoch, playId) =>
          playId === 'old'
            ? old.promise
            : Promise.resolve({
                playId,
                songId: '11',
                placement: 'standalone',
                receipt: 'new',
                songs: [row('80')],
              }),
        playerVinylRelatedPlaylists: async (_epoch, playId) => ({
          playId,
          songId: playId === 'old' ? '10' : '11',
          placement: 'standalone',
          playlists: [{ id: playId, name: 'Playlist', coverUrl: null, playCount: 2 }],
        }),
      },
      seed,
      visible,
      async (_target, receipt) => {
        sends.push(receipt);
        return true;
      },
      ref('standalone'),
    ),
  )!;
  try {
    await nextTick();
    model.expand();
    seed.value = { epoch: '7', playId: 'new', songId: '11' };
    assert.equal(model.expanded.value, false);
    assert.deepEqual(model.playlists.value, []);
    await nextTick();
    await nextTick();
    old.complete({
      playId: 'old',
      songId: '10',
      placement: 'standalone',
      receipt: 'old',
      songs: [row('41')],
    });
    await nextTick();
    assert.equal(model.songs.value[0].track.id, '80');
    assert.equal(model.playlists.value[0].id, 'new');
    await model.select({ kind: 'header' });
    visible.value = false;
    await model.select({ kind: 'cover', index: 0 });
    assert.deepEqual(sends, ['new']);
  } finally {
    scope.stop();
  }
});
