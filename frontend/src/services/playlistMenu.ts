import type { CoreClient, PlaylistView, TrackView } from './contracts';
import type { MenuItem } from './menu';

export type PlaylistMenuScene = 'collection' | 'detail';
export function playlistLink(playlist: PlaylistView) {
  return !playlist.private && /^\d+$/.test(playlist.id)
    ? `https://music.163.com/#/playlist?id=${playlist.id}`
    : '';
}

/** Official 622.js playlist menu conditions. Queue/batch branches are tracked
 * separately; reorder is a named community extension until drag/drop exists. */
export function playlistMenuItems(
  list: PlaylistView,
  scene: PlaylistMenuScene,
  state: { lists: PlaylistView[]; busy: boolean; playable: boolean; preparing: boolean },
): MenuItem[] {
  const nonempty = list.trackCount > 0;
  const known = state.lists.some((p) => p.id === list.id);
  const group = state.lists.filter((p) => p.owned === list.owned && !p.favorite);
  const index = group.findIndex((p) => p.id === list.id);
  const items: MenuItem[] = [];
  if (scene === 'collection') {
    items.push(
      { key: 'view', label: '查看歌单', icon: 'list' },
      {
        key: 'play',
        label: '播放',
        icon: 'play',
        disabled: !nonempty || !state.playable || state.preparing,
      },
      { key: 'resource-divider', label: '', separator: true },
    );
    if (!known && !list.owned)
      items.push({ key: 'subscribe', label: '收藏', icon: 'star', disabled: state.busy });
  }
  items.push({
    key: 'share',
    label: '分享…',
    icon: 'share',
    disabled: !nonempty || !playlistLink(list),
    hint: list.private ? '私密歌单不能分享' : undefined,
  });
  if (scene === 'collection') {
    items.push(
      {
        key: 'copy',
        label: '复制链接',
        icon: 'copy',
        disabled: !nonempty || !playlistLink(list),
        hint: list.private ? '私密歌单不能分享' : undefined,
      },
      { key: 'download', label: '下载全部', icon: 'download', disabled: !nonempty || state.busy },
    );
    if (!list.favorite && (list.owned || known)) {
      items.push({ key: 'manage-divider', label: '', separator: true });
      if (list.owned)
        items.push({ key: 'edit', label: '编辑歌单信息', icon: 'edit', disabled: state.busy });
      // The official label says 删除歌单 for both kinds. Distinguish the result
      // here and in the confirmation: subscribed playlists are only unfollowed.
      items.push({
        key: 'remove',
        label: list.owned ? '删除歌单' : '取消收藏',
        icon: 'trash',
        danger: true,
        disabled: state.busy,
      });
      if (index >= 0)
        items.push(
          { key: 'order-divider', label: '', separator: true },
          { key: 'up', label: '上移歌单', icon: 'up', disabled: state.busy || index === 0 },
          {
            key: 'down',
            label: '下移歌单',
            icon: 'down',
            disabled: state.busy || index === group.length - 1,
          },
        );
    }
  }
  return items;
}

/** Read the entire revision-bound list, never label one displayed page 全部. */
export async function playlistDownloadTracks(
  client: Pick<CoreClient, 'playlistPage'>,
  epoch: string,
  id: string,
  active: () => boolean,
  progress: (count: number, total: number) => void,
): Promise<TrackView[]> {
  const tracks: TrackView[] = [];
  let offset = 0,
    revision: string | null = null;
  while (active()) {
    const response = await client.playlistPage(id, { offset, limit: 100, revision });
    if (!active() || response.epoch !== epoch) throw { code: 'stale', message: '读取已取消' };
    const page = response.value;
    if (page.offset !== offset || (offset > 0 && (!revision || page.revision !== revision)))
      throw { code: 'stale', message: '歌单已改变，请重新读取' };
    if (page.total > 50000 || offset + page.tracks.length > 50000)
      throw { code: 'invalid_input', message: '歌单过大，无法一次读取' };
    revision = page.revision;
    tracks.push(...page.tracks.filter((t) => !t.missing && !t.local));
    offset += page.tracks.length;
    progress(offset, page.total);
    if (!page.more) return [...new Map(tracks.map((t) => [t.id, t])).values()];
    if (!page.tracks.length) throw { code: 'invalid_response', message: '歌单读取未完成，请重试' };
  }
  throw { code: 'stale', message: '读取已取消' };
}
