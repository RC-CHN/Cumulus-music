import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useCommentResource } from '../src/composables/useCommentResource.ts';
import {
  commentResourceSelection,
  currentCommentResource,
  type CommentResourceClient,
  type CommentResourceCard,
} from '../src/services/commentResource.ts';
import type { PlayerView } from '../src/services/player.ts';
const target = { kind: 'track' as const, id: '1' };
const track: CommentResourceCard = {
  kind: 'track',
  track: {
    id: '1',
    name: 'Song',
    artists: [],
    album: '',
    coverUrl: null,
    durationMs: 0,
    fee: 0,
    missing: false,
    local: false,
  },
};
const reply = (epoch = '1', id = '1', cached = false) => ({
  epoch,
  value: { resource: { ...target, id }, card: { ...track, track: { ...track.track, id } }, cached },
});
test('metadata failure can show its own cached card and retry without blocking comments', async () => {
  let failed = true;
  const calls: boolean[] = [];
  const core: CommentResourceClient = {
    commentsResource: async (e, r, refresh) => {
      calls.push(refresh);
      if (refresh && failed) throw { code: 'network', message: 'offline' };
      return reply(e, r.id, !refresh);
    },
  };
  const m = useCommentResource(core, () => assert.fail('must not expire'));
  await m.connect('1', target);
  assert.equal(m.error.value, 'offline');
  assert.equal(m.view.value?.cached, true);
  failed = false;
  await m.read();
  assert.equal(m.error.value, '');
  assert.equal(m.view.value?.cached, false);
  assert.deepEqual(calls, [true, false, true]);
});
test('resource and account switches discard both fresh and cached late replies', async () => {
  let release: (v: ReturnType<typeof reply>) => void = () => {};
  const core: CommentResourceClient = {
    commentsResource: async (e, r) =>
      r.id === '1'
        ? new Promise((resolve) => {
            release = resolve;
          })
        : reply(e, r.id),
  };
  const m = useCommentResource(core, () => {});
  const old = m.connect('1', target);
  await m.connect('2', { ...target, id: '2' });
  release(reply());
  await old;
  assert.equal(m.view.value?.resource.id, '2');
  assert.equal(m.loading.value, false);
  const pending = m.connect('2', target);
  m.disconnect();
  release(reply('2'));
  await pending;
  assert.equal(m.view.value, undefined);
});
test('wrong resource identity and expiration never publish a foreign header', async () => {
  let mode = 'wrong',
    expired = 0;
  const calls: boolean[] = [];
  const m = useCommentResource(
    {
      commentsResource: async (_e, _r, fresh) => {
        calls.push(fresh);
        if (mode === 'expired') throw { code: 'session_expired', message: 'expired' };
        return reply('1', '9');
      },
    },
    () => expired++,
  );
  await m.connect('1', target);
  assert.equal(m.view.value, undefined);
  assert.ok(m.error.value);
  assert.deepEqual(calls, [true, false]);
  mode = 'expired';
  await m.read();
  assert.equal(expired, 1);
  assert.equal(m.view.value, undefined);
  assert.equal(m.loading.value, false);
  assert.deepEqual(calls, [true, false, true]);
});
test('playback keeps voice and online song namespaces distinct and restarts stopped resources', () => {
  const voice: CommentResourceCard = {
    kind: 'voice',
    episode: {
      id: '81',
      name: 'Voice',
      coverUrl: null,
      creator: '',
      radioId: null,
      radioName: '',
      durationMs: 0,
      feeType: 0,
      liked: false,
      unavailable: false,
    },
  };
  const current = {
    playId: 'p',
    state: 'paused',
    resourceKind: 'catalog',
    track: { id: '1' },
    qualityPreferences: { quality: 'hires' },
  } as PlayerView;
  assert.equal(currentCommentResource(track, current), true);
  assert.equal(currentCommentResource(track, { ...current, state: 'idle' }), false);
  assert.equal(currentCommentResource(track, { ...current, resourceKind: 'cloud' }), false);
  assert.equal(
    currentCommentResource(voice, {
      ...current,
      resourceKind: 'voice',
      track: { ...track.track, voiceId: '81' },
    }),
    true,
  );
  assert.equal(
    currentCommentResource(voice, {
      ...current,
      resourceKind: 'voice',
      track: { ...track.track, id: '81' },
    }),
    false,
  );
  assert.deepEqual(commentResourceSelection(track, current), {
    kind: 'songs',
    ids: ['1'],
    index: 0,
    quality: 'hires',
  });
  assert.deepEqual(commentResourceSelection(voice, current), { kind: 'voice', id: '81' });
  assert.equal(
    commentResourceSelection({ kind: 'mv', id: '123', title: '', coverUrl: null, artists: [] }),
    undefined,
  );
});
