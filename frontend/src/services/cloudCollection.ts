import type { CloudClient, CloudQuery, CloudTrack } from './cloud';
export interface CloudCollectionData {
  epoch: string;
  revision: string;
  tracks: CloudTrack[];
  total: number;
}
/** Read a complete, consistent native catalog for a captured filter/order. */
export async function readCloudCollection(
  client: Pick<CloudClient, 'cloudPage'>,
  epoch: string,
  query: CloudQuery,
  active: () => boolean,
  progress: (loaded: number, total: number) => void,
  expected?: { revision: string; maximum: number; action: string },
): Promise<CloudCollectionData> {
  const tracks: CloudTrack[] = [],
    ids = new Set<string>(),
    captured = { ...query, tab: 'uploaded' as const, offset: 0, limit: 200 };
  let revision: string | undefined, total: number | undefined;
  while (active()) {
    const offset = tracks.length;
    const page = await client.cloudPage(epoch, { ...captured, offset }, offset === 0);
    if (!active() || page.epoch !== epoch) throw { code: 'stale', message: '读取已取消' };
    if (
      !page.revision ||
      page.offset !== offset ||
      !Number.isSafeInteger(page.total) ||
      page.total < 0 ||
      page.total > 200000 ||
      page.tracks.length > 200 ||
      offset + page.tracks.length > page.total
    )
      throw { code: 'invalid_response', message: '云盘列表不完整，请重新读取' };
    if (
      (expected && page.revision !== expected.revision) ||
      (revision && page.revision !== revision) ||
      (total !== undefined && page.total !== total)
    )
      throw { code: 'snapshot_changed', message: '云盘已改变，请刷新列表后重试' };
    if (expected && page.total > expected.maximum)
      throw {
        code: 'selection_limit',
        message: `单次${expected.action}最多 ${expected.maximum} 首歌曲，请缩小搜索范围后重试。`,
      };
    revision = page.revision;
    total = page.total;
    for (const row of page.tracks) {
      if (ids.has(row.track.id))
        throw { code: 'invalid_response', message: '云盘返回重复歌曲，请重新读取' };
      ids.add(row.track.id);
      tracks.push(row);
    }
    progress(tracks.length, total);
    if (tracks.length === total) return { epoch, revision, total, tracks };
    if (!page.tracks.length) throw { code: 'invalid_response', message: '云盘读取未完成，请重试' };
  }
  throw { code: 'stale', message: '读取已取消' };
}
