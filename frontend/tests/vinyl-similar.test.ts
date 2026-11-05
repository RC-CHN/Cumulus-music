import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, ref } from 'vue';
import { useVinylSimilar } from '../src/composables/useVinylSimilar.ts';
import type { VinylSimilarSeed, VinylSimilarSong } from '../src/services/vinylSimilar.ts';

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
            receipt: 'native-snapshot',
            songs: [row('51', false), row('52')],
          };
        },
        playerVinylRelatedPlaylists: async (_epoch, playId) => {
          playlistReads++;
          return {
            playId,
            songId: '10',
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
            : Promise.resolve({ playId, songId: '11', receipt: 'new', songs: [row('80')] }),
        playerVinylRelatedPlaylists: async (_epoch, playId) => ({
          playId,
          songId: playId === 'old' ? '10' : '11',
          playlists: [{ id: playId, name: 'Playlist', coverUrl: null, playCount: 2 }],
        }),
      },
      seed,
      visible,
      async (_target, receipt) => {
        sends.push(receipt);
        return true;
      },
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
    old.complete({ playId: 'old', songId: '10', receipt: 'old', songs: [row('41')] });
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
