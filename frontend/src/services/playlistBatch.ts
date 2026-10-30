import type { CoreClient, TrackPage } from './contracts';

/** Official batch pages wait for the entire revision-bound playlist. */
export async function readPlaylistBatch(
  client: Pick<CoreClient, 'playlistPage'>,
  epoch: string,
  id: string,
  active: () => boolean,
  progress: (loaded: number, total: number) => void,
): Promise<TrackPage & { revision: string }> {
  let result: TrackPage | undefined;
  const tracks: TrackPage['tracks'] = [];
  while (active()) {
    const offset = result?.tracks.length ?? 0;
    const response = await client.playlistPage(id, {
      offset,
      limit: 100,
      revision: result?.revision ?? null,
    });
    if (!active() || response.epoch !== epoch) throw { code: 'stale', message: '读取已取消' };
    const page = response.value;
    if (!page.revision) throw { code: 'invalid_response', message: '歌单信息不完整，请重新读取' };
    if (page.offset !== offset || (result && page.revision !== result.revision))
      throw { code: 'snapshot_changed', message: '歌单已改变，请重新读取' };
    if (page.total > 50000 || offset + page.tracks.length > 50000)
      throw { code: 'invalid_input', message: '歌单过大，无法一次读取' };
    tracks.push(...page.tracks);
    result = { ...page, offset: 0, tracks };
    progress(tracks.length, page.total);
    if (!page.more) return { ...result, revision: page.revision };
    if (!page.tracks.length) throw { code: 'invalid_response', message: '歌单读取未完成，请重试' };
  }
  throw { code: 'stale', message: '读取已取消' };
}

export function toggleBatchSelection(
  ids: string[],
  current: Set<string>,
  index: number,
  anchor: number,
  range: boolean,
) {
  const id = ids[index];
  if (!id) return current;
  const next = new Set(current),
    checked = range && anchor >= 0 ? true : !next.has(id);
  const from = range && anchor >= 0 ? Math.min(anchor, index) : index;
  const to = range && anchor >= 0 ? Math.max(anchor, index) : index;
  for (let i = from; i <= to; i++) {
    if (checked) next.add(ids[i]!);
    else next.delete(ids[i]!);
  }
  return next;
}
