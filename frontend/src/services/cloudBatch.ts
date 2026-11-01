import type { CloudClient, CloudTrack } from './cloud';
export interface CloudBatchData {
  epoch: string;
  revision: string;
  tracks: CloudTrack[];
  total: number;
}
/** Official batch source is the whole cloudOnlineList in its natural order. */
export async function readCloudBatch(
  client: Pick<CloudClient, 'cloudPage'>,
  epoch: string,
  active: () => boolean,
  progress: (loaded: number, total: number) => void,
): Promise<CloudBatchData> {
  const tracks: CloudTrack[] = [],
    ids = new Set<string>();
  let revision: string | undefined, total: number | undefined;
  while (active()) {
    const offset = tracks.length;
    const page = await client.cloudPage(
      epoch,
      { tab: 'uploaded', keyword: '', sort: 'natural', descending: false, offset, limit: 200 },
      offset === 0,
    );
    if (!active() || page.epoch !== epoch) throw { code: 'stale', message: '读取已取消' };
    if (
      !page.revision ||
      page.offset !== offset ||
      page.total < 0 ||
      page.total > 200000 ||
      page.tracks.length > 200 ||
      offset + page.tracks.length > page.total
    )
      throw { code: 'invalid_response', message: '云盘列表不完整，请重新读取' };
    if ((revision && page.revision !== revision) || (total !== undefined && page.total !== total))
      throw { code: 'snapshot_changed', message: '云盘已改变，请重新读取' };
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
