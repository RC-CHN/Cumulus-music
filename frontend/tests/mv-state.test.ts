import { test } from 'node:test';
import assert from 'node:assert/strict';
import { decodeVideoFrame } from '../src/services/mv.ts';
import type { VideoView, MvDetail, MvPage } from '../src/services/mv.ts';
import type { AccountData, CoreClient } from '../src/services/contracts.ts';
import { useMv } from '../src/composables/useMv.ts';
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { resolve, promise };
}
const turn = () => new Promise((r) => setImmediate(r));
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: null,
  avatarUrl: null,
});
const video = (epoch = '1', revision = '1', id: string | null = null): VideoView => ({
  epoch,
  revision,
  playId: id ? `play-${id}` : '',
  id,
  state: id ? 'playing' : 'idle',
  requestedResolution: 480,
  actualResolution: id ? 240 : null,
  local: false,
  position: 0,
  duration: 120,
  paused: !id,
  buffering: false,
  volume: 60,
  speed: 1,
  error: null,
});
const detail = (id: string): MvDetail => ({
  card: {
    id,
    title: id,
    artists: [],
    coverUrl: null,
    durationMs: 120000,
    playCount: 0,
    subscribed: false,
  },
  description: '',
  publishedDate: '',
  playResolutions: [240],
  downloads: [],
  cached: false,
  liked: false,
  likeCount: 0,
  commentCount: 0,
  likeOperation: null,
  relatedSongs: [],
  relatedPrivilegeFailed: false,
});
const page = (epoch = '1'): MvPage => ({
  epoch,
  revision: 'collection',
  cards: [],
  total: 0,
  offset: 0,
  operations: [],
});
test('binary frames reject malformed sizes and preserve 64-bit sequence without JSON expansion', () => {
  const buffer = new ArrayBuffer(16 + 17 * 19 * 4),
    header = new DataView(buffer);
  header.setUint32(0, 17, true);
  header.setUint32(4, 19, true);
  header.setBigUint64(8, 9007199254740993n, true);
  const frame = decodeVideoFrame(buffer)!;
  assert.equal(frame.sequence, 9007199254740993n);
  assert.equal(frame.pixels.byteLength, 17 * 19 * 4);
  assert.equal(decodeVideoFrame(new ArrayBuffer(0)), null);
  assert.throws(() => decodeVideoFrame(new ArrayBuffer(8)));
  header.setUint32(0, 1921, true);
  assert.throws(() => decodeVideoFrame(buffer));
});
test('account switch discards pending detail collection and video responses', async () => {
  const d = deferred<AccountData<MvDetail>>(),
    v = deferred<VideoView>(),
    p = deferred<MvPage>();
  const core = {
    videoSnapshot: async (e: string) => video(e),
    videoAction: () => v.promise,
    mvDetail: () => d.promise,
    mvPage: () => p.promise,
  } as unknown as CoreClient;
  const m = useMv(core);
  m.connect(account());
  const opening = m.open('123'),
    reading = m.load();
  m.connect(account('2'));
  d.resolve({ epoch: '1', value: detail('123') });
  v.resolve(video('1', '900', '123'));
  p.resolve(page());
  await Promise.all([opening, reading]);
  await turn();
  assert.equal(m.video.value?.epoch, '2');
  assert.equal(m.detail.value, undefined);
  assert.equal(m.page.value, undefined);
  m.dispose();
});
test('slow metadata cannot start old video and old snapshots cannot replace a newer action', async () => {
  const old = deferred<AccountData<MvDetail>>();
  const calls: string[] = [];
  const m = useMv({
    videoSnapshot: async () => video(),
    videoAction: async (_e: string, a: { id?: string }) => {
      calls.push(a.id!);
      return video('1', a.id === '1' ? '2' : '3', a.id);
    },
    mvDetail: async (_e: string, id: string) =>
      id === '1' ? old.promise : { epoch: '1', value: detail(id) },
  } as unknown as CoreClient);
  m.connect(account());
  await turn();
  const first = m.open('1');
  await m.open('2');
  old.resolve({ epoch: '1', value: detail('1') });
  await first;
  assert.deepEqual(calls, ['1', '2']);
  assert.equal(m.detail.value?.card.id, '2');
  assert.equal(m.video.value?.id, '2');
  m.dispose();
});
test('uncertain collection errors remain visible after readback and account changes discard late download success', async () => {
  const dl = deferred<{ epoch: string; accepted: string[] }>();
  const m = useMv({
    videoSnapshot: async (e: string) => video(e),
    videoAction: async () => video('1', '2', '1'),
    mvDetail: async () => ({ epoch: '1', value: detail('1') }),
    mvCollect: async () => {
      throw { code: 'uncertain', message: '待核对' };
    },
    mvPage: async (e: string) => page(e),
    downloadCommand: () => dl.promise,
  } as unknown as CoreClient);
  m.connect(account());
  await m.open('1');
  await m.collect('1', true);
  assert.equal(m.error.value, '待核对');
  const downloading = m.download(240);
  m.connect(account('2'));
  dl.resolve({ epoch: '1', accepted: ['old'] });
  await downloading;
  assert.equal(m.notice.value, '');
  assert.equal(m.busy.value, false);
  m.dispose();
});

test('failed refresh exposes cached pending operations without hiding the network error', async () => {
  const requests: boolean[] = [];
  const m = useMv({
    videoSnapshot: async () => video(),
    mvPage: async (e: string, _query: unknown, refresh: boolean) => {
      requests.push(refresh);
      if (refresh) throw { code: 'network', message: '网络读取失败' };
      return {
        ...page(e),
        operations: [{ id: '123', desired: true, observed: null, phase: 'uncertain' }],
      };
    },
  } as unknown as CoreClient);
  m.connect(account());
  await m.load(true);
  assert.deepEqual(requests, [true, false]);
  assert.equal(m.page.value?.operations[0]?.phase, 'uncertain');
  assert.equal(m.error.value, '网络读取失败');
  m.dispose();
});

test('shrinking collection returns to a valid page and playback errors survive state polling', async () => {
  const m = useMv({
    videoSnapshot: async () => video('1', '50', '1'),
    videoAction: async () => {
      throw { code: 'invalid', message: '控制失败' };
    },
    mvPage: async (e: string, q: { offset: number }) => ({
      ...page(e),
      total: 1,
      offset: q.offset,
    }),
  } as unknown as CoreClient);
  m.connect(account());
  m.query.value = { ...m.query.value, offset: 24 };
  await m.load(true);
  assert.equal(m.query.value.offset, 0);
  await m.action({ kind: 'seek', seconds: 999 });
  await new Promise((r) => setTimeout(r, 250));
  assert.equal(m.videoError.value, '控制失败');
  assert.equal(m.videoReadError.value, '');
  m.dispose();
});
