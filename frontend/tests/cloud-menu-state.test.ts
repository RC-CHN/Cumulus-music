import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { effectScope, nextTick, ref } from 'vue';
import { cloudMenuItems, cloudMenuTarget, cloudShareTrack } from '../src/services/cloudMenu.ts';
import { useCloudResource } from '../src/composables/useCloudResource.ts';
import type { CloudQuery, CloudResource, CloudTrack } from '../src/services/cloud.ts';
const query: CloudQuery = {
  tab: 'uploaded',
  keyword: '',
  sort: 'title',
  descending: true,
  offset: 200,
  limit: 200,
};
function row(id = '1001'): CloudTrack {
  return {
    track: {
      id,
      name: 'Synthetic cloud',
      artists: [],
      album: '',
      coverUrl: null,
      durationMs: 1000,
      fee: 0,
      missing: false,
      local: false,
    },
    publicSongId: '9001',
    fileName: 'original.flac',
    fileSize: 123,
    addedAt: 1,
    bitrate: 900000,
    format: 'flac',
  };
}
function resource(id = '1001'): CloudResource {
  return { epoch: '1', revision: 'r1', track: row(id), localAvailable: false };
}
function target(selected = new Set<string>()) {
  return cloudMenuTarget([row()], selected, '1001', '1', 'r1', query)!;
}
function deferred<T>() {
  let resolve!: (v: T) => void, reject!: (v: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { resolve, reject, promise };
}
test('cloud menu order and unavailable/selected/local conditions match executed official vectors', () => {
  const oracle = JSON.parse(
    readFileSync(new URL('../../core/tests/fixtures/cloud_menu.json', import.meta.url), 'utf8'),
  );
  for (const vector of oracle.vectors) {
    const value = resource();
    value.localAvailable = vector.local;
    if (vector.status < 0) value.track.publicSongId = null;
    const items = cloudMenuItems(target(vector.selection ? new Set(['1001']) : undefined), value, {
      locked: false,
      playable: true,
      queueable: true,
      fm: false,
    });
    assert.deepEqual(
      items.map((i) => ({ label: i.label, separator: !!i.separator, enabled: !i.disabled })),
      vector.rows.map((i: { label: string; separator: boolean; enabled: boolean }) => ({
        label: i.label,
        separator: i.separator,
        enabled: i.enabled,
      })),
    );
    assert(!items.some((i) => ['download', 'comments', 'copy'].includes(i.key)));
  }
});
test('row target freezes visible selection order/query, while links require separate verified catalog identity', () => {
  const rows = [row('3'), row('1'), row('2')],
    selected = new Set(['2', '3']);
  const input = { ...query };
  const value = cloudMenuTarget(rows, selected, '3', '1', 'r1', input)!;
  input.keyword = 'changed';
  selected.clear();
  assert.equal(value.selection, true);
  assert.deepEqual(
    value.tracks.map((t) => t.track.id),
    ['3', '2'],
  );
  assert.equal(value.query.keyword, '');
  assert.equal(cloudMenuTarget(rows, new Set(['2']), '3', '1', 'r1', query)!.selection, false);
  assert.equal(cloudShareTrack(resource())!.id, '9001');
  assert.equal(resource().track.track.id, '1001');
  for (const id of [null, undefined, '0', 'https://invalid.example/private']) {
    const v = resource();
    v.track.publicSongId = id;
    assert.equal(cloudShareTrack(v), undefined);
  }
  const disabled = cloudMenuItems(target(), resource(), {
    locked: true,
    playable: true,
    queueable: true,
    fm: false,
  });
  assert(disabled.filter((i) => !i.separator).every((i) => i.disabled));
  const fm = cloudMenuItems(target(), resource(), {
    locked: false,
    playable: true,
    queueable: false,
    fm: true,
  });
  assert(fm.find((i) => i.key === 'next')?.disabled);
  assert(fm.find((i) => i.key === 'next')?.hint);
});
test('resource lookup retries only explicitly and rejects mismatched identity before exposing actions', async () => {
  const epoch = ref('1'),
    revision = ref('r1'),
    held = deferred<CloudResource>();
  let calls = 0;
  const scope = effectScope();
  const model = scope.run(() =>
    useCloudResource(
      {
        cloudResource: async () => {
          calls++;
          return calls === 1 ? held.promise : resource();
        },
      },
      () => epoch.value,
      () => revision.value,
      () => {},
    ),
  )!;
  model.set(target());
  const pending = model.read();
  await model.read();
  assert.equal(calls, 1);
  held.resolve({ ...resource(), epoch: '2' });
  await pending;
  assert.equal(model.resource.value, undefined);
  assert.equal(model.needsRefresh.value, true);
  assert(model.error.value);
  await model.read();
  assert.equal(calls, 2);
  assert.equal(model.resource.value?.track.track.id, '1001');
  assert.equal(model.error.value, '');
  scope.stop();
});
test('query replacement, account switch and disposal discard resource replies and clear private context', async () => {
  for (const change of ['query', 'account', 'dispose']) {
    const epoch = ref('1'),
      revision = ref<string | undefined>('r1'),
      held = deferred<CloudResource>();
    let calls = 0;
    const scope = effectScope();
    const model = scope.run(() =>
      useCloudResource(
        {
          cloudResource: async () => {
            calls++;
            return held.promise;
          },
        },
        () => epoch.value,
        () => revision.value,
        () => {},
      ),
    )!;
    model.set(target());
    const pending = model.read();
    if (change === 'query') revision.value = undefined;
    else if (change === 'account') epoch.value = '2';
    else scope.stop();
    await nextTick();
    held.resolve(resource());
    await pending;
    assert.equal(model.resource.value, undefined);
    assert.equal(model.target.value, undefined);
    assert.equal(model.active(), false);
    await model.read();
    assert.equal(calls, 1);
    scope.stop();
  }
});
test('expired resource lookup reports session change without retrying or retaining a result', async () => {
  const scope = effectScope();
  let expired = 0,
    calls = 0;
  const model = scope.run(() =>
    useCloudResource(
      {
        cloudResource: async () => {
          calls++;
          throw { code: 'session_expired', message: '登录已过期' };
        },
      },
      () => '1',
      () => 'r1',
      () => expired++,
    ),
  )!;
  model.set(target());
  await model.read();
  assert.equal(calls, 1);
  assert.equal(expired, 1);
  assert.equal(model.error.value, '登录已过期');
  assert.equal(model.loading.value, false);
  scope.stop();
});
