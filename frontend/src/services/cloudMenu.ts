import type { CloudQuery, CloudResource, CloudTrack } from './cloud';
import type { TrackView } from './contracts';
import type { MenuItem } from './menu';
export interface CloudMenuTarget {
  epoch: string;
  revision: string;
  query: CloudQuery;
  clickedId: string;
  selection: boolean;
  tracks: CloudTrack[];
}
export function cloudMenuTarget(
  rows: CloudTrack[],
  selected: Set<string>,
  clickedId: string,
  epoch: string,
  revision: string,
  query: CloudQuery,
): CloudMenuTarget | undefined {
  const row = rows.find((t) => t.track.id === clickedId);
  if (!row || !epoch || !revision || query.tab !== 'uploaded') return;
  const selection = selected.has(clickedId);
  return {
    epoch,
    revision,
    query: { ...query },
    clickedId,
    selection,
    tracks: selection ? rows.filter((t) => selected.has(t.track.id)) : [row],
  };
}
export function cloudShareTrack(resource: CloudResource): TrackView | undefined {
  const id = resource.track.publicSongId;
  return id && /^[1-9][0-9]*$/.test(id) ? { ...resource.track.track, id, local: false } : undefined;
}
// Official M/556 track menu in the cloud scene; download/comment/copy are
// filtered there. M/35's unavailable share condition is projected by core.
export function cloudMenuItems(
  target: CloudMenuTarget,
  resource: CloudResource,
  state: { locked: boolean; playable: boolean; queueable: boolean; fm: boolean },
): MenuItem[] {
  const share = !target.selection && cloudShareTrack(resource);
  return [
    ...(resource.localIssue
      ? [{ key: 'local-issue', label: resource.localIssue, disabled: true }]
      : []),
    { key: 'play', label: '播放', icon: 'play', disabled: state.locked || !state.playable },
    {
      key: 'next',
      label: '下一首播放',
      icon: 'next',
      disabled: state.locked || !state.queueable,
      hint: state.fm ? '请先退出私人漫游再添加到普通播放列表' : undefined,
    },
    { key: 'resource-divider', label: '', separator: true },
    { key: 'collect', label: '收藏', icon: 'star', disabled: state.locked },
    ...(share ? [{ key: 'share', label: '分享', icon: 'share', disabled: state.locked }] : []),
    ...(resource.localAvailable
      ? [{ key: 'open', label: '打开文件所在目录', icon: 'list', disabled: state.locked }]
      : []),
    { key: 'delete-divider', label: '', separator: true },
    { key: 'remove', label: '从网盘中删除', icon: 'trash', danger: true, disabled: state.locked },
  ];
}
