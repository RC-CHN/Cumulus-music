import test from 'node:test';
import assert from 'node:assert/strict';
import { useFollowNewSong } from '../src/composables/useFollowNewSong.ts';
import type { AccountData, TrackView } from '../src/services/contracts.ts';
import type { FollowNewSongPage } from '../src/services/followNewSong.ts';

const tick = async () => {
  await Promise.resolve();
  await Promise.resolve();
};
const track = (id: string) => ({ id }) as TrackView;
const page = (nextTimestamp: number | null, name: string): FollowNewSongPage => ({
  nextTimestamp,
  latestVisitTime: 99,
  releases: [{ artistName: name }] as FollowNewSongPage['releases'],
});

test('starts play-all independently and retries the same failed page without clearing current releases', async () => {
  const requests: (string | number | null)[] = [];
  let fails = true;
  let finishLatest!: (value: AccountData<TrackView[]>) => void;
  const model = useFollowNewSong(
    {
      followedNewSongLatest(epoch) {
        requests.push(`latest:${epoch}`);
        return new Promise((resolve) => (finishLatest = resolve));
      },
      async followedNewSongPage(epoch, cursor) {
        requests.push(cursor);
        if (cursor !== null && fails) throw { code: 'offline', message: '测试断线' };
        return { epoch, value: cursor === null ? page(95, '首页') : page(null, '次页') };
      },
    },
    () => assert.fail('unexpected session refresh'),
  );
  model.connect('1');
  await tick();
  assert.deepEqual(requests, ['latest:1', null]);
  assert.equal(model.latestPending.value, true);
  assert.equal(model.releases.value[0]?.artistName, '首页');
  await model.read();
  assert.equal(model.error.value, '测试断线');
  assert.equal(model.releases.value[0]?.artistName, '首页');
  fails = false;
  await model.read();
  finishLatest({ epoch: '1', value: [track('30')] });
  await tick();
  assert.deepEqual(requests, ['latest:1', null, 95, 95]);
  assert.deepEqual(
    model.releases.value.map((r) => r.artistName),
    ['首页', '次页'],
  );
  assert.equal(model.latest.value[0]?.id, '30');
  assert.equal(model.cursor.value, null);
});

test('old release or play-all responses cannot appear after switching accounts', async () => {
  let oldPage!: (reply: AccountData<FollowNewSongPage>) => void;
  let oldLatest!: (reply: AccountData<TrackView[]>) => void;
  const model = useFollowNewSong(
    {
      followedNewSongLatest(epoch) {
        return epoch === 'old'
          ? new Promise((resolve) => (oldLatest = resolve))
          : Promise.resolve({ epoch, value: [track('2')] });
      },
      followedNewSongPage(epoch) {
        return epoch === 'old'
          ? new Promise((resolve) => (oldPage = resolve))
          : Promise.resolve({ epoch, value: page(null, '当前') });
      },
    },
    () => assert.fail('unexpected session refresh'),
  );
  model.connect('old');
  model.connect('new');
  oldPage({ epoch: 'old', value: page(80, '旧') });
  oldLatest({ epoch: 'old', value: [track('1')] });
  await tick();
  assert.equal(model.releases.value[0]?.artistName, '当前');
  assert.equal(model.latest.value[0]?.id, '2');
  assert.equal(model.cursor.value, null);
});

test('four-page render groups retain earlier releases and keep later reads cursor-bound', async () => {
  const calls: (number | null)[] = [];
  const model = useFollowNewSong(
    {
      followedNewSongLatest: async (epoch) => ({ epoch, value: [] }),
      followedNewSongPage: async (epoch, cursor) => {
        calls.push(cursor);
        const next = cursor === null ? 100 : cursor - 1;
        const releases = Array.from({ length: 10 }, () => ({
          artistName: String(next),
        })) as FollowNewSongPage['releases'];
        return {
          epoch,
          value: { releases, nextTimestamp: next === 96 ? null : next, latestVisitTime: null },
        };
      },
    },
    () => assert.fail('unexpected session refresh'),
  );
  model.connect('1');
  await tick();
  await model.read();
  await model.read();
  await model.read();
  assert.equal(model.releases.value.length, 40);
  assert.equal(model.autoMore.value, false);
  await model.read();
  assert.equal(model.group.value, 1);
  assert.equal(model.releases.value.length, 10);
  assert.deepEqual(calls, [null, 100, 99, 98, 97]);
  model.chooseGroup(0);
  assert.equal(model.releases.value.length, 40);
  assert.equal(model.releases.value.at(0)?.artistName, '100');
});
