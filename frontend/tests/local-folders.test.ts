import assert from 'node:assert/strict';
import { test } from 'node:test';
import { localFolderTree, visibleLocalFolders } from '../src/services/localFolders.ts';

const group = (key: string, count = 1) => ({ key, name: key, count });
const displayed = (keys: string[], expanded: string[]) =>
  visibleLocalFolders(
    localFolderTree(keys.map((key, index) => group(key, index + 1))),
    new Set(expanded),
  ).map(({ node, depth }) => [node.group.key, node.name, node.group.count, depth]);

test('disclosure preserves exact folder keys/counts without inventing selectable ancestors', () => {
  const keys = ['/music', '/music/artist/live', '/music/disc', '/music-old/disc', '/music/猫\\音'];
  assert.deepEqual(displayed(keys, []), [
    ['/music', '/music', 1, 0],
    ['/music-old/disc', '/music-old/disc', 4, 0],
  ]);
  assert.deepEqual(displayed(keys, ['/music']), [
    ['/music', '/music', 1, 0],
    ['/music/artist/live', 'live', 2, 1],
    ['/music/disc', 'disc', 3, 1],
    ['/music/猫\\音', '猫\\音', 5, 1],
    ['/music-old/disc', '/music-old/disc', 4, 0],
  ]);
});

test('Windows drives and UNC shares cannot become ancestors of one another', () => {
  const keys = [
    'C:\\',
    String.raw`C:\music`,
    String.raw`C:\music\disc`,
    String.raw`D:\music\disc`,
    String.raw`\\server\share`,
    String.raw`\\server\share\music`,
    String.raw`\\server\shared\music`,
  ];
  const rows = displayed(keys, [keys[0], keys[1], keys[4]]);
  assert.deepEqual(
    rows.map((row) => row[3]),
    [0, 1, 2, 0, 0, 1, 0],
  );
  assert.deepEqual(
    rows.map((row) => row[0]),
    keys,
  );
});
