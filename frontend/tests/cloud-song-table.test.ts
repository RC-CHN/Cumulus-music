import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import { useCloudSongSelection } from '../src/composables/useCloudSongSelection.ts';
import { useCloud } from '../src/composables/useCloud.ts';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import {
  cloudSongColumns,
  cloudSongSelection,
  cloudCapacityText,
  cloudCapacityPercent,
} from '../src/services/cloudSongs.ts';
import { tableColumnWidths, resizeTableColumns } from '../src/services/tableColumns.ts';
import { cloudMenuItems } from '../src/services/cloudMenu.ts';
const oracle = JSON.parse(
  readFileSync(new URL('../../core/tests/fixtures/cloud-song-ui.json', import.meta.url), 'utf8'),
);
const close = (a: number[], b: number[]) => {
  assert.equal(a.length, b.length);
  a.forEach((v, i) => assert(Math.abs(v - b[i]!) < 1e-8, `${v} != ${b[i]}`));
};
test('ordinary cloud columns match the recovered viewer and execute 106 resizer vectors with an explicit extra checkbox', () => {
  assert.deepEqual(
    cloudSongColumns.slice(1),
    oracle.columns.map(({ width, grow, resize }: any) => ({ width, grow, resize })),
  );
  for (const sample of oracle.resizing) {
    let growth = cloudSongColumns.map((c) => c.grow);
    close(tableColumnWidths(sample.width + 28, cloudSongColumns, growth), [28, ...sample.initial]);
    for (const step of sample.steps) {
      growth = resizeTableColumns(
        cloudSongColumns,
        growth,
        step.index + 1,
        step.delta,
        sample.width + 28,
      );
      close(growth, [0, ...step.growth]);
      close(tableColumnWidths(sample.width + 28, cloudSongColumns, growth), [28, ...step.widths]);
    }
  }
});
test('ordinary row plain/control/shift/control-shift and context selection match executed official 106 hooks', () => {
  const ids = Array.from({ length: 8 }, (_, i) => String(i + 1));
  let selected = new Set<string>(),
    anchor: string | undefined;
  for (const event of oracle.selection) {
    if (event.button !== 2 || !selected.has(ids[event.index]!))
      ({ selected, anchor } = cloudSongSelection(ids, selected, event.index, anchor, {
        ctrl: event.ctrl,
        shift: event.shift,
      }));
    assert.deepEqual([...selected], event.selected);
  }
});
test('cloud capacity rounding is the executed official binary G/TB formatter, without fabricated unknown values', () => {
  for (const entry of oracle.formats) assert.equal(cloudCapacityText(entry.bytes), entry.text);
  for (const value of [null, undefined, NaN, -1]) assert.equal(cloudCapacityText(value), '—');
  assert.equal(cloudCapacityPercent(null, 1024), undefined);
  assert.equal(cloudCapacityPercent(5, 0), undefined);
  assert.equal(cloudCapacityPercent(0, 1024), 0);
  assert.equal(cloudCapacityPercent(1, 100000), 0.1);
  assert.equal(cloudCapacityPercent(200, 100), 100);
});
test('a native local association fault keeps ordinary cloud playback and other actions available', () => {
  const target: any = {
    epoch: '1',
    revision: 'r',
    clickedId: '1',
    selection: false,
    query: { tab: 'uploaded' },
    tracks: [],
  };
  const resource: any = {
    epoch: '1',
    revision: 'r',
    track: { track: { id: '1' }, publicSongId: '9001' },
    localAvailable: false,
    localIssue: '本地文件关联暂不可用，其他歌曲操作仍可使用',
  };
  const items = cloudMenuItems(target, resource, {
    locked: false,
    playable: true,
    queueable: true,
    fm: false,
  });
  assert(items.find((row) => row.key === 'local-issue')?.disabled);
  assert.equal(items.find((row) => row.key === 'play')?.disabled, false);
  assert.equal(items.find((row) => row.key === 'remove')?.disabled, false);
  assert(!items.some((row) => row.key === 'open'));
});

test('changing a cloud query clears feedback that belonged to the previous rows', () => {
  const model = useCloud({} as any);
  model.notice.value = 'previous playback';
  model.error.value = 'previous selection';
  model.filter({ keyword: 'new song' });
  assert.equal(model.notice.value, '');
  assert.equal(model.error.value, '');
  model.notice.value = 'same-query acknowledgement';
  model.filter({ keyword: 'new song' });
  assert.equal(model.notice.value, 'same-query acknowledgement');
  model.dispose();
});

test('successive checkbox/control events retain the preceding change before controlled props are patched', () => {
  const scope = effectScope();
  const external = ref(new Set<string>()),
    changes: Set<string>[] = [];
  const state = scope.run(() =>
    useCloudSongSelection(
      () => ['1', '2', '3'],
      () => external.value,
      (value) => changes.push(value),
    ),
  )!;
  state.choose(0, true);
  state.choose(1, true);
  assert.deepEqual([...state.selected.value], ['1', '2']);
  assert.deepEqual(
    changes.map((v) => [...v]),
    [['1'], ['1', '2']],
  );
  external.value = new Set(['3']);
  assert.deepEqual([...state.selected.value], ['3']);
  state.all();
  state.choose(1, true);
  assert.deepEqual([...state.selected.value], ['1', '3']);
  scope.stop();
});
